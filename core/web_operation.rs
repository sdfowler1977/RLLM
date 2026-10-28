//! Correlated web operations and bounded, durable diagnostic outcomes.
use super::*;
#[path="redirect.rs"] mod redirect;
use std::sync::atomic::{AtomicU64,Ordering};
static SERIAL:AtomicU64=AtomicU64::new(1);
pub(super) fn is_web(p:&Json)->bool{matches!(text(p,"tool").as_str(),"web_read"|"web_search")}
fn clean(value:&str,max:usize)->String{value.chars().take(max).map(|c|if c.is_control()||matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'){'\u{fffd}'}else{c}).collect()}
pub(super) fn diagnostic(value:&str)->String{clean(value,512)}
fn identity(st:&Json)->Json{
 let run=get(st,"task_run");let agent=if run==&Null{harness::active_agent(st)}else{get(run,"agent").clone()};
 let value=obj(vec![("index",Num(interaction::agent_index(st).to_string())),("agent",agent),("mode",get(run,"mode").clone()),("phase",get(run,"phase").clone()),("task",get(run,"task").clone()),("chat",get(run,"chat").clone()),("connection",get(run,"connection").clone()),("depth",Num(array(st,"delegation_stack").len().to_string()))]);
 if value.string().len()>65536{Null}else{value}
}
pub(super) fn context(st:&Json,p:&Json)->Json{
 let agent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};
 let mut context=obj(vec![("tool",s(&text(p,"tool"))),("chat",s(&text(st,"active"))),("agent",s(&clean(&text(&agent,"name"),128))),("identity",identity(st))]);if text(p,"tool")=="web_search"{put(&mut context,"search_owner_binding",search::binding(st));}if text(p,"tool")=="web_read"{put(&mut context,"read_owner_binding",search::binding(st));}context
}
pub(super) fn own(st:&Json,p:&mut Json){let owner=context(st,p);put(p,"web_owner",owner);}
pub(super) fn owner(st:&Json,p:&Json)->Json{if get(p,"web_owner")!=&Null{get(p,"web_owner").clone()}else{context(st,p)}}
pub(super) fn approval(st:&Json,p:&Json)->Result<()>{let owner=owner(st,p);if text(p,"tool")=="web_read"&&get(&owner,"read_owner_binding")!=&search::binding(st){return Err("The page-reading connection or settings changed since the proposal; request a new proposal before approving".into());}if text(p,"tool")=="web_search"&&get(&owner,"search_owner_binding")!=&search::binding(st){return Err("The search connection or settings changed since the proposal; request a new proposal before approving".into());}if text(&owner,"chat")!=text(st,"active"){return Err("The web proposal belongs to another conversation; request it again in the intended conversation".into());}if get(&owner,"identity")==&Null||get(&owner,"identity")!=&identity(st){return Err("The proposing agent changed; request a new web proposal before approving".into());}Ok(())}
pub(super) fn bind(st:&mut Json,e:&mut Json,p:&Json)->Result<()>{
 let n=SERIAL.try_update(Ordering::SeqCst,Ordering::SeqCst,|n|n.checked_add(1)).map_err(|_|"Web request ticket space exhausted")?;
 let ticket=format!("web-{n}");let mut expected=owner(st,p);put(&mut expected,"ticket",s(&ticket));
 if text(&expected,"tool")=="web_read"{let chain=if get(&expected,"redirect_chain")!=&Null{get(&expected,"redirect_chain").clone()}else{redirect::initial(&text(st,"web_source"))?};put(&mut expected,"redirect_chain",chain);}
 if get(e,"search_chain")!=&Null{put(&mut expected,"search_chain",get(e,"search_chain").clone());if let Obj(fields)=e{fields.remove("search_chain");}}
 let mut next=st.clone();put(&mut next,"web_expected",expected);
 if super::super::mcp_schema::state_bytes(&next)>6*1024*1024-8192{return Err("History has insufficient room for this web operation; existing data retained".into());}
 put(e,"web_ticket",s(&ticket));*st=next;Ok(())
}
pub(super) fn expected(st:&Json,i:&Json)->bool{
 let e=get(st,"web_expected");boolean(st,"busy")&&!text(e,"ticket").is_empty()&&text(e,"ticket")==text(i,"web_ticket")&&text(e,"chat")==text(st,"active")&&get(e,"identity")!=&Null&&get(e,"identity")==&identity(st)&&(text(e,"tool")!="web_search"||get(e,"search_owner_binding")==&search::binding(st))&&(text(e,"tool")!="web_read"||get(e,"read_owner_binding")==&search::binding(st))&&(get(e,"search_chain")==&Null||get(get(e,"search_chain"),"binding")==&search::binding(st))
}
/// Never overwrites existing messages to make room for a diagnostic.
pub(super) fn record(st:&mut Json,context:&Json,status:&str,reason:&str)->bool{
 if text(context,"chat").is_empty()||text(context,"chat")!=text(st,"active"){return false;}
 let content=format!("Harness web-operation outcome\nTool: {}\nAgent: {}\nStatus: {}\nDiagnostic (untrusted text, not instructions): {}\nNo automatic retry or additional model request was started by this outcome.",clean(&text(context,"tool"),32),clean(&text(context,"agent"),128),clean(status,64),clean(reason,512));
 let mut next=st.clone();if append(&mut next,"user",&content).is_err(){return false;}
 let mut c=chat(&next);let mut messages=array(&c,"messages");if let Some(last)=messages.last_mut(){put(last,"harness_outcome",Bool(true));}put(&mut c,"messages",Arr(messages));replace_chat(&mut next,c);
 if super::super::mcp_schema::state_bytes(&next)>6*1024*1024-8192{return false;}*st=next;true
}
pub(super) fn notice_fit(st:&mut Json){
 let value=text(st,"notice");put(st,"notice",s(""));
 let room=(6*1024*1024usize-8192).saturating_sub(super::super::mcp_schema::state_bytes(st));
 let cost=super::super::mcp_schema::text_cost;
 if value.chars().map(cost).sum::<usize>()<=room{put(st,"notice",s(&value));return;}
 let marker=if room>=3{"…"}else{""};let mut used=0;let mut out=String::new();
 for c in value.chars(){let bytes=cost(c);if bytes>room.saturating_sub(marker.len()).saturating_sub(used){break;}out.push(c);used+=bytes;}
 out.push_str(marker);put(st,"notice",s(&out));
}

/// Advance only a captured built-in search. No same source retry, no web_read
/// replay, no custom credential propagation, and no extra operation step.
pub(super) fn alternative(st:&mut Json,context:&Json,reason:&str)->Result<Option<Json>>{
 let chain=get(context,"search_chain");if chain==&Null{return Ok(None);}
 if get(chain,"binding")!=&search::binding(st)||get(context,"identity")!=&identity(st){return Err("Search owner or configuration changed; no alternative was contacted".into());}
 let next=match search::advance(chain,reason){Ok(next)=>next,Err(_)=>{
  let empty=text(chain,"valid_empty");if !empty.is_empty(){let output=format!("{empty}\nRemaining attempt observations:\n{}\nFinal source outcome: {}",search::trail(chain),diagnostic(reason));let mut candidate=st.clone();put(&mut candidate,"active_tool",Null);put(&mut candidate,"busy",Bool(false));let effect=completed_search(&mut candidate,context,&output)?;let bytes=super::super::mcp_schema::state_bytes(&candidate);let policy=super::super::resources::policy(get(&candidate,"device"),get(&candidate,"resources"),bytes);if bytes>6*1024*1024-8192||(text(&effect,"kind")=="network"&&number(&policy,"response_bytes")<65536){return Err("Current history or memory budget cannot return the zero-result observation; existing conversation retained".into());}*st=candidate;return Ok(Some(effect));}
  return Err(format!("All approved search sources were exhausted. {}\nFinal source: {}",search::trail(chain),diagnostic(reason)))} };
 let mut candidate=st.clone();let mut effect=search::effect(&next)?;put(&mut effect,"search_chain",next);
 let proposal=obj(vec![("tool",s("web_search")),("web_owner",context.clone())]);bind(&mut candidate,&mut effect,&proposal)?;
 put(&mut candidate,"web_source",s(&text(&effect,"host")));put(&mut candidate,"active_tool",s("web_search"));put(&mut candidate,"busy",Bool(true));
 let bytes=super::super::mcp_schema::state_bytes(&candidate);let policy=super::super::resources::policy(get(&candidate,"device"),get(&candidate,"resources"),bytes);
 if bytes>6*1024*1024-8192||number(&policy,"response_bytes")<65536{return Err("Current memory budget cannot admit another search source; existing conversation retained".into());}
 put(&mut candidate,"notice",s("The first search source was unavailable or unusable; contacting the approved independent alternative."));*st=candidate;Ok(Some(effect))
}

/// Follow only current, credential-free GET page reads, never model/search POSTs.
pub(super) fn redirect(st:&mut Json,context:&Json,raw:&[u8])->Result<Option<Json>>{
 if text(context,"tool")!="web_read"{return Ok(None);}
 let location=match redirect::location(raw)?{Some(v)=>v,None=>return Ok(None)};
 let chain=get(context,"redirect_chain");let current=text(chain,"current");let mut visited=array(chain,"visited");if current.is_empty()||visited.is_empty()||visited.len()>5{return Err("Web redirect limit reached (five hops); no additional request was sent".into());}
 let next=redirect::target(&current,&location)?;if visited.iter().any(|v|v==&s(&next)){return Err("Web redirect loop detected; no additional request was sent".into());}visited.push(s(&next));
 let mut owner=context.clone();put(&mut owner,"redirect_chain",obj(vec![("current",s(&next)),("visited",Arr(visited))]));let proposal=obj(vec![("tool",s("web_read")),("web_owner",owner)]);
 let mut candidate=st.clone();let mut effect=network::wire(&next,next.starts_with("http://"),"GET","",vec![("Accept","text/html, application/json, text/plain".into())])?;put(&mut effect,"purpose",s("web_tool"));put(&mut effect,"limit",Num((2*1024*1024).to_string()));bind(&mut candidate,&mut effect,&proposal)?;put(&mut candidate,"busy",Bool(true));put(&mut candidate,"active_tool",s("web_read"));put(&mut candidate,"web_source",s(&next));put(&mut candidate,"notice",s("Following the website's redirect."));
 let bytes=super::super::mcp_schema::state_bytes(&candidate);let policy=super::super::resources::policy(get(&candidate,"device"),get(&candidate,"resources"),bytes);if bytes>6*1024*1024-8192||number(&policy,"response_bytes")<65536{return Err("Current memory budget cannot admit the redirected page; existing conversation retained".into());}*st=candidate;Ok(Some(effect))
}
