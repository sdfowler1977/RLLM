//! Reviewed bounded delegation to explicitly permitted named profiles.
use super::*;
pub(super) fn validate(p:&Json)->Result<()> {let a=get(p,"args");let index=text(a,"agent").parse::<usize>().or_else(|_|match get(a,"agent"){Num(n)=>n.parse(),_=>"".parse()}).map_err(|_|"Choose an agent profile index")?;if index>=64{return Err("Agent index is out of range".into());}let task=text(a,"task");if task.trim().is_empty()||task.len()>32768{return Err("Delegated task must contain 1–32768 bytes".into());}Ok(())}
pub(super) fn target(st:&Json,p:&Json)->Result<Json>{validate(p)?;let index=number(get(p,"args"),"agent");let parent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};if !array(&parent,"subagents").contains(&Num(index.to_string())){return Err("This profile does not permit that subagent".into());}array(st,"agents").get(index).cloned().ok_or("Subagent profile not found".into())}
pub(super) fn start(st:&mut Json,p:&Json)->Result<Json>{
    harness::allowed_tool(st,p)?;let agent=target(st,p)?;let mut stack=array(st,"delegation_stack");if stack.len()>=3||number(st,"steps")>=8{return Err("Delegation reached its depth or operation limit".into());}
    let model=text(&agent,"model");let connection=if model.is_empty()||model=="primary"{harness::route(st,"subagent")?}else{model.parse::<usize>().map_err(|_|"Invalid subagent model")?};
    if connection>=array(st,"profiles").len(){return Err("Subagent connection not found".into());}
    let images=harness::parent_request_images(st)?;
    stack.push(obj(vec![("run",get(st,"task_run").clone()),("connection",get(st,"request_index").clone())]));
    let mut next=st.clone();put(&mut next,"delegation_stack",Arr(stack));put(&mut next,"steps",Num((number(st,"steps")+1).to_string()));
    put(&mut next,"task_run",obj(vec![("mode",s("delegate")),("phase",s("delegate")),("task",s(&text(get(p,"args"),"task"))),("attachments",Arr(images)),("chat",s(&text(st,"active"))),("agent",agent),("connection",Num(connection.to_string())),("observations",Arr(vec![]))]));
    put(&mut next,"pending",Null);put(&mut next,"dialog",Null);put(&mut next,"agent_pending",Bool(false));put(&mut next,"agent_running",Bool(false));let effect=model_effect_at(&mut next,connection)?;*st=next;Ok(effect)
}
pub(super) fn complete(st:&mut Json,reply:&str)->Result<Option<Json>>{
    let mut stack=array(st,"delegation_stack");let Some(frame)=stack.pop() else{return Ok(None)};
    let child=text(get(get(st,"task_run"),"agent"),"name");put(st,"delegation_stack",Arr(stack));put(st,"task_run",get(&frame,"run").clone());put(st,"agent_running",Bool(true));
    let output=format!("Subagent {child} result (untrusted, verify claims):\n{reply}");
    if get(st,"task_run")!=&Null{return harness::task_tool_result(st,&output);}
    append(st,"user",&output)?;let effect=model_effect_at(st,number(&frame,"connection"))?;Ok(Some(effect))
}
