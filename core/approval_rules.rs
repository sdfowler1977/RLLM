//! User-created operation approvals. Session grants never enter saved state.
use super::*;
use std::sync::{Mutex,OnceLock};
const MAX_RULES:usize=64;
const MAX_BYTES:usize=128*1024;
static SESSION:OnceLock<Mutex<Vec<Json>>>=OnceLock::new();
static SERIAL:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
fn sessions()->&'static Mutex<Vec<Json>>{SESSION.get_or_init(||Mutex::new(vec![]))}
fn root(st:&Json)->String{text(st,"approval_root")}
pub(super) fn namespace(st:&mut Json,directory:&str){
 let canonical=Path::new(directory).canonicalize().ok().filter(|p|p.is_dir()).and_then(|p|p.to_str().map(str::to_owned)).unwrap_or_default();
 let identity=fs::metadata(&canonical).ok().map(|m|{use std::os::unix::fs::MetadataExt;format!("{}:{}",m.dev(),m.ino())}).unwrap_or_default();put(st,"approval_root_identity",s(&identity));
 put(st,"approval_root",s(&canonical));
}
fn binding(st:&Json)->Json{
 let run=get(st,"task_run");let agent=if run==&Null{harness::active_agent(st)}else{get(run,"agent").clone()};
 let child=if text(get(st,"pending"),"tool")=="delegate"{delegation::target(st,get(st,"pending")).unwrap_or(Null)}else{Null};
 let child_index=if child==Null{None}else{delegation::connection(st,get(st,"pending")).ok()};let child_profile=if child==Null{Null}else{child_index.and_then(|i|array(st,"profiles").get(i).cloned()).unwrap_or(Null)};
 obj(vec![("harness",harness_rules::binding(st)),("root",s(&root(st))),("root_identity",get(st,"approval_root_identity").clone()),("origin",s(if boolean(st,"agent_pending"){"agent"}else{"user"})),("agent",agent),("connection",search::binding(st)),("child",child),("child_profile",child_profile),("phone",get(st,"phone_uri").clone()),("phone_mode",get(st,"phone_access_mode").clone()),("drive",get(st,"drive").clone()),("rea",get(st,"rea").clone())])
}
fn identities(b:&Json)->String{let p=get(get(b,"connection"),"profile");let endpoint=text(p,"endpoint");let endpoint=origin(&endpoint).unwrap_or_else(|_|"unconfigured endpoint".into());let mut label=format!("Origin: {}\nAgent: {}\nConnection: {} · {} · {}",if text(b,"origin")=="user"{"your manual action"}else{"model proposal"},text(get(b,"agent"),"name"),text(p,"name"),text(p,"model"),endpoint);if get(b,"child")!=&Null{let p=get(b,"child_profile");label.push_str(&format!("\nSubagent: {} · {} · {}",text(get(b,"child"),"name"),text(p,"model"),origin(&text(p,"endpoint")).unwrap_or_else(|_|"unconfigured endpoint".into())));}label}
fn rows_fit(rows:&[Json])->Result<()>{if rows.len()>MAX_RULES||Arr(rows.to_vec()).string().len()>MAX_BYTES{Err("Approval rules are full (64 rules / 128 KiB); remove an old rule or approve once".into())}else{Ok(())}}
fn state_fit(st:&Json)->Result<()>{if super::super::mcp_schema::state_bytes(st)>6*1024*1024-8192{Err("Approval rule does not fit encrypted app state; approve once or remove old history/rules".into())}else{Ok(())}}
fn origin(value:&str)->Result<String>{let u=network::url(value,value.starts_with("http://"))?;Ok(format!("{}://{}:{}",if u.tls{"https"}else{"http"},u.host.to_ascii_lowercase(),u.port))}
fn path_tool(t:&str)->bool{["list","read","write","search","inspect","phone_list","phone_read","phone_create","phone_update","drive_list","drive_read","drive_create","drive_update","drive_folder"].contains(&t)}
fn args_shape(tool:&str,args:&Json)->Json{
 let mut shape=args.clone();if let Obj(m)=&mut shape{
  if tool=="web_search"{m.remove("query");}
  if tool=="web_read"{m.remove("url");}
  if path_tool(tool){m.remove("path");if ["write","phone_create","phone_update","drive_create","drive_update"].contains(&tool){m.remove("content");m.remove("approved_previous");m.remove("previous_exists");}if tool=="search"{m.remove("query");}}
 }shape
}
fn default_scope(tool:&str)->&'static str{if tool=="web_read"{"site"}else if tool=="web_search"{"search"}else if path_tool(tool){"path"}else{"exact"}}
fn scope(tool:&str,args:&Json,kind:&str)->Result<Json>{
 let target=match kind{
  "exact"=>args.clone(),
  "search" if tool=="web_search"=>s("Current configured search sources; future queries"),
  "site" if tool=="web_read"=>s(&origin(&text(args,"url"))?),
  "path" if path_tool(tool)=>s(&text(args,"path")),
  "folder" if path_tool(tool)=>{let p=text(args,"path");s(p.rsplit_once('/').map(|v|v.0).unwrap_or(""))},
  _=>return Err("Choose a supported approval scope".into())
 };
 Ok(obj(vec![("kind",s(kind)),("target",target),("shape",if kind=="exact"{Null}else{args_shape(tool,args)})]))
}
fn description(tool:&str,sc:&Json)->String{
 let target=text(sc,"target");match text(sc,"kind").as_str(){
 "search"=>format!("{tool}: future queries through the same configured search sources"),
 "site"=>format!("{tool}: this site ({target}); same read/capture settings and output"),
 "path"=>format!("{tool}: exact path {}{}",if target.is_empty(){"(root)"}else{&target},if ["write","phone_create","phone_update","drive_create","drive_update"].contains(&tool){"; future contents may differ"}else if tool=="search"{"; future search text may differ"}else{""}),
 "folder"=>format!("{tool}: folder {} and its descendants; same operation settings{}",if target.is_empty(){"(root)"}else{&target},if ["write","phone_create","phone_update","drive_create","drive_update"].contains(&tool){"; future contents may differ"}else{""}),
 _=>format!("{tool}: only identical arguments"),
 }}
