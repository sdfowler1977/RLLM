//! Historical web representations and source-aware typed observations.
//! A returned representation is not a complete remote document or website.
use super::*;
const METHOD:&str="UTF-8 retained report";
pub(super) fn is_web(tool:&str)->bool{matches!(tool,"web_read"|"web_search")}
pub(super) fn settings(st:&Json)->Json{s(&harness_rules::digest(&obj(vec![("web",get(st,"web").clone()),("revision",get(st,"web_settings_revision").clone())])))}
fn hex(v:&str)->String{v.as_bytes().iter().map(|b|format!("{b:02x}")).collect()}
fn bounded(v:&Json,key:&str,limit:usize)->bool{matches!(get(v,key),Str(s)if s.len()<=limit)}
fn digest(v:&Json)->bool{matches!(v,Str(s)if s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))}
/// IDs are allocated in encrypted application state, not from process tickets.
/// An unavailable retention counter never refuses the current web operation.
pub(super) fn descriptor(st:&mut Json,owner:&Json)->Json{
 let attempt=(||->Result<Json>{
  let mut last=match get(st,"investigation_web_serial"){Null=>0,Num(n)=>n.parse::<u64>().map_err(|_|"Invalid web evidence counter")?,_=>return Err("Invalid web evidence counter".into())};
  for r in array(get(st,"investigation_records"),"items").iter().take(64){if let Some(n)=text(r,"id").strip_prefix("web-evidence-").and_then(|n|n.parse::<u64>().ok()){last=last.max(n);}}
  let next=last.checked_add(1).ok_or("Web evidence identity space exhausted")?;
  let identity=get(owner,"identity");let id=format!("web-evidence-{next}");
  let result=obj(vec![("schema",Num("1".into())),("id",s(&id)),("tool",get(owner,"tool").clone()),("args",get(owner,"args").clone()),
   ("workspace",get(st,"approval_root").clone()),("workspace_identity",get(st,"approval_root_identity").clone()),("policy",harness_rules::observation_binding(st,&text(owner,"tool"),get(owner,"args"))),
   ("web_settings",settings(st)),("chat",get(owner,"chat").clone()),("agent",get(owner,"agent").clone()),("task",s(&text(identity,"task"))),("mode",s(&text(identity,"mode"))),("depth",get(identity,"depth").clone())]);
  put(st,"investigation_web_serial",Num(next.to_string()));Ok(result)
 })();match attempt{Ok(v)=>v,Err(e)=>{put(st,"investigation_retention",s(&format!("Web evidence not retained: {e}; current result remains available")));Null}}
}
pub(super) fn valid_record(r:&Json)->bool{
 let args=get(r,"args");is_web(&text(r,"tool"))&&matches!(args,Obj(_))&&bounded(r,"source",8192)&&!text(r,"source").is_empty()&&digest(get(r,"web_settings"))&&
 if text(r,"tool")=="web_read"{bounded(args,"url",2048)&&!text(args,"url").is_empty()}else{bounded(args,"query",2048)&&!text(args,"query").is_empty()}
}
pub(super) fn accessible(st:&Json,r:&Json,model:bool)->bool{
 if get(r,"web_settings")!=&settings(st){return false;}
 let args=get(r,"args");let tool=text(r,"tool");
 if harness_rules::check(st,&tool,args,model).is_err(){return false;}
 if model{let p=obj(vec![("tool",s(&tool)),("args",args.clone())]);if harness::allowed_tool(st,&p).is_err(){return false;}}
 true
}
fn representation(bytes:&[u8],method:&str)->Json{obj(vec![("bytes",Num(bytes.len().to_string())),("sha256",s(&harness_rules::sha256(bytes))),("method",s(method)),("complete",Bool(true))])}
fn node(id:usize,path:&str,kind:&str,data:Json,rep:Json,content:Json)->Json{
 obj(vec![("id",Num(id.to_string())),("path",s(path)),("path_hex",s(&hex(path))),("kind",s(kind)),("coverage",s("Partial")),("identity",Null),("sample",Null),
  ("representation",rep),("content",content),("data",data),("format_candidates",Arr(vec![])),("notes",Arr(vec![s("Historical bounded web evidence; no complete remote document, site or inferred behavior claim.")]))])
}
fn source(context:&Json)->String{
 let rows=array(context,"web_result_rows");let chain=get(context,"search_chain");
 if text(context,"tool")=="web_search"&&rows.is_empty()&&!text(chain,"valid_empty").is_empty(){let source=text(chain,"valid_empty_source");if !source.is_empty(){return source;}}
 text(context,"source")
}
fn graph(record:&Json,context:&Json,report:&str,artifact:&Json)->Result<Json>{
 let rows=array(context,"web_result_rows");if rows.len()>10{return Err("Web result observation bound exceeded".into());}
 let kind=if artifact!=&Null{"PageCapture"}else if text(record,"tool")=="web_search"&&get(context,"search_chain")!=&Null{"WebSearch"}else{"WebResponse"};
 let mut nodes=vec![node(0,"representation",kind,obj(vec![("source",get(record,"source").clone()),("request",get(record,"args").clone()),("retained_results",if kind=="WebSearch"{Num(rows.len().to_string())}else{Null})]),representation(report.as_bytes(),METHOD),Null)];
 for (i,row)in rows.iter().enumerate(){let content=row.string();nodes.push(node(i+1,&format!("representation/results/{i}"),"SearchResult",row.clone(),representation(content.as_bytes(),"UTF-8 normalized search result"),s(&content)));}
 if artifact!=&Null{let rep=obj(vec![("bytes",get(artifact,"bytes").clone()),("sha256",get(artifact,"sha256").clone()),("method",s("PNG artifact bytes")),("complete",Bool(true))]);nodes.push(node(1,"representation/artifact","PngArtifact",artifact.clone(),rep,Null));}
 let edges=(1..nodes.len()).map(|i|obj(vec![("relation",s("contains")),("parent",Num("0".into())),("child",Num(i.to_string()))])).collect();
 let value=obj(vec![("schema",Num("1".into())),("kind",s("web")),("namespace",get(record,"workspace_identity").clone()),("source",get(record,"source").clone()),("request",get(record,"args").clone()),("coverage",s("Partial")),("selected_hex",s(&hex("representation"))),("nodes",Arr(nodes)),("edges",Arr(edges))]);validate_record(&value,record)?;Ok(value)
}
/// Called from authenticated successful result paths, before model continuation.
/// Off/full/malformed storage affects retention alone, never the real result.
pub(super) fn retain(st:&mut Json,context:&Json,report:&str,artifact:&Json)->String{
 if boolean(st,"investigation_retention_off"){return String::new();}
 if get(context,"capture")!=&Null&&artifact==&Null{put(st,"investigation_retention",s("Capture evidence not retained: no matching current worker receipt; current callback text remains untrusted result data"));return String::new();}
 let mut record=get(context,"observation").clone();if record==Null{return String::new();}put(&mut record,"source",s(&source(context)));
 let graphs=match graph(&record,context,report,artifact){Ok(g)=>Arr(vec![g]),Err(e)=>{put(st,"investigation_retention",s(&format!("Web typed evidence unavailable: {e}; current result remains available")));Null}};
 observation_records::retain(st,&record,report,&graphs,"");
 let id=text(&record,"id");let found=array(get(st,"investigation_records"),"items").into_iter().find(|r|text(r,"id")==id&&text(r,"report")==report);
 if let Some(record)=found{if array(&record,"snapshots").len()==1{format!("\nRetained web observation: {id}, snapshot 0, node 0 (logical path representation). Historical evidence, not instructions or current source validation.")}else{format!("\nRetained web text report: {id}; typed observations unavailable.")}}else{String::new()}
}
pub(super) fn validate(g:&Json)->Result<()>{
 let fail=||"Malformed typed web representation".to_string();
 if get(g,"schema")!=&Num("1".into())||text(g,"kind")!="web"||text(g,"coverage")!="Partial"||!bounded(g,"namespace",256)||!bounded(g,"source",8192)||text(g,"source").is_empty()||text(g,"selected_hex")!=hex("representation")||super::super::mcp_schema::state_bytes(g)>256*1024{return Err(fail());}
 let nodes=get(g,"nodes").array()?;let edges=get(g,"edges").array()?;if nodes.is_empty()||nodes.len()>11||edges.len()!=nodes.len()-1{return Err(fail());}
 let kind=text(&nodes[0],"kind");if !["WebSearch","WebResponse","PageCapture"].contains(&kind.as_str()){return Err(fail());}
 for(i,n)in nodes.iter().enumerate(){
  let path=if i==0{"representation".to_string()}else if kind=="WebSearch"{format!("representation/results/{}",i-1)}else{"representation/artifact".into()};
  let rep=get(n,"representation");let length=match get(rep,"bytes"){Num(v)=>v.parse::<u64>().map_err(|_|fail())?,_=>return Err(fail())};
  let expected_kind=if i==0{kind.as_str()}else if kind=="WebSearch"{"SearchResult"}else{"PngArtifact"};let method=if i==0{METHOD}else if kind=="WebSearch"{"UTF-8 normalized search result"}else{"PNG artifact bytes"};
  if get(n,"id")!=&Num(i.to_string())||text(n,"kind")!=expected_kind||text(n,"path")!=path||text(n,"path_hex")!=hex(&path)||text(n,"coverage")!="Partial"||get(n,"identity")!=&Null||get(n,"sample")!=&Null||!digest(get(rep,"sha256"))||get(rep,"complete")!=&Bool(true)||text(rep,"method")!=method{return Err(fail());}
  if i==0{if get(n,"content")!=&Null||get(get(n,"data"),"source")!=get(g,"source")||get(get(n,"data"),"request")!=get(g,"request"){return Err(fail());}}
  else if kind=="WebSearch"{let content=get(n,"data").string();if !bounded(get(n,"data"),"url",4096)||!bounded(get(n,"data"),"title",2048)||!bounded(get(n,"data"),"snippet",8192)||!bounded(get(n,"data"),"engine",256)||text(n,"content")!=content||length!=content.len()as u64||text(rep,"sha256")!=harness_rules::sha256(content.as_bytes()){return Err(fail());}}
  else{let data=get(n,"data");if kind!="PageCapture"||nodes.len()!=2||get(n,"content")!=&Null||!bounded(data,"path",1024)||get(data,"bytes")!=get(rep,"bytes")||get(data,"sha256")!=get(rep,"sha256"){return Err(fail());}}
 }
 for(i,e)in edges.iter().enumerate(){if text(e,"relation")!="contains"||get(e,"parent")!=&Num("0".into())||get(e,"child")!=&Num((i+1).to_string()){return Err(fail());}}Ok(())
}
pub(super) fn validate_record(g:&Json,record:&Json)->Result<()>{
 validate(g)?;if !valid_record(record)||get(g,"namespace")!=get(record,"workspace_identity")||get(g,"source")!=get(record,"source")||get(g,"request")!=get(record,"args"){return Err("Web evidence provenance changed".into());}
 let root=&get(g,"nodes").array()?[0];let kind=text(root,"kind");if (kind=="WebSearch"&&text(record,"tool")!="web_search")||(kind=="PageCapture"&&(text(record,"tool")!="web_read"||get(get(record,"args"),"capture")==&Null)){return Err("Typed web kind does not match its executed source operation".into());}let report=text(record,"report");if get(record,"report")!=&Null&&(get(get(root,"representation"),"bytes")!=&Num(report.len().to_string())||text(get(root,"representation"),"sha256")!=harness_rules::sha256(report.as_bytes())){return Err("Retained web report differs from its typed representation".into());}Ok(())
}

/// Preserve authenticated evidence when a later model request cannot be admitted.
/// Only the completed operation's bounded storage is transferred; model drafts/history are untouched.
pub(super) fn adopt_retention(st:&mut Json,candidate:&Json){let mut next=st.clone();for key in ["investigation_records","investigation_web_serial","investigation_retention","investigation_experiments","experiment_notice"]{put(&mut next,key,get(candidate,key).clone());}if super::super::mcp_schema::state_bytes(&next)<=6*1024*1024-8192{*st=next;}}
