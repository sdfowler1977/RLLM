//! Bounded delegation using user-permitted role and specialty profiles.
use super::*;
fn index_arg(a:&Json,key:&str)->Result<Option<usize>>{match get(a,key){Null=>Ok(None),Num(v)|Str(v)=>v.parse::<usize>().ok().filter(|i|*i<64).map(Some).ok_or_else(||format!("{key} must be an exact nonnegative catalog index below64")),_=>Err(format!("{key} must be a catalog index"))}}
pub(super) fn validate(p:&Json)->Result<()>{
 let a=get(p,"args");let Obj(fields)=a else{return Err("Delegate arguments must be an object".into());};if fields.keys().any(|k|!["task","agent","role","specialty","model","connection"].contains(&k.as_str())){return Err("Delegate accepts task and an exact agent, model or connection selector, or a declared role/specialty; unknown fields are not ignored".into());}
 let task=get(a,"task").text()?;if task.trim().is_empty()||task.len()>32768{return Err("Delegated task must contain1–32768 bytes".into());}
 let agent=index_arg(a,"agent")?;let connection=index_arg(a,"connection")?;
 if get(a,"model")!=&Null{let model=get(a,"model").text()?;if model.is_empty()||model.len()>512{return Err("Model selector must be the exact nonempty configured model ID or connection name, at most512bytes; prefer agent:index from the catalog".into());}}
 let selectors=usize::from(agent.is_some())+usize::from(connection.is_some())+usize::from(get(a,"model")!=&Null);if selectors>1{return Err("Choose one delegate selector: agent, model or connection".into());}
 if get(a,"role")!=&Null&&!matches!(get(a,"role"),Str(v)if ["any","coding","nonwrite","orchestrator"].contains(&v.as_str())){return Err("Role must be any/coding/nonwrite/orchestrator from the configured catalog".into());}
 if get(a,"specialty")!=&Null&&!matches!(get(a,"specialty"),Str(v)if v.len()<=128){return Err("Specialty must be a configured label of at most128bytes".into());}
 if agent.is_some()&&(get(a,"role")!=&Null||get(a,"specialty")!=&Null){return Err("An exact agent index already selects its configured role/specialty; do not override them".into());}Ok(())
}
fn parent(st:&Json)->Json{if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()}}
pub(super) fn target(st:&Json,p:&Json)->Result<Json>{
 validate(p)?;let a=get(p,"args");let parent=parent(st);let permitted=array(&parent,"subagents");let agents=array(st,"agents");
 if let Some(index)=index_arg(a,"agent")?{if !permitted.contains(&Num(index.to_string())){return Err("This profile does not permit that agent index; use its current callable-agent catalog".into());}let child=agents.get(index).cloned().ok_or("Configured child agent no longer exists")?;prospective(st,p,child.clone())?;return Ok(child);}
 let role=text(a,"role");let wanted=if role.is_empty(){"any"}else{&role};let specialty=text(a,"specialty");let model=text(a,"model");let connection=index_arg(a,"connection")?;let exact=!model.is_empty()||connection.is_some();let mut candidates=vec![];let mut seen=std::collections::BTreeSet::new();
 for i in permitted{let Num(i)=i else{continue};let Ok(i)=i.parse::<usize>()else{continue};if !seen.insert(i){continue;}let Some(agent)=agents.get(i)else{continue};let role=text(agent,"role");let declared=if role.is_empty(){"any"}else{&role};if wanted!="any"&&declared!=wanted{continue;}if !specialty.is_empty()&&!text(agent,"specialty").eq_ignore_ascii_case(&specialty){continue;}
  if exact{let Ok((_,index))=prospective(st,p,agent.clone())else{continue};let Some(profile)=array(st,"profiles").get(index).cloned()else{continue};if connection.is_some_and(|c|c!=index)||(!model.is_empty()&&model!=text(&profile,"model")&&model!=text(&profile,"name")){continue;}}
  if !exact&&prospective(st,p,agent.clone()).is_err(){continue;}candidates.push(agent.clone());if !exact{break;}
 }
 if exact&&candidates.len()>1{return Err("Multiple permitted child profiles match that model/connection; choose the exact agent index from the catalog".into());}
 candidates.into_iter().next().ok_or("No permitted child profile matches. Configured model connections and callable child profiles are distinct; use the exact current catalog. No child was started".into())
}
pub(super) fn slots_available(st:&Json)->bool{array(st,"delegation_stack").len()<3&&harness::has_operation_room(st)}
fn prospective(st:&Json,p:&Json,agent:Json)->Result<(Json,usize)>{
 let mut stack=array(st,"delegation_stack");if !slots_available(st){return Err("Delegation reached its depth or operation limit".into());}
 let model=text(&agent,"model");let fallback=if model.is_empty()||model=="primary"{harness::route(st,"subagent")?}else{model.parse::<usize>().map_err(|_|"Invalid subagent model")?};
 if fallback>=array(st,"profiles").len(){return Err("Subagent connection not found".into());}
 stack.push(obj(vec![("run",get(st,"task_run").clone()),("connection",get(st,"request_index").clone()),("inquiry_id",get(p,"delegate_question").clone()),("harness_role",s(&harness_rules::execution_role(st)))]));
 let mut next=st.clone();put(&mut next,"delegation_stack",Arr(stack));put(&mut next,"task_run",obj(vec![("mode",s("delegate")),("phase",s("delegate")),("task",s(&text(get(p,"args"),"task"))),("agent",agent),("chat",get(st,"active").clone()),("connection",Num(fallback.to_string()))]));
 let connection=harness_rules::model_route(&next,fallback)?;Ok((next,connection))
}
pub(super) fn connection(st:&Json,p:&Json)->Result<usize>{let agent=target(st,p)?;Ok(prospective(st,p,agent)?.1)}
fn parent_guard(st:&Json)->String{harness_rules::digest(&obj(vec![("chat",get(st,"active").clone()),("root",get(st,"approval_root_identity").clone()),("parent",parent(st)),("task",get(st,"task_run").clone()),("policy",harness_rules::policy_binding(st)),("profile",array(st,"profiles").get(number(st,"request_index")).cloned().unwrap_or(Null))]))}
fn review_guard(st:&Json,p:&Json,child:&Json,index:usize)->String{harness_rules::digest(&obj(vec![("parent",s(&parent_guard(st))),("args",get(p,"args").clone()),("question",get(p,"delegate_question").clone()),("child",child.clone()),("connection",Num(index.to_string())),("profile",array(st,"profiles").get(index).cloned().unwrap_or(Null))]))}
pub(super) fn unreviewed(p:&mut Json){if let Obj(fields)=p{for key in ["delegate_parent_guard","delegate_review_guard","delegate_question","agent_name","child_connection","child_connection_name","child_model","child_role"]{fields.remove(key);}}}
pub(super) fn prepare(st:&Json,p:&mut Json)->Result<()>{let child=target(st,p)?;let(_,index)=prospective(st,p,child.clone())?;let profile=array(st,"profiles").get(index).cloned().ok_or("Child connection no longer exists")?;put(p,"agent_name",get(&child,"name").clone());put(p,"child_connection",Num(index.to_string()));put(p,"child_connection_name",get(&profile,"name").clone());put(p,"child_model",get(&profile,"model").clone());put(p,"child_role",s(if text(&child,"role").is_empty(){"any"}else{get(&child,"role").text().unwrap_or("any")}));if text(p,"delegate_parent_guard")!=parent_guard(st){put(p,"delegate_question",s(&inquiry::active(st)));}put(p,"delegate_parent_guard",s(&parent_guard(st)));put(p,"delegate_review_guard",s(&review_guard(st,p,&child,index)));Ok(())}
fn eligibility(st:&Json,parent:&Json,index:usize,agent:&Json)->Result<usize>{
 if !text(parent,"tools").split(',').map(str::trim).any(|t|t=="delegate"){return Err("delegate is disabled in the current parent profile".into());}
 if !harness::can_request_operations(st)||!harness::phase_permits(st,"delegate"){return Err("current phase or operation allowance does not permit delegation".into());}
 let p=obj(vec![("tool",s("delegate")),("args",obj(vec![("agent",Num(index.to_string())),("task",s("Current configured child route"))]))]);harness_rules::check(st,"delegate",get(&p,"args"),true)?;Ok(prospective(st,&p,agent.clone())?.1)
}
pub(super) fn has_callable(st:&Json,parent:&Json)->bool{let agents=array(st,"agents");array(parent,"subagents").iter().any(|id|matches!(id,Num(v)if v.parse::<usize>().ok().and_then(|i|agents.get(i).map(|a|eligibility(st,parent,i,a).is_ok())).unwrap_or(false)))}
fn public_label(value:&Json,key:&str)->Json{let v=text(value,key);let cap=512;obj(vec![("text",s(&v[..v.floor_char_boundary(v.len().min(cap))])),("complete",Bool(v.len()<=cap)),("bytes",Num(v.len().to_string()))])}
pub(super) fn catalog(st:&Json,parent:&Json)->String{
 let profiles=array(st,"profiles");let connections=profiles.iter().enumerate().take(32).map(|(i,p)|obj(vec![("connection",Num(i.to_string())),("name",public_label(p,"name")),("model",public_label(p,"model")),("protocol",public_label(p,"protocol"))])).collect();
 let agents=array(st,"agents");let mut children=vec![];let mut default=Null;let mut seen=std::collections::BTreeSet::new();
 for id in array(parent,"subagents").iter().take(64){let Num(raw)=id else{continue};let Ok(index)=raw.parse::<usize>()else{continue};if !seen.insert(index){continue;}let Some(agent)=agents.get(index)else{children.push(obj(vec![("agent",Num(index.to_string())),("callable",Bool(false)),("reason",s("configured child profile no longer exists"))]));continue};
  let route=eligibility(st,parent,index,agent);let mut row=obj(vec![("agent",Num(index.to_string())),("name",public_label(agent,"name")),("role",s(if text(agent,"role").is_empty(){"any"}else{get(agent,"role").text().unwrap_or("any")})),("specialty",public_label(agent,"specialty")),("configured_model_route",public_label(agent,"model")),("tools",public_label(agent,"tools")),("callable",Bool(route.is_ok()))]);
  match route{Ok(connection)=>{if default==Null{default=Num(index.to_string());}put(&mut row,"connection",Num(connection.to_string()));if let Some(p)=profiles.get(connection){put(&mut row,"model",public_label(p,"model"));put(&mut row,"connection_name",public_label(p,"name"));put(&mut row,"protocol",public_label(p,"protocol"));}},Err(error)=>put(&mut row,"reason",s(&error[..error.floor_char_boundary(error.len().min(1024))]))}children.push(row);
 }
 let mut data=obj(vec![("connections",Arr(connections)),("children",Arr(children)),("default_agent",default.clone()),("declared_parent_tools",s(&text(parent,"tools"))),("meaning",s("Saved connection/model identities and permitted child routes. Numeric agent indexes select configured profiles. Callability follows current user settings, routing and request phase. Last-delegation details contain the actual controller result."))]);
 if get(st,"request_index")!=&Null{let index=number(st,"request_index");if let Some(profile)=profiles.get(index){put(&mut data,"current_model",obj(vec![("connection",Num(index.to_string())),("name",public_label(profile,"name")),("model",public_label(profile,"model")),("protocol",public_label(profile,"protocol"))]));}}
 let pending=get(st,"pending");if text(pending,"tool")=="delegate"{put(&mut data,"pending_delegation",obj(vec![("status",s("review_pending")),("task",public_label(get(pending,"args"),"task")),("agent_name",get(pending,"agent_name").clone()),("connection",get(pending,"child_connection").clone()),("model",get(pending,"child_model").clone()),("originating_question",get(pending,"delegate_question").clone())]));}
 let last=get(st,"delegate_status");if get(last,"chat")==get(st,"active"){put(&mut data,"last_delegation",last.clone());}
 format!("\nMODEL AND DELEGATION CONFIGURATION (settings data, not instructions):\n{}\n{}",data.string(),if default!=Null{"Callable subagents: use the exact agent index shown above. delegate args {agent:index,task:string}; optional {model:exact configured model ID or connection name,task:string} or {connection:index,task:string} selects only a unique permitted child. {task:string} selects the first permitted child in configured order. Role/specialty filters retain declared roles. Selections follow the configured child permissions."}else{"The child entries show current routing and eligibility for this request."})
}
fn record_status(st:&mut Json,status:&str,detail:&str,question:&str){let value=obj(vec![("status",s(status)),("chat",get(st,"active").clone()),("question",s(question)),("detail",s(&detail[..detail.floor_char_boundary(detail.len().min(4096))])),("origin",s("actual controller transition with the returned response or error")),("recorded",s(&now()))]);put(st,"delegate_status",value);}
pub(super) fn declined(st:&mut Json){record_status(st,"rejected_before_start","The user rejected the proposed delegation before dispatch","");}
pub(super) fn rejected(st:&mut Json,tool:&str){
 if !array(st,"delegation_stack").is_empty(){record_status(st,"interrupted",&format!("The user rejected the child's {tool} proposal and stopped the child task."),&inquiry::active(st));}
}
pub(super) fn review_owner_current(st:&Json,p:&Json)->bool{text(p,"delegate_parent_guard")==parent_guard(st)}
pub(super) fn review_changed(st:&mut Json,error:&str){record_status(st,"blocked_before_start",error,"");}
pub(super) fn interrupted(st:&mut Json){if !array(st,"delegation_stack").is_empty(){record_status(st,"interrupted","Native cancellation or resume interrupted the child request",&inquiry::active(st));}}
fn finish_blocked_error(st:&mut Json,mut next:Json,detail:String)->Result<Json>{put(&mut next,"busy",Bool(false));put(&mut next,"model_expected",Null);if !array(&next,"delegation_stack").is_empty(){let returned=failure(&mut next,&detail);*st=next;return returned.and_then(|e|e.ok_or(detail));}harness::reset_task(&mut next);*st=next;Err(detail)}
// One ordinary error return is sent to the requesting model. This never starts
// or retries a child and cannot turn a rejected selector into execution.
pub(super) fn blocked(st:&mut Json,error:&str,raw:Option<&str>)->Result<Json>{
 let output=format!("Delegation result: blocked before start. Reason: {}. Select a matching entry from the current callable-agent catalog and use the ordinary review flow.",&error[..error.floor_char_boundary(error.len().min(2048))]);
 let used=boolean(st,"delegate_feedback_used");let mut next=st.clone();record_status(&mut next,"blocked_before_start",&output,"");put(&mut next,"pending",Null);put(&mut next,"agent_pending",Bool(false));put(&mut next,"dialog",Null);put(&mut next,"active_tool",Null);
 if get(&next,"task_run")!=&Null{if let Some(raw)=raw{let mut run=get(&next,"task_run").clone();let mut trail=array(&run,"tool_trail");let mut message=obj(vec![("role",s("assistant")),("content",s(raw))]);hosted_decorate(&next,&mut message);trail.push(message);put(&mut run,"tool_trail",Arr(trail));put(&mut next,"task_run",run);}}
 if used{if get(&next,"task_run")==&Null{append(&mut next,"user",&output)?;}return finish_blocked_error(st,next,format!("{error}. Delegation feedback was already returned once for this instruction"));}
 put(&mut next,"delegate_feedback_used",Bool(true));let index=number(st,"request_index");
 let prepared=if get(&next,"task_run")!=&Null{harness::task_tool_result_at(&mut next,&output,Some(index)).and_then(|e|e.ok_or("The requesting task is no longer active".into()))}else{append(&mut next,"user",&output)?;model_effect_at(&mut next,index)};
 let effect=match prepared{Ok(effect)=>effect,Err(reason)=>{return finish_blocked_error(st,next,format!("{error}. Rejection recorded; model continuation preparation failed: {reason}"));}};
 put(&mut next,"notice",s("Delegation did not start; returning the exact rejection to the requesting model"));*st=next;Ok(effect)
}
pub(super) fn start(st:&mut Json,p:&Json)->Result<Json>{
 harness::allowed_tool(st,p)?;let agent=target(st,p)?;let images=harness::parent_request_images(st)?;let(mut next,connection)=prospective(st,p,agent)?;
 if text(p,"delegate_review_guard")!=review_guard(st,p,get(get(&next,"task_run"),"agent"),connection){return Err("Delegation owner, child or model settings changed after review; no child was started. Review the current destination again".into());}
 put(&mut next,"steps",Num((number(st,"steps")+1).to_string()));let mut run=get(&next,"task_run").clone();put(&mut run,"attachments",Arr(images));put(&mut run,"connection",Num(connection.to_string()));put(&mut run,"observations",Arr(vec![]));put(&mut next,"task_run",run);
 put(&mut next,"pending",Null);put(&mut next,"dialog",Null);put(&mut next,"agent_pending",Bool(false));put(&mut next,"agent_running",Bool(false));record_status(&mut next,"started",&format!("Actual child request prepared on configured connection{connection}; waiting for its owned response"),"");let effect=model_effect_at(&mut next,connection)?;*st=next;Ok(effect)
}
pub(super) fn complete(st:&mut Json,reply:&str)->Result<Option<Json>>{settle(st,reply,"response_received")}
fn settle(st:&mut Json,reply:&str,status:&str)->Result<Option<Json>>{
 let mut stack=array(st,"delegation_stack");let Some(frame)=stack.pop()else{return Ok(None)};let child=text(get(get(st,"task_run"),"agent"),"name");let mut next=st.clone();put(&mut next,"delegation_stack",Arr(stack));put(&mut next,"task_run",get(&frame,"run").clone());put(&mut next,"agent_running",Bool(true));let output=format!("Subagent {child} result:\n{reply}");
 if get(&next,"task_run")==&Null{inquiry::restore_parent(&mut next,&text(&frame,"inquiry_id"));}
 record_status(&mut next,status,reply,&inquiry::active(st));let prepared=if get(&next,"task_run")!=&Null{harness::task_tool_result_at(&mut next,&output,Some(number(&frame,"connection")))}else{append(&mut next,"user",&output)?;model_effect_at(&mut next,number(&frame,"connection")).map(Some)};
 match prepared{Ok(effect)=>{*st=next;Ok(effect)},Err(error)=>{put(&mut next,"busy",Bool(false));put(&mut next,"model_expected",Null);record_status(&mut next,&format!("{status}_parent_unavailable"),&format!("Child return retained; parent continuation failed: {error}"),&inquiry::active(st));*st=next;Err(format!("Child return retained in its captured parent context; parent continuation failed: {error}"))}}
}
/// Return an actual child failure once; never replay its task or claim a result.
pub(super) fn failure(st:&mut Json,error:&str)->Result<Option<Json>>{
 if array(st,"delegation_stack").is_empty()||text(get(st,"task_run"),"phase")!="delegate"{return Ok(None);}
 let mut next=st.clone();
 put(&mut next,"pending",Null);put(&mut next,"dialog",Null);put(&mut next,"agent_pending",Bool(false));put(&mut next,"active_tool",Null);put(&mut next,"hosted_request",Null);put(&mut next,"stream_text",Null);
 let mut reason=error.to_string();
 for _ in 0..3{let detail:String=reason.chars().take(4096).collect();match settle(&mut next,&format!("Child request result: failed. Reason: {detail}. Use this actual result when continuing the parent task."),"failed"){Ok(effect)=>{*st=next;return Ok(effect);},Err(error)=>{reason=error;if array(&next,"delegation_stack").is_empty(){*st=next;return Err(reason);}}}}
 *st=next;Err(reason)
}
pub(super) fn status(st:&Json)->String{
 let stack=array(st,"delegation_stack");
 if let Some(frame)=stack.last(){return format!("Subagent {} is active (depth {}). Parent connection {} resumes after the actual child result or failure. Actual child results and failures resume the captured parent.",text(get(get(st,"task_run"),"agent"),"name"),stack.len(),number(frame,"connection")+1);}
 if text(get(st,"task_run"),"phase")=="delegate"{return format!("Standalone agent task {} is active on connection {}. Its result is saved in this conversation. Reviewed child delegation uses its own captured parent return.",text(get(get(st,"task_run"),"agent"),"name"),number(st,"request_index")+1);}
 let parent=harness::active_agent(st);let count=array(&parent,"subagents").len();
 let status=format!("Run agent task uses the selected profile and configured task model route; its answer is saved in this conversation. The selected agent has {count} permitted child links. A real child starts through a reviewed delegate operation and returns to its captured parent. Choose each child's model in Agent profiles and enable its Callable subagent checkbox on the parent. The controller returns the actual child response or error to its captured parent.");
 let last=get(st,"delegate_status");
 if get(last,"chat")==get(st,"active"){format!("{status}\nLast child transition: {} — {}",text(last,"status"),text(last,"detail"))}else{status}
}
fn callback_context(st:&Json)->String{
 harness_rules::digest(&obj(vec![("chat",get(st,"active").clone()),("task",get(st,"task_run").clone()),("stack",get(st,"delegation_stack").clone()),("tool",get(st,"active_tool").clone())]))
}
/// Existing operation owners remain authoritative. Only child network tools
/// without one of those owners receive this additional correlation token.
pub(super) fn bind_callback(st:&mut Json,effect:&mut Json)->Result<()>{
 let purpose=text(effect,"purpose");
 if array(st,"delegation_stack").is_empty()||text(effect,"kind")!="network"||purpose=="model"||purpose=="web_tool"||purpose.starts_with("mcp_client_")||model_metadata::is_result(effect)||remote_monitor::is_result(effect)||project_store::is_result(effect){return Ok(());}
 static SERIAL:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
 let serial=SERIAL.try_update(std::sync::atomic::Ordering::Relaxed,std::sync::atomic::Ordering::Relaxed,|n|n.checked_add(1)).map_err(|_|"Child callback identity exhausted")?;
 let ticket=format!("child-{}-{serial}",now());
 put(st,"child_network",obj(vec![("ticket",s(&ticket)),("purpose",s(&purpose)),("context",s(&callback_context(st)))]));
 put(effect,"child_ticket",s(&ticket));Ok(())
}
pub(super) fn callback_owned(st:&Json,action:&str,inputs:&Json,local:bool)->bool{
 if array(st,"delegation_stack").is_empty()||!boolean(st,"busy"){return false;}
 if local{return true;}
 let purpose=text(inputs,"purpose");
 if matches!(action,"network_result"|"failure"){
  if purpose=="model"{let ticket=text(st,"model_expected");return !ticket.is_empty()&&ticket==text(inputs,"model_ticket");}
  if purpose=="web_tool"{return web_operation::expected(st,inputs);}
  if purpose.starts_with("mcp_client_"){return harness::mcp_client_expected(st,inputs);}
  let expected=get(st,"child_network");let ticket=text(expected,"ticket");
  if !ticket.is_empty()&&ticket==text(inputs,"child_ticket")&&text(expected,"purpose")==purpose&&text(expected,"context")==callback_context(st){return true;}
 }
 if action=="tool_result"&&purpose=="web_tool"{return web_operation::expected(st,inputs);}
 if action=="phone_revision_result"||(action=="failure"&&purpose=="phone_revision"){
  let revision=get(st,"phone_revision");let ticket=text(revision,"ticket");
  return text(st,"active_tool")=="phone_update"&&!ticket.is_empty()&&ticket==text(inputs,"ticket")&&text(revision,"phase")==text(inputs,"phase");
 }
 false
}
