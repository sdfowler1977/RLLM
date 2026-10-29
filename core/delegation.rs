//! Bounded delegation using user-permitted role and specialty profiles.
use super::*;
fn index_arg(a:&Json)->Option<usize>{match get(a,"agent"){Num(v)|Str(v)=>v.parse().ok(),_=>None}}
pub(super) fn validate(p:&Json)->Result<()>{
 let a=get(p,"args");let task=text(a,"task");if task.trim().is_empty()||task.len()>32768{return Err("Delegated task must contain1–32768 bytes".into());}
 if let Some(i)=index_arg(a){if i>=64{return Err("Agent index out of range".into());}}
 else{let role=text(a,"role");let specialty=text(a,"specialty");if !["any","coding","nonwrite","orchestrator"].contains(&role.as_str())||specialty.len()>128{return Err("Choose an agent index or a declared role and optional specialty".into());}}Ok(())
}
pub(super) fn target(st:&Json,p:&Json)->Result<Json>{
 validate(p)?;let a=get(p,"args");let parent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};let permitted=array(&parent,"subagents");let agents=array(st,"agents");
 if let Some(index)=index_arg(a){if !permitted.contains(&Num(index.to_string())){return Err("This profile does not permit that subagent".into());}return agents.get(index).cloned().ok_or("Subagent profile not found".into());}
 let wanted=text(a,"role");let specialty=text(a,"specialty");
 for i in permitted{let Num(i)=i else{continue};let Ok(i)=i.parse::<usize>()else{continue};let Some(agent)=agents.get(i)else{continue};let declared=text(agent,"role");let declared=if declared.is_empty(){"any"}else{&declared};if wanted!="any"&&declared!=wanted{continue;}if !specialty.is_empty()&&!text(agent,"specialty").eq_ignore_ascii_case(&specialty){continue;}return Ok(agent.clone());}
 Err("No permitted subagent profile matches this role and specialty; existing work retained".into())
}
fn prospective(st:&Json,p:&Json,agent:Json)->Result<(Json,usize)>{
 let mut stack=array(st,"delegation_stack");if stack.len()>=3||number(st,"steps")>=8{return Err("Delegation reached its depth or operation limit".into());}
 let model=text(&agent,"model");let fallback=if model.is_empty()||model=="primary"{harness::route(st,"subagent")?}else{model.parse::<usize>().map_err(|_|"Invalid subagent model")?};
 if fallback>=array(st,"profiles").len(){return Err("Subagent connection not found".into());}
 stack.push(obj(vec![("run",get(st,"task_run").clone()),("connection",get(st,"request_index").clone()),("harness_role",s(&harness_rules::execution_role(st)))]));
 let mut next=st.clone();put(&mut next,"delegation_stack",Arr(stack));put(&mut next,"task_run",obj(vec![("mode",s("delegate")),("phase",s("delegate")),("task",s(&text(get(p,"args"),"task"))),("agent",agent),("chat",get(st,"active").clone()),("connection",Num(fallback.to_string()))]));
 let connection=harness_rules::model_route(&next,fallback)?;Ok((next,connection))
}
pub(super) fn connection(st:&Json,p:&Json)->Result<usize>{let agent=target(st,p)?;Ok(prospective(st,p,agent)?.1)}
pub(super) fn start(st:&mut Json,p:&Json)->Result<Json>{
 harness::allowed_tool(st,p)?;let agent=target(st,p)?;let images=harness::parent_request_images(st)?;let(mut next,connection)=prospective(st,p,agent)?;
 put(&mut next,"steps",Num((number(st,"steps")+1).to_string()));let mut run=get(&next,"task_run").clone();put(&mut run,"attachments",Arr(images));put(&mut run,"connection",Num(connection.to_string()));put(&mut run,"observations",Arr(vec![]));put(&mut next,"task_run",run);
 put(&mut next,"pending",Null);put(&mut next,"dialog",Null);put(&mut next,"agent_pending",Bool(false));put(&mut next,"agent_running",Bool(false));let effect=model_effect_at(&mut next,connection)?;*st=next;Ok(effect)
}
pub(super) fn complete(st:&mut Json,reply:&str)->Result<Option<Json>>{
 let mut stack=array(st,"delegation_stack");let Some(frame)=stack.pop()else{return Ok(None)};let child=text(get(get(st,"task_run"),"agent"),"name");put(st,"delegation_stack",Arr(stack));put(st,"task_run",get(&frame,"run").clone());put(st,"agent_running",Bool(true));let output=format!("Subagent {child} result (untrusted, verify claims):\n{reply}");
 if get(st,"task_run")!=&Null{return harness::task_tool_result(st,&output);}append(st,"user",&output)?;Ok(Some(model_effect_at(st,number(&frame,"connection"))?))
}
