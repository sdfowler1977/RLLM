//! Retained investigation reports. Historical data never becomes policy.
use super::*;
const KEY:&str="investigation_records";
const MAX_ROWS:usize=64;
const MAX_BYTES:usize=1024*1024;
const MAX_REPORT:usize=128*1024;
const PAGE:usize=12;
fn age(r:&Json)->String{
 let Some(seconds)=text(r,"recorded_at").parse::<u128>().ok().and_then(|t|now().parse::<u128>().ok()?.checked_sub(t)).map(|n|n/1_000_000_000)else{return "recorded time unavailable".into();};
 if seconds<60{"just now".into()}else if seconds<3600{format!("{} min ago",seconds/60)}else if seconds<86400{format!("{} h ago",seconds/3600)}else{format!("{} days ago",seconds/86400)}
}
fn access_binding(st:&Json,tool:&str,args:&Json)->Json{harness_rules::observation_binding(st,tool,args)}
fn rows(st:&Json)->Result<&Vec<Json>>{
    static EMPTY:std::sync::OnceLock<Vec<Json>>=std::sync::OnceLock::new();
    match get(st,KEY){Null=>Ok(EMPTY.get_or_init(Vec::new)),Obj(v)=>{
        if v.get("schema")!=Some(&Num("1".into())){return Err("Unsupported investigation record version; saved data retained".into());}
        match v.get("items"){Some(Arr(items)) if items.len()<=MAX_ROWS && super::super::mcp_schema::state_bytes(get(st,KEY))<=MAX_BYTES=>Ok(items),_=>Err("Malformed investigation records; saved data retained".into())}
    },_=>Err("Malformed investigation records; saved data retained".into())}
}
fn store_with_items(st:&Json,items:Vec<Json>)->Json{
 let mut store=get(st,KEY).clone();if store==Null{store=obj(vec![("schema",Num("1".into()))]);}
 put(&mut store,"items",Arr(items));store
}
fn bounded_string(v:&Json,key:&str,cap:usize)->bool{matches!(get(v,key),Str(t) if t.len()<=cap)}
fn valid(r:&Json)->bool{
 number(r,"schema")==1&&(["inspect","compare"].contains(&text(r,"tool").as_str())||web_evidence::is_web(&text(r,"tool")))&&
 ["id","recorded_at","workspace_identity","policy"].iter().all(|k|bounded_string(r,k,256))&&
 bounded_string(r,"workspace",4096)&&bounded_string(r,"chat",128)&&bounded_string(r,"agent",128)&&
 bounded_string(r,"task",32768)&&bounded_string(r,"mode",32)&&matches!(get(r,"depth"),Num(n) if n.parse::<usize>().ok().filter(|n|*n<=3).is_some())&&bounded_string(r,"report",MAX_REPORT)&&
 matches!(get(r,"args"),Obj(_))&&(if web_evidence::is_web(&text(r,"tool")){web_evidence::valid_record(r)}else{bounded_string(get(r,"args"),"path",1024)&&(text(r,"tool")!="compare"||bounded_string(get(r,"args"),"other",1024))})&&!text(r,"id").is_empty()
}
pub(super) fn descriptor(st:&Json,effect:&Json,ticket:&str)->Json{
 let tool=text(effect,"tool");if boolean(st,"investigation_retention_off")||!["inspect","compare"].contains(&tool.as_str()){return Null;}
 let args=get(effect,"args");let mut sources=obj(vec![("path",s(&text(args,"path")))]);if tool=="compare"{put(&mut sources,"other",s(&text(args,"other")));}
 if get(args,"content")!=&Null{put(&mut sources,"content",get(args,"content").clone());}
 let run=get(st,"task_run");let agent=if run==&Null{harness::active_agent(st)}else{get(run,"agent").clone()};
 obj(vec![("schema",Num("1".into())),("id",s(ticket)),("tool",s(&tool)),("args",sources.clone()),
 ("workspace",get(st,"approval_root").clone()),("workspace_identity",get(st,"approval_root_identity").clone()),
 ("policy",access_binding(st,&tool,&sources)),("chat",s(&text(st,"active"))),("agent",s(&text(&agent,"name"))),
 ("task",s(&text(run,"task"))),("mode",s(&text(run,"mode"))),("depth",Num(array(st,"delegation_stack").len().to_string()))])
}
/// Called only after the operation's authenticated successful callback settles.
/// Capacity/malformed data affects retention alone, never the returned evidence.
pub(super) fn retain(st:&mut Json,descriptor:&Json,report:&str,graphs:&Json,graph_note:&str){
 if descriptor==&Null{return;}
 let before_bytes=super::super::mcp_schema::state_bytes(st);
 if before_bytes>6*1024*1024-256*1024{return;}
 let before=super::super::resources::policy(get(st,"device"),get(st,"resources"),before_bytes);
 let mut next=st.clone();retain_staged(&mut next,descriptor,report,graphs,graph_note);
 let bytes=super::super::mcp_schema::state_bytes(&next);let after=super::super::resources::policy(get(&next,"device"),get(&next,"resources"),bytes);
 if bytes<=6*1024*1024-256*1024&&number(&after,"response_bytes")>=number(&before,"response_bytes"){*st=next;}
}
fn retain_staged(st:&mut Json,descriptor:&Json,report:&str,graphs:&Json,graph_note:&str){
 let result=(||->Result<()>{
   if report.len()>MAX_REPORT{return Err("Report exceeds the 128 KiB retention limit; current result remains available".into());}
   let mut record=descriptor.clone();put(&mut record,"recorded_at",s(&now()));put(&mut record,"report",s(report));
   if !valid(&record){return Err("Report provenance cannot be retained; current result remains available".into());}
   let old=rows(st)?;if old.iter().any(|r|text(r,"id")==text(&record,"id")){return Ok(());}
   if old.len()>=MAX_ROWS{return Err("Investigation storage is full (64 records); existing records and current result are retained".into());}
   let mut typed_note=graph_note.to_string();
   if graphs!=&Null&&observation_graph::validate_record(graphs,&record).is_err(){typed_note="Typed observation provenance was invalid; textual report retained".into();}
   else if graphs!=&Null{put(&mut record,"snapshots",graphs.clone());let mut trial=old.clone();trial.push(record.clone());let candidate=store_with_items(st,trial);let projected=super::super::mcp_schema::state_bytes(st).saturating_sub(super::super::mcp_schema::state_bytes(get(st,KEY))).saturating_add(super::super::mcp_schema::state_bytes(&candidate));if super::super::mcp_schema::state_bytes(&candidate)>MAX_BYTES||projected>6*1024*1024-256*1024{if let Obj(fields)=&mut record{fields.remove("snapshots");}typed_note="Typed snapshot could not fit retained storage; textual report retained".into();}}
   if !typed_note.is_empty(){put(&mut record,"graph_note",s(&typed_note));}
   let mut items=old.clone();items.push(record);let store=store_with_items(st,items);
   if super::super::mcp_schema::state_bytes(&store)>MAX_BYTES{return Err("Investigation storage is full (1 MiB); existing records and current result are retained".into());}
   let old_bytes=super::super::mcp_schema::state_bytes(get(st,KEY));let projected=super::super::mcp_schema::state_bytes(st).saturating_sub(old_bytes).saturating_add(super::super::mcp_schema::state_bytes(&store));
   if projected>6*1024*1024-256*1024{return Err("Encrypted state has insufficient room for another investigation record; current result remains available".into());}
   let policy=super::super::resources::policy(get(st,"device"),get(st,"resources"),projected);
   if boolean(st,"busy")&&number(&policy,"response_bytes")<65536{return Err("Current response memory cannot admit another retained report; current result remains available".into());}
   put(st,KEY,store);Ok(())
 })();
 put(st,"investigation_retention",s(&match result{Ok(())=>format!("Recorded investigation {}",text(descriptor,"id")),Err(e)=>format!("Investigation not saved: {e}")}));
}
pub(super) fn validate(args:&Json)->Result<()>{
 let Obj(fields)=args else{return Err("Observation query requires an object".into());};
 if fields.keys().any(|k|!["id","query","offset","snapshot","node","path_hex","children","path","legacy"].contains(&k.as_str())){return Err("Observation query accepts id, query, path, legacy, offset, snapshot, node, path_hex and children only".into());}
 for(key,cap)in[("id",128),("query",512),("path_hex",8192),("path",1024)]{if get(args,key)!=&Null&&!bounded_string(args,key,cap){return Err(format!("Observation {key} must be a bounded string"));}}
 if get(args,"offset")!=&Null{let Num(v)=get(args,"offset")else{return Err("Observation offset must be a nonnegative integer".into());};if v.parse::<usize>().ok().filter(|n|*n<=500).is_none(){return Err("Observation offset exceeds 500".into());}}
 if get(args,"path_hex")!=&Null{let path=text(args,"path_hex");if path.len()%2!=0||!path.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err("Observation path_hex requires lowercase hexadecimal exact path bytes".into());}}
 let typed=["snapshot","node","path_hex","children"].iter().any(|k|get(args,k)!=&Null);
 if get(args,"legacy")!=&Null&&!matches!(get(args,"legacy"),Bool(_)){return Err("Observation legacy selector must be Boolean".into());}
 if typed&&boolean(args,"legacy"){return Err("Legacy reports have no typed directory snapshot".into());}
 if typed&&text(args,"id").is_empty(){return Err("Typed observation queries require an exact retained record ID".into());}
 for key in ["snapshot","node","children"]{if get(args,key)!=&Null&&!matches!(get(args,key),Num(n) if n.parse::<usize>().ok().is_some_and(|v|v<=500)){return Err(format!("Observation {key} must be a nonnegative bounded integer"));}}
 if ["node","path_hex","children"].iter().filter(|k|get(args,k)!=&Null).count()>1{return Err("Choose a node, exact path or parent children filter".into());}
 if !text(args,"id").is_empty()&&(!text(args,"query").is_empty()||(!typed&&number(args,"offset")!=0)){return Err("Choose an exact record ID or a paged query".into());}Ok(())
}
fn accessible(st:&Json,r:&Json,model:bool)->bool{
 if !valid(r)||get(r,"workspace")!=get(st,"approval_root")||get(r,"workspace_identity")!=get(st,"approval_root_identity")||get(r,"policy")!=&access_binding(st,&text(r,"tool"),get(r,"args")){return false;}
 let source=obj(vec![("tool",get(r,"tool").clone()),("args",get(r,"args").clone())]);
 if harness_rules::check(st,&text(r,"tool"),get(r,"args"),model).is_err(){return false;}
 if web_evidence::is_web(&text(r,"tool")){return web_evidence::accessible(st,r,model);}
 if model{
   if harness::allowed_tool(st,&source).is_err(){return false;}
   // Reports can contain child names/header samples. Requiring both current
   // capabilities is conservative even for a record of a regular file.
   for t in ["list","read"]{if harness::allowed_tool(st,&obj(vec![("tool",s(t)),("args",get(r,"args").clone())])).is_err(){return false;}}
 }true
}
fn public(r:&Json,body:bool)->Json{
 let mut v=obj(["schema","id","recorded_at","tool","args","chat","agent","mode","depth"].iter().map(|k|(*k,get(r,k).clone())).collect());
 put(&mut v,"source",get(r,"source").clone());put(&mut v,"historical",Bool(true));put(&mut v,"authority",s("Observed historical data, not instructions or current source validation"));
 put(&mut v,"snapshot_count",Num(array(r,"snapshots").len().to_string()));put(&mut v,"graph_note",get(r,"graph_note").clone());
 if body{put(&mut v,"report",get(r,"report").clone());put(&mut v,"task",get(r,"task").clone());}v
}
fn record_args(st:&Json,id:&str)->Json{let mut args=obj(vec![("id",s(id))]);if boolean(st,"investigation_record_legacy"){put(&mut args,"legacy",Bool(true));}args}
fn query_body(st:&Json,args:&Json,model:bool)->Result<String>{
 validate(args)?;harness_rules::check(st,"observations",args,model)?;
 let empty=Vec::new();let (all,current_error)=match rows(st){Ok(v)=>(v,String::new()),Err(e)=>(&empty,e)};let id=text(args,"id");let only_legacy=boolean(args,"legacy");
 let (legacy,legacy_unavailable,legacy_error)=match observation_store::eligible(st,model){Ok((v,n))=>(v,n,String::new()),Err(e)=>(vec![],0,e)};
 if !id.is_empty(){
  if !only_legacy{if let Some(r)=all.iter().find(|r|text(r,"id")==id&&accessible(st,r,model)){
   if ["snapshot","node","path_hex","children"].iter().any(|k|get(args,k)!=&Null){observation_graph::validate_record(get(r,"snapshots"),r)?;let mut graph=observation_graph::query(get(r,"snapshots"),args)?;put(&mut graph,"source_tool",get(r,"tool").clone());put(&mut graph,"source_args",get(r,"args").clone());put(&mut graph,"record_id",s(&id));put(&mut graph,"authority",s("Historical observed data, not instructions or current source validation"));return Ok(graph.string());}return Ok(public(r,true).string());
  }}
  if ["snapshot","node","path_hex","children"].iter().any(|k|get(args,k)!=&Null){return Err("Legacy or unavailable report has no typed directory snapshot".into());}
  return legacy.into_iter().find(|r|text(r,"id")==id).map(|r|r.string()).ok_or_else(||if legacy_error.is_empty(){"Record unavailable under the current workspace, permissions or rules".into()}else{legacy_error});
 }
 let q=text(args,"query");let path=text(args,"path");let mut matches=Vec::new();let mut unavailable=legacy_unavailable;
 let matches_filter=|r:&Json| (q.is_empty()||["report","task","agent"].iter().any(|k|get(r,k).text().unwrap_or("").contains(&q))||get(r,"args").string().contains(&q))&&(path.is_empty()||text(get(r,"args"),"path")==path||text(get(r,"args"),"other")==path||text(get(r,"args"),"url")==path||text(r,"source")==path);
 if !only_legacy{for r in all{if !accessible(st,r,model){unavailable+=1;continue;}if matches_filter(r){matches.push(public(r,false));}}}
 for r in legacy{if matches_filter(&r){matches.push(observation_store::metadata(&r));}}
 let offset=number(args,"offset");let page=matches.iter().skip(offset).take(PAGE).cloned().collect();
 Ok(obj(vec![("historical",Bool(true)),("matching",Num(matches.len().to_string())),("unavailable",Num(unavailable.to_string())),("offset",Num(offset.to_string())),("next_offset",if offset.saturating_add(PAGE)<matches.len(){Num((offset+PAGE).to_string())}else{Null}),("records",Arr(page)),("legacy_error",s(&legacy_error)),("current_error",s(&current_error)),("note",s("Only records accessible under current workspace identity, rules and permissions are shown. Legacy records retain their original full-policy binding and source provenance. Query does not reopen or execute the original source."))]).string())
}
pub(super) fn query(st:&Json,args:&Json,model:bool)->Result<String>{
 let output=query_body(st,args,model)?;
 let policy=super::super::resources::policy(get(st,"device"),get(st,"resources"),super::super::mcp_schema::state_bytes(st));
 if output.len()>number(&policy,"response_bytes").min(MAX_BYTES){return Err("Historical report exceeds the current response memory allowance; saved record is retained".into());}
 Ok(output)
}
fn management(st:&Json)->Result<Vec<Json>>{let mut all=rows(st).map(|v|v.as_slice()).unwrap_or(&[]).iter().filter(|r|valid(r)).map(|r|obj(vec![("id",get(r,"id").clone()),("recorded_at",get(r,"recorded_at").clone()),("tool",get(r,"tool").clone()),("legacy",Bool(false))])).collect::<Vec<_>>();if let Ok(old)=observation_store::management(st){all.extend(old);}Ok(all)}
fn managed_exists(st:&Json,id:&str)->Result<bool>{Ok(management(st)?.iter().any(|r|text(r,"id")==id&&boolean(r,"legacy")==boolean(st,"investigation_record_legacy")))}
pub(super) fn handles(a:&str)->bool{["investigation_records","investigation_records_search","investigation_records_next","investigation_records_previous","investigation_records_toggle","investigation_record_delete","investigation_record_delete_confirm","investigation_records_manage","investigation_records_manage_next","investigation_records_manage_previous","investigation_graph","investigation_graph_all","investigation_graph_next","investigation_graph_previous","investigation_graph_side"].contains(&a)||a.starts_with("investigation_legacy_record:")||a.starts_with("investigation_record:")||a.starts_with("investigation_record_manage:")||a.starts_with("investigation_legacy_manage:")||a.starts_with("investigation_graph_node:")||a.starts_with("investigation_graph_children:")}
pub(super) fn event(st:&mut Json,a:&str)->Result<()>{
 if interaction::locked(st){return Err("Finish or cancel the current operation before browsing investigation records".into());}
 if a.starts_with("investigation_graph"){
  let id=text(st,"investigation_record_id");let mut args=get(st,"investigation_graph_query").clone();if a=="investigation_graph"{args=obj(vec![("id",s(&id)),("snapshot",Num("0".into()))]);}
  else if a=="investigation_graph_all"{args=obj(vec![("id",s(&id)),("snapshot",get(&args,"snapshot").clone())]);}
  else if a=="investigation_graph_side"{let side=number(&args,"snapshot");args=obj(vec![("id",s(&id)),("snapshot",Num((1-side.min(1)).to_string()))]);}
  else if let Some(n)=a.strip_prefix("investigation_graph_node:"){let index=n.parse::<usize>().map_err(|_|"Invalid observation node")?;args=obj(vec![("id",s(&id)),("snapshot",get(&args,"snapshot").clone()),("node",Num(index.to_string()))]);}
  else if let Some(n)=a.strip_prefix("investigation_graph_children:"){let index=n.parse::<usize>().map_err(|_|"Invalid observation parent")?;args=obj(vec![("id",s(&id)),("snapshot",get(&args,"snapshot").clone()),("children",Num(index.to_string()))]);}
  else{let offset=number(&args,"offset");put(&mut args,"offset",Num(if a=="investigation_graph_next"{offset.saturating_add(PAGE)}else{offset.saturating_sub(PAGE)}.to_string()));}
  query(st,&args,false)?;put(st,"investigation_graph_query",args);put(st,"dialog",s("investigation_graph"));return Ok(());
 }
 if let Some(id)=a.strip_prefix("investigation_record:").or_else(||a.strip_prefix("investigation_legacy_record:")){let legacy=a.starts_with("investigation_legacy_record:");let mut args=obj(vec![("id",s(id))]);if legacy{put(&mut args,"legacy",Bool(true));}let shown=parse(&query(st,&args,false)?)?;put(st,"investigation_record_legacy",Bool(boolean(&shown,"legacy")));put(st,"investigation_record_id",s(id));put(st,"investigation_record_management",Bool(false));put(st,"dialog",s("investigation_record"));return Ok(());}
 if a.starts_with("investigation_records_manage"){
  management(st)?;let offset=number(st,"investigation_management_offset");let offset=match a{"investigation_records_manage_next"=>offset.saturating_add(PAGE).min(MAX_ROWS+32),"investigation_records_manage_previous"=>offset.saturating_sub(PAGE),_=>0};
  put(st,"investigation_management_offset",Num(offset.to_string()));put(st,"dialog",s("investigation_records_manage"));return Ok(());
 }
 if let Some(id)=a.strip_prefix("investigation_record_manage:").or_else(||a.strip_prefix("investigation_legacy_manage:")){
  let legacy=a.starts_with("investigation_legacy_manage:");if text(st,"dialog")!="investigation_records_manage"||!management(st)?.iter().any(|r|text(r,"id")==id&&boolean(r,"legacy")==legacy){return Err("Select a saved report from retention management".into());}
  put(st,"investigation_record_legacy",Bool(legacy));put(st,"investigation_record_id",s(id));put(st,"investigation_record_management",Bool(true));put(st,"dialog",s("investigation_record_delete"));return Ok(());
 }
 if a=="investigation_record_delete"||a=="investigation_record_delete_confirm"{
  let expected=if a=="investigation_record_delete"{"investigation_record"}else{"investigation_record_delete"};
  if text(st,"dialog")!=expected{return Err("Open the exact retained report before confirming deletion".into());}
  let id=text(st,"investigation_record_id");if !boolean(st,"investigation_record_management"){query(st,&record_args(st,&id),false)?;}else if !managed_exists(st,&id)?{return Err("Selected retained report no longer exists".into());}
  if a=="investigation_record_delete"{put(st,"dialog",s("investigation_record_delete"));return Ok(());}
  if boolean(st,"investigation_record_legacy"){observation_store::remove(st,&id)?;}else{let items=rows(st)?.iter().filter(|r|text(r,"id")!=id).cloned().collect();let store=store_with_items(st,items);put(st,KEY,store);}
  put(st,"investigation_retention",s("Deleted only the selected retained report; source files and conversation history are unchanged."));put(st,"investigation_record_id",Null);put(st,"dialog",s("investigation_records"));return Ok(());
 }
 if a=="investigation_records_toggle"{put(st,"investigation_retention_off",Bool(!boolean(st,"investigation_retention_off")));put(st,"investigation_retention",s(if boolean(st,"investigation_retention_off"){"Retaining new reports is off. Existing records remain."}else{"Retaining new successful reports is on."}));}
 else if a=="investigation_records"{put(st,"edit",obj(vec![]));put(st,"investigation_offset",Num("0".into()));}
 else if a=="investigation_records_search"{put(st,"investigation_offset",Num("0".into()));}
 else{let offset=number(st,"investigation_offset");put(st,"investigation_offset",Num(if a=="investigation_records_next"{offset.saturating_add(PAGE).min(MAX_ROWS+32)}else{offset.saturating_sub(PAGE)}.to_string()));}
 query(st,&obj(vec![("query",s(&text(get(st,"edit"),"query"))),("offset",get(st,"investigation_offset").clone())]),false)?;
 put(st,"dialog",s("investigation_records"));Ok(())
}
pub(super) fn dialog(st:&Json)->Json{
 let dialog=text(st,"dialog");
 if dialog=="investigation_graph"{
  let args=get(st,"investigation_graph_query");let mut buttons=vec![button("All nodes","investigation_graph_all"),button("Back to report",&format!("investigation_record:{}",text(st,"investigation_record_id")))];
  if get(args,"node")!=&Null{buttons.push(button("Form hypothesis about this node","claim_from_node"));buttons.push(button("Reconstruction from this node","reconstruction_new"));}
  let content=match query(st,args,false).and_then(|v|parse(&v)){Ok(v)=>{
   for n in array(&v,"nodes"){let index=number(&n,"id");buttons.push(button(&format!("{} · {}",text(&n,"kind"),text(&n,"path")),&format!("investigation_graph_node:{index}")));if text(&n,"kind")=="Directory"{buttons.push(button(&format!("Children of {}",text(&n,"path")),&format!("investigation_graph_children:{index}")));}}
   if number(&v,"snapshot_count")>1{buttons.push(button("Other comparison snapshot","investigation_graph_side"));}if number(args,"offset")>0{buttons.push(button("Previous nodes","investigation_graph_previous"));}if get(&v,"next_offset")!=&Null{buttons.push(button("Next nodes","investigation_graph_next"));}let mut display=format!("Snapshot {} of {} · {} coverage
{} matching nodes; page starts at {}. Stored observations; no source was reopened.
",number(&v,"snapshot")+1,number(&v,"snapshot_count"),text(&v,"coverage"),number(&v,"matching"),number(&v,"offset"));
   for n in array(&v,"nodes"){display.push_str(&format!("\n[{}] {} · {} · {}\nExact path hex: {}\nObserved identity: {}\n",number(&n,"id"),text(&n,"path"),text(&n,"kind"),text(&n,"coverage"),text(&n,"path_hex"),get(&n,"identity").string()));let rep=get(&n,"representation");if rep!=&Null{display.push_str(&format!("Representation: {}\nBytes: {}; SHA-256: {}\nObserved data: {}\n",text(rep,"method"),get(rep,"bytes").string(),text(rep,"sha256"),get(&n,"data").string()));}let sample=get(&n,"sample");if sample!=&Null{display.push_str(&format!("Captured bytes {} + {}; complete file: {}\nSample SHA-256: {}\n",get(sample,"offset").string(),get(sample,"length").string(),boolean(sample,"complete_file"),text(sample,"sha256")));}else{display.push_str("No content sample retained.\n");}for note in array(&n,"notes"){display.push_str(note.text().unwrap_or(""));display.push('\n');}}
   display
  },Err(e)=>e};return modal("Observed relationships",vec![field_ui("observations","Stored typed observations",&content,"readonly")],buttons);
 }

 if dialog=="investigation_records_manage"{
  let mut buttons=vec![];let mut summary=match management(st){Ok(items)=>{let offset=number(st,"investigation_management_offset");for r in items.iter().skip(offset).take(PAGE){buttons.push(button(&format!("{} · {} · {}",text(r,"tool"),age(r),text(r,"id")),&format!("{}:{}",if boolean(r,"legacy"){"investigation_legacy_manage"}else{"investigation_record_manage"},text(r,"id"))));}if offset>0{buttons.push(button("Previous records","investigation_records_manage_previous"));}if offset+PAGE<items.len(){buttons.push(button("Next records","investigation_records_manage_next"));}format!("{} retained reports across workspaces. IDs and recording times only; source names, contents and task text are not shown. Select an exact report to confirm deletion and recover storage capacity.",items.len())},Err(e)=>e};if let Err(e)=rows(st){summary.push_str(&format!("\n{}",e));}if let Err(e)=observation_store::management(st){summary.push_str(&format!("\n{}",e));}buttons.push(button("Back to records","investigation_records"));return modal("Retention management",vec![field_ui("status","Saved reports",&summary,"readonly")],buttons);
 }
 if dialog=="investigation_record"||dialog=="investigation_record_delete"{
  let id=text(st,"investigation_record_id");let output=if dialog=="investigation_record_delete"&&boolean(st,"investigation_record_management"){managed_exists(st,&id).and_then(|yes|if yes{Ok(String::new())}else{Err("Selected retained report no longer exists".into())})}else{query(st,&record_args(st,&id),false)};
  return match output{Ok(output)=>if dialog=="investigation_record_delete"{
   modal("Delete retained report?",vec![field_ui("record","Exact report ID",&id,"readonly"),field_ui("deletion","Effect","Permanently delete only this saved investigation report. Original source files and conversation history remain. This does not erase earlier exports or backups.","readonly")],vec![button("Delete this retained report","investigation_record_delete_confirm"),button("Keep report","investigation_records")])
  }else{{let r=parse(&output).unwrap_or(Null);modal("Investigation report",vec![field_ui("source","Source",&format!("{} · {}{}",text(&r,"tool"),if web_evidence::is_web(&text(&r,"tool")){format!("{} · {}",text(&r,"source"),if text(&r,"tool")=="web_search"{text(get(&r,"args"),"query")}else{text(get(&r,"args"),"url")})}else{text(get(&r,"args"),"path")},if text(&r,"tool")=="compare"{format!(" ↔ {}",text(get(&r,"args"),"other"))}else{String::new()}),"readonly"),field_ui("provenance","Recorded context",&format!("{}
Agent: {}
Conversation: {}
Task: {}
Historical observation, not current source validation.",age(&r),if boolean(&r,"legacy"){"not recorded by legacy schema".into()}else{text(&r,"agent")},text(&r,"chat"),text(&r,"task")),"readonly"),field_ui("legacy_source","Original legacy provenance",&if boolean(&r,"legacy"){get(&r,"legacy_source").string()}else{String::new()},"readonly"),field_ui("report","Report",&text(&r,"report"),"readonly"),field_ui("typed","Typed observations",&format!("{} snapshots. {}",number(&r,"snapshot_count"),text(&r,"graph_note")),"readonly")],{let mut buttons=vec![button("Delete retained report","investigation_record_delete"),button("Back to records","investigation_records")];if number(&r,"snapshot_count")>0{buttons.insert(0,button("Browse observed relationships","investigation_graph"));}buttons})}},Err(e)=>modal("Investigation report",vec![field_ui("status","Unavailable",&e,"readonly")],vec![button("Back to records","investigation_records")])};
 }
 let args=obj(vec![("query",s(&text(get(st,"edit"),"query"))),("offset",get(st,"investigation_offset").clone())]);
 let result=query(st,&args,false).and_then(|s|parse(&s));let mut buttons=vec![button("Search records","investigation_records_search"),button("Manage retained reports","investigation_records_manage"),button(if boolean(st,"investigation_retention_off"){"Retain new reports: off"}else{"Retain new reports: on"},"investigation_records_toggle")];
 let summary=match result{Ok(v)=>{for r in array(&v,"records"){buttons.push(button(&format!("{} · {} · {}",text(&r,"tool"),if web_evidence::is_web(&text(&r,"tool")){format!("{} · {}",text(&r,"source"),if text(&r,"tool")=="web_search"{text(get(&r,"args"),"query")}else{text(get(&r,"args"),"url")})}else{text(get(&r,"args"),"path")},age(&r)),&format!("{}:{}",if boolean(&r,"legacy"){"investigation_legacy_record"}else{"investigation_record"},text(&r,"id"))));}if number(st,"investigation_offset")>0{buttons.push(button("Previous records","investigation_records_previous"));}if get(&v,"next_offset")!=&Null{buttons.push(button("Next records","investigation_records_next"));}format!("{} matching historical records; {} records unavailable under current workspace, permissions or changed rules. Saved report text and source/task provenance; these are not current source checks or rules.\n{}",number(&v,"matching"),number(&v,"unavailable"),format!("{}\n{}\n{}",text(st,"investigation_retention"),text(&v,"current_error"),text(&v,"legacy_error")))},Err(e)=>e};
 modal("Investigation records",vec![field_ui("query","Search report or source text",&text(get(st,"edit"),"query"),"text"),field_ui("status","Retained observations",&summary,"readonly")],buttons)
}