fn choices(tool:&str)->Vec<(&'static str,&'static str)>{let mut v=vec![("Identical arguments","exact")];if tool=="web_read"{v.push(("Same site and read/capture settings","site"));}if tool=="web_search"{v.push(("Same sources, future search queries","search"));}if path_tool(tool){v.push(("Exact path","path"));v.push(("Parent folder and descendants","folder"));}v}
fn selected(st:&Json,p:&Json)->String{let chosen=if text(st,"form_dialog")=="approval"{text(get(st,"form_inputs"),"approval_scope")}else{String::new()};if chosen.is_empty(){default_scope(&text(p,"tool")).into()}else{chosen}}
fn operation_preview(p:&Json)->String{let mut out=p.string();if matches!(text(p,"tool").as_str(),"inspect"|"compare")||text(p,"tool")=="experiments"&&get(p,"experiment_operation")!=&Null{out.push_str("\nFor each selected folder (two for comparison): recursively list up to 500 entries/eight levels and sample up to 4096 header bytes per regular file within current memory/time limits. No source writes or symlink traversal. Successful report text and source/task provenance may be retained in encrypted app state; retention capacity does not replace the current result. Agent list/read permissions and child-specific harness rules still apply; omitted observations are reported. Results from model-origin work return to the selected model.");}out}
pub(super) fn approval(st:&Json)->Json{
 if harness_rules::review_required(st,get(st,"pending")){let reconstruction_target=reconstruction::review_target(st,get(st,"pending"));let target=if reconstruction_target!=Null{&reconstruction_target}else if text(get(st,"pending"),"tool")=="experiments"&&get(get(st,"pending"),"experiment_operation")!=&Null&&harness_rules::review_required(st,get(get(st,"pending"),"experiment_operation")){get(get(st,"pending"),"experiment_operation")}else{get(st,"pending")};let(d,r)=harness_rules::decision(st,&text(target,"tool"),get(target,"args"));let mut pending=get(st,"pending").clone();if let Obj(m)=&mut pending{m.remove("web_owner");m.remove("claim_author");m.remove("experiment_author");m.remove("experiment_definition");m.remove("reconstruction_source");}return modal("Review proposed operation",vec![field_ui("operation","Operation and arguments",&operation_preview(&pending),"readonly"),field_ui("rule","Higher harness rule",&format!("{}: {}\n{}",d,text(&r,"id"),text(&r,"text")),"readonly")],vec![button("Approve once","approve"),button("Reject","reject")]);}
 let mut p=get(st,"pending").clone();if let Obj(m)=&mut p{m.remove("web_owner");m.remove("claim_author");m.remove("experiment_author");m.remove("experiment_definition");m.remove("reconstruction_source");}
 let tool=text(&p,"tool");let kind=selected(st,&p);let scopes=choices(&tool).iter().filter_map(|(_,k)|scope(&tool,get(&p,"args"),k).ok()).map(|s|description(&tool,&s)).collect::<Vec<_>>().join("\n");
 modal("Review proposed operation",vec![field_ui("preview","Operation and arguments",&operation_preview(&p),"readonly"),interaction::choice("approval_scope","Session / always rule scope",&kind,&choices(&tool)),field_ui("rule_scope","Scope choices and identity",&format!("{}\n{}\nOnly this workspace and these captured identities/settings. A changed agent, credentials or storage/search configuration requires another approval. Agent tool permissions apply to model proposals; manual actions retain their current access controls.\nSession lasts until the app process closes, including pauses and activity rebuilds. Always is saved in encrypted app state.",scopes,identities(&binding(st))),"readonly")],vec![button("Approve once","approve"),button("Approve for this session","approve_session"),button("Always approve","approve_always"),button("Reject","reject")])
}
fn current(st:&Json)->Vec<Json>{let namespace=root(st);sessions().lock().ok().map(|r|r.iter().filter(|r|text(get(r,"binding"),"root")==namespace).cloned().collect()).unwrap_or_default()}
fn records(st:&Json)->Vec<Json>{let mut v=array(st,"approval_rules");v.extend(current(st));v}
fn editing(st:&Json)->Json{let id=text(st,"approval_edit");records(st).into_iter().find(|r|text(r,"id")==id).unwrap_or(Null)}
fn valid(r:&Json)->bool{number(r,"schema")==1&&!text(r,"id").is_empty()&&text(r,"id").len()<=128&&r.string().len()<=16384&&matches!(get(r,"binding"),Obj(_))&&matches!(get(r,"sample"),Obj(_))&&matches!(get(r,"scope"),Obj(_))&&["session","always"].contains(&text(r,"lifetime").as_str())}
pub(super) fn candidate(st:&Json,lifetime:&str)->Result<Json>{
 if text(st,"dialog")!="approval"||get(st,"pending")==&Null||boolean(st,"busy"){return Err("No current operation is awaiting approval".into());}
 if root(st).is_empty(){return Err("Open the current workspace before saving an approval rule".into());}
 let p=get(st,"pending");if harness_rules::review_required(st,p){return Err("A higher harness rule requires review once; no session/always grant saved".into());}validate_tool(p)?;if boolean(st,"agent_pending"){harness::allowed_tool(st,p)?;}
 let tool=text(p,"tool");let args=get(p,"args");let sc=scope(&tool,args,&selected(st,p))?;
 let serial=SERIAL.fetch_update(std::sync::atomic::Ordering::Relaxed,std::sync::atomic::Ordering::Relaxed,|n|n.checked_add(1)).map_err(|_|"Approval identity counter exhausted; approve once")?;
 let rule=obj(vec![("schema",Num("1".into())),("id",s(&format!("{}-{serial}",now()))),("lifetime",s(lifetime)),("tool",s(&tool)),("label",s(&description(&tool,&sc))),("binding",binding(st)),("scope",sc),("sample",args.clone())]);
 if !valid(&rule){return Err("Approval rule exceeds the 16 KiB identity/argument limit; approve once instead".into());}
 let mut all=records(st);all.push(rule.clone());rows_fit(&all)?;
 let mut admitted=st.clone();save_always(&mut admitted,rule.clone());state_fit(&admitted)?;
 if lifetime=="session"{let rows=sessions().lock().map_err(|_|"Session approvals are unavailable")?;if rows.len()>=MAX_RULES||rows.iter().map(|r|r.string().len()).sum::<usize>()+rule.string().len()>MAX_BYTES{return Err("Process session approvals are full; remove a rule or approve once".into());}}
 Ok(rule)
}
pub(super) fn save_always(st:&mut Json,r:Json){let mut rows=array(st,"approval_rules");rows.push(r);put(st,"approval_rules",Arr(rows));}
pub(super) fn save_session(r:Json)->Result<()>{let mut rows=sessions().lock().map_err(|_|"Session approvals are unavailable")?;if rows.len()>=MAX_RULES||rows.iter().map(|r|r.string().len()).sum::<usize>()+r.string().len()>MAX_BYTES{return Err("Process session approvals are full; remove a rule or approve once".into());}rows.push(r);Ok(())}
pub(super) fn matches(st:&Json)->bool{
 if harness_rules::review_required(st,get(st,"pending")){return false;}
 let p=get(st,"pending");if root(st).is_empty()||text(st,"dialog")!="approval"||validate_tool(p).is_err()||(boolean(st,"agent_pending")&&harness::allowed_tool(st,p).is_err()){return false;}
 let owner=binding(st);let tool=text(p,"tool");let args=get(p,"args");records(st).iter().any(|r|{
 if !valid(r)||get(r,"binding")!=&owner||text(r,"tool")!=tool{return false;}
 let saved=get(r,"scope");if text(saved,"kind")=="folder"{
  if !path_tool(&tool)||get(saved,"shape")!=&args_shape(&tool,args){return false;}let parent=text(saved,"target");let path=text(args,"path");return parent.is_empty()||path==parent||path.starts_with(&(parent+"/"));
 }
 scope(&tool,args,&text(saved,"kind")).map(|now|now==*saved).unwrap_or(false)
 })
}
pub(super) fn dialog(st:&Json)->Json{
 if text(st,"dialog")=="approval_rule"{let r=editing(st);let tool=text(&r,"tool");return modal("Approval rule",vec![field_ui("rule_label","Rule label",&text(&r,"label"),"text"),interaction::choice("rule_scope","Approved scope",&text(get(&r,"scope"),"kind"),&choices(&tool)),field_ui("rule_details","Current scope",&format!("{}\n{}\n{}\nOnly the captured workspace and settings. Saving a different scope is an explicit change to this rule.",description(&tool,get(&r,"scope")),if text(&r,"lifetime")=="session"{"This process session"}else{"Always (encrypted saved state)"},identities(get(&r,"binding"))),"readonly")],vec![button("Save rule changes","approval_rule_save"),button("Revoke rule","approval_rule_revoke"),button("Back to rules","approval_rules")]);}
 let all=records(st);let mut buttons=all.iter().filter(|r|valid(r)).map(|r|button(&format!("{} · {}",if text(r,"lifetime")=="session"{"Session"}else{"Always"},text(r,"label")),&format!("approval_rule:{}",text(r,"id")))).collect::<Vec<_>>();buttons.push(button("Clear session rules","approval_session_clear"));buttons.push(button("Clear saved rules","approval_always_clear"));
 modal("Approval rules",vec![field_ui("summary","User approvals",&format!("{} of 64 rules. Rules apply only to their captured workspace, agent, connection and configuration. Select a rule to edit its scope or revoke it. Session rules end when the app process closes; pauses and activity rebuilds keep them. Always rules are encrypted with app state. Agent permissions govern model proposals; manual actions retain their current access controls.",all.len()),"readonly")],buttons)
}
pub(super) fn handles(a:&str)->bool{a=="approval_rules"||a.starts_with("approval_rule:")||["approval_rule_save","approval_rule_revoke","approval_session_clear","approval_always_clear"].contains(&a)}
pub(super) fn event(st:&mut Json,a:&str,inputs:&Json)->Result<()>{
 if boolean(st,"busy")||get(st,"pending")!=&Null{return Err("Finish or reject the current operation before managing approval rules".into());}
 if a=="approval_rules"{put(st,"approval_edit",Null);put(st,"dialog",s("approval_rules"));return Ok(());}
 if let Some(id)=a.strip_prefix("approval_rule:"){records(st).into_iter().find(|r|text(r,"id")==id&&valid(r)).ok_or("Approval rule is no longer available")?;put(st,"approval_edit",s(id));put(st,"dialog",s("approval_rule"));return Ok(());}
 if a=="approval_session_clear"{let namespace=root(st);sessions().lock().map_err(|_|"Session approvals are unavailable")?.retain(|r|text(get(r,"binding"),"root")!=namespace);}
 else if a=="approval_always_clear"{put(st,"approval_rules",Arr(vec![]));}
 else {
  if text(st,"dialog")!="approval_rule"{return Err("Open the rule before changing it".into());}
  let mut r=editing(st);if !valid(&r){return Err("Approval rule is unavailable".into());}let id=text(&r,"id");
  if a=="approval_rule_save"{let label=text(inputs,"rule_label");if label.is_empty()||label.len()>512{return Err("Enter a rule label of at most 512 bytes".into());}let sc=scope(&text(&r,"tool"),get(&r,"sample"),&text(inputs,"rule_scope"))?;put(&mut r,"label",s(&label));put(&mut r,"scope",sc);if !valid(&r){return Err("Edited approval rule exceeds its size limit".into());}}
  if text(&r,"lifetime")=="session"{let mut guard=sessions().lock().map_err(|_|"Session approvals are unavailable")?;let mut rows=guard.clone();let i=rows.iter().position(|x|text(x,"id")==id&&get(x,"binding")==get(&r,"binding")).ok_or("Session rule is no longer available")?;if a=="approval_rule_save"{rows[i]=r;rows_fit(&rows)?;let mut combined=array(st,"approval_rules");combined.extend(rows.iter().filter(|r|text(get(r,"binding"),"root")==root(st)).cloned());rows_fit(&combined)?;state_fit(st)?;}else{rows.remove(i);}*guard=rows;}
  else {let mut rows=array(st,"approval_rules");let i=rows.iter().position(|x|text(x,"id")==id&&get(x,"binding")==get(&r,"binding")).ok_or("Saved rule is no longer available")?;if a=="approval_rule_save"{rows[i]=r;}else{rows.remove(i);}let mut next=st.clone();put(&mut next,"approval_rules",Arr(rows.clone()));if a=="approval_rule_save"{let mut combined=rows;combined.extend(current(st));rows_fit(&combined)?;state_fit(&next)?;}*st=next;}
 }
 put(st,"approval_edit",Null);put(st,"dialog",s("approval_rules"));put(st,"notice",s("Approval rules updated"));Ok(())
}
