//! Immutable directory observation projections; retained samples are hashes, not bytes.
use super::*;
use investigation::directory_observation::{Report,Identity};
const CAP:usize=256*1024;
fn hex(b:&[u8])->String{b.iter().map(|b|format!("{b:02x}")).collect()}
fn identity(i:&Identity)->Json{obj(vec![("dev",Num(i.dev.to_string())),("ino",Num(i.ino.to_string())),("size",Num(i.size.to_string())),("modified_seconds",Num(i.modified_seconds.to_string())),("modified_nanos",Num(i.modified_nanos.to_string())),("changed_seconds",Num(i.changed_seconds.to_string())),("changed_nanos",Num(i.changed_nanos.to_string()))])}
fn strings(v:&[&str])->Json{Arr(v.iter().map(|v|s(v)).collect())}
pub(super) fn snapshot(r:&Report)->Result<Json>{
 let mut nodes=Vec::new();let mut cost=0usize;
 for (index,n) in r.occurrences.iter().enumerate(){
  if super::super::execution::cancelled(){return Err("Typed observation projection cancelled; no graph retained".into());}
  let sample=n.sample.as_ref().map(|p|obj(vec![("offset",Num(p.offset.to_string())),("length",Num(p.bytes.len().to_string())),("complete_file",Bool(p.complete_file)),("sha256",s(&harness_rules::sha256(&p.bytes))),("method",s("SHA-256 of exactly the captured sample bytes; a prefix digest is not whole-file identity"))])).unwrap_or(Null);
  let node=obj(vec![("id",Num(index.to_string())),("path_hex",s(&hex(&n.path))),("path",s(&String::from_utf8_lossy(&n.path))),("kind",s(&format!("{:?}",n.kind))),("identity",n.identity.as_ref().map(identity).unwrap_or(Null)),("coverage",s(&format!("{:?}",n.coverage))),("sample",sample),("format_candidates",strings(&n.format_candidates)),("notes",strings(&n.notes))]);
  cost=cost.saturating_add(super::super::mcp_schema::state_bytes(&node));if cost>CAP{return Err("Typed directory snapshot exceeds 256 KiB; textual observation remains available".into());}nodes.push(node);
 }
 let graph=obj(vec![("schema",Num("1".into())),("kind",s("directory")),("selected_hex",s(&hex(&r.selected))),("workspace_identity",identity(&r.workspace_identity)),("coverage",s(&format!("{:?}",r.coverage))),("sampled_bytes",Num(r.sampled_bytes.to_string())),("bytes_read",Num(r.bytes_read.to_string())),("nodes",Arr(nodes)),("edges",Arr(r.contains.iter().map(|e|obj(vec![("relation",s("contains")),("parent",Num(e.parent.to_string())),("child",Num(e.child.to_string()))])).collect())),("notes",strings(&r.notes))]);
 if super::super::mcp_schema::state_bytes(&graph)>CAP{return Err("Typed directory snapshot exceeds 256 KiB; textual observation remains available".into());}validate(&graph)?;Ok(graph)
}
fn integer(v:&Json)->Option<usize>{if let Num(n)=v{n.parse().ok()}else{None}}
fn raw_path(v:&Json)->bool{matches!(v,Str(t) if t.len()<=8192&&t.len()%2==0&&t.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))}
fn coverage(v:&Json)->bool{matches!(v,Str(t) if ["Complete","Partial","Changed"].contains(&t.as_str()))}
fn decode_path(v:&Json)->Option<Vec<u8>>{
 if !raw_path(v){return None;}let text=v.text().ok()?;let mut bytes=Vec::with_capacity(text.len()/2);
 for pair in text.as_bytes().chunks_exact(2){let digit=|b:u8|if b<=b'9'{b-b'0'}else{b-b'a'+10};bytes.push(digit(pair[0])*16+digit(pair[1]));}
 if bytes.contains(&0)||(!bytes.is_empty()&&bytes.split(|b|*b==b'/').any(|part|part.is_empty()||part==b"."||part==b"..")){return None;}Some(bytes)
}
fn texts(v:&Json,count:usize,bytes:usize)->bool{matches!(v,Arr(a)if a.len()<=count&&a.iter().all(|v|matches!(v,Str(s)if s.len()<=bytes)))}
pub(super) fn valid_identity(v:&Json)->bool{if v==&Null{return true;}["dev","ino","size"].iter().all(|k|matches!(get(v,k),Num(n) if n.parse::<u64>().is_ok()))&&["modified_seconds","changed_seconds"].iter().all(|k|matches!(get(v,k),Num(n) if n.parse::<i64>().is_ok()))&&["modified_nanos","changed_nanos"].iter().all(|k|matches!(get(v,k),Num(n) if n.parse::<u32>().ok().is_some_and(|n|n<1_000_000_000)))}
pub(super) fn validate(g:&Json)->Result<()>{
 if text(g,"kind")=="web"{return web_evidence::validate(g);}
 if text(g,"kind")=="complete_file"{return file_evidence::validate_graph(g);}
 let fail=||"Malformed or unsupported typed observation; textual record remains available".to_string();
 if get(g,"schema")!=&Num("1".into())||text(g,"kind")!="directory"||!coverage(get(g,"coverage"))||get(g,"workspace_identity")==&Null||!valid_identity(get(g,"workspace_identity"))||super::super::mcp_schema::state_bytes(g)>CAP||!texts(get(g,"notes"),64,512){return Err(fail());}
 let selected=decode_path(get(g,"selected_hex")).ok_or_else(fail)?;
 let sampled=integer(get(g,"sampled_bytes")).ok_or_else(fail)?;let read=integer(get(g,"bytes_read")).ok_or_else(fail)?;if sampled>read||read>1024*1024{return Err(fail());}
 let Arr(nodes)=get(g,"nodes")else{return Err(fail());};let Arr(edges)=get(g,"edges")else{return Err(fail());};if nodes.is_empty()||nodes.len()>500||edges.len()!=nodes.len()-1{return Err(fail());}
 let mut paths=Vec::<Vec<u8>>::new();let mut sample_total=0usize;
 for (i,n)in nodes.iter().enumerate(){
  let path=decode_path(get(n,"path_hex")).ok_or_else(fail)?;
  if integer(get(n,"id"))!=Some(i)||!matches!(get(n,"path"),Str(p) if p==&String::from_utf8_lossy(&path))||!coverage(get(n,"coverage"))||!valid_identity(get(n,"identity"))||!["Directory","File","Symlink","Other","Unknown"].contains(&text(n,"kind").as_str())||!texts(get(n,"notes"),64,512)||!texts(get(n,"format_candidates"),16,128){return Err(fail());}
  if i==0{if path!=selected||text(n,"kind")!="Directory"||get(n,"identity")==&Null{return Err(fail());}}else{if path<=paths[i-1]||(!selected.is_empty()&&!(path.starts_with(&selected)&&path.get(selected.len())==Some(&b'/'))){return Err(fail());}}
  if text(g,"coverage")=="Complete"&&text(n,"coverage")!="Complete"{return Err(fail());}if text(n,"coverage")=="Changed"&&text(g,"coverage")!="Changed"{return Err(fail());}
  let p=get(n,"sample");if p!=&Null{
   let Some(offset)=integer(get(p,"offset"))else{return Err(fail());};let Some(length)=integer(get(p,"length"))else{return Err(fail());};let hash=text(p,"sha256");let size=if let Num(v)=get(get(n,"identity"),"size"){v.parse::<u64>().ok()}else{None};
   if text(n,"kind")!="File"||text(n,"coverage")=="Changed"||offset!=0||length>4096||!matches!(get(p,"complete_file"),Bool(_))||hash.len()!=64||!raw_path(get(p,"sha256"))||text(p,"method")!="SHA-256 of exactly the captured sample bytes; a prefix digest is not whole-file identity"||size.is_none()||length as u64>size.unwrap_or(0)||boolean(p,"complete_file")!=(size==Some(length as u64))||(!boolean(p,"complete_file")&&text(n,"coverage")!="Partial"){return Err(fail());}
   sample_total=sample_total.checked_add(length).ok_or_else(fail)?;
  }else if !array(n,"format_candidates").is_empty(){return Err(fail());}
  paths.push(path);
 }
 if sample_total!=sampled{return Err(fail());}
 let mut children=std::collections::BTreeSet::new();for e in edges{
  let Some(p)=integer(get(e,"parent"))else{return Err(fail());};let Some(c)=integer(get(e,"child"))else{return Err(fail());};
  if text(e,"relation")!="contains"||p>=c||c>=nodes.len()||!children.insert(c)||text(&nodes[p],"kind")!="Directory"{return Err(fail());}
  let child=&paths[c];let parent=child.iter().rposition(|b|*b==b'/').map(|i|&child[..i]).unwrap_or(&[]);if parent!=paths[p]{return Err(fail());}
 }
 if children.len()!=nodes.len()-1{return Err(fail());}Ok(())
}
/// Snapshot namespace and selected sources are bound to the authenticated report,
/// not merely internally consistent fields inside a saved graph.
pub(super) fn validate_record(graphs:&Json,record:&Json)->Result<()>{
 let fail=||"Malformed typed observation provenance; textual record remains available".to_string();
 let Arr(items)=graphs else{return Err(fail());};let expected=if text(record,"tool")=="compare"{2}else{1};if items.len()!=expected{return Err(fail());}
 for (i,g)in items.iter().enumerate(){
  validate(g)?;if text(g,"kind")=="web"{web_evidence::validate_record(g,record)?;continue;}if text(g,"kind")=="complete_file"&&text(get(record,"args"),"content")!="complete_digest"{return Err(fail());}let identity=get(g,"workspace_identity");let namespace=format!("{}:{}",get(identity,"dev").string(),get(identity,"ino").string());if namespace!=text(record,"workspace_identity"){return Err(fail());}
  let source=text(get(record,"args"),if i==0{"path"}else{"other"});let mut normalized=std::path::PathBuf::new();for component in std::path::Path::new(&source).components(){match component{std::path::Component::Normal(part)=>normalized.push(part),std::path::Component::CurDir=>{},_=>return Err(fail())}}
  use std::os::unix::ffi::OsStrExt;if decode_path(get(g,"selected_hex")).as_deref()!=Some(normalized.as_os_str().as_bytes()){return Err(fail());}
 }Ok(())
}
pub(super) fn envelope(display:String,snapshots:Result<Vec<Json>>,budget:usize)->String{
 let (graphs,note)=match snapshots{Ok(v)=>(Arr(v),String::new()),Err(e)=>(Null,e)};
 let mut v=obj(vec![("observation_format",Num("1".into())),("display",s(&display)),("snapshots",graphs),("graph_note",s(&note))]);
 if super::super::mcp_schema::state_bytes(&v)>budget.min(1024*1024){put(&mut v,"snapshots",Null);put(&mut v,"graph_note",s("Typed snapshot exceeds this operation's response allowance; textual observation remains available"));}if super::super::mcp_schema::state_bytes(&v)>budget.min(1024*1024){return display;}v.string()
}
pub(super) fn unpack(output:&str)->(String,Json,String){
 match parse(output){Ok(v)if get(&v,"observation_format")==&Num("1".into())&&matches!(get(&v,"display"),Str(_))=>{
  let graphs=get(&v,"snapshots").clone();if let Arr(items)=&graphs{if items.is_empty()||items.len()>2||items.iter().any(|g|validate(g).is_err()){return(text(&v,"display"),Null,"Typed observations were invalid; textual result retained".into());}}
  else if graphs!=Null{return(text(&v,"display"),Null,"Typed observations were invalid; textual result retained".into());}
  (text(&v,"display"),graphs,text(&v,"graph_note"))
 },_=>(output.into(),Null,String::new())}
}
pub(super) fn query(graphs:&Json,args:&Json)->Result<Json>{
 let Arr(items)=graphs else{return Err("This retained report has no typed directory observations".into());};
 if items.is_empty()||items.len()>2{return Err("Malformed typed snapshot set".into());}let side=integer(get(args,"snapshot")).unwrap_or(0);let g=items.get(side).ok_or("Snapshot index is unavailable")?;validate(g)?;let nodes=array(g,"nodes");
 let parent=integer(get(args,"children"));if parent.is_some_and(|p|p>=nodes.len()){return Err("Parent node is unavailable".into());}
 let selected=integer(get(args,"node"));let path=text(args,"path_hex");let offset=integer(get(args,"offset")).unwrap_or(0);
 let indices:Vec<usize>=if let Some(n)=selected{if n>=nodes.len(){return Err("Node is unavailable".into());}vec![n]}else if get(args,"path_hex")!=&Null{nodes.iter().enumerate().filter(|(_,n)|text(n,"path_hex")==path).map(|(i,_)|i).collect()}else if let Some(p)=parent{array(g,"edges").iter().filter(|e|integer(get(e,"parent"))==Some(p)).filter_map(|e|integer(get(e,"child"))).collect()}else{(0..nodes.len()).collect()};
 let chosen:Vec<usize>=indices.iter().skip(offset).take(12).copied().collect();
 let mut out=obj(vec![("kind",get(g,"kind").clone()),("source",get(g,"source").clone()),("snapshot",Num(side.to_string())),("snapshot_count",Num(items.len().to_string())),("coverage",get(g,"coverage").clone()),("selected_hex",get(g,"selected_hex").clone()),("matching",Num(indices.len().to_string())),("offset",Num(offset.to_string())),("next_offset",if offset.saturating_add(12)<indices.len(){Num((offset+12).to_string())}else{Null}),("nodes",Arr(chosen.iter().map(|i|nodes[*i].clone()).collect())),("edges",Arr(array(g,"edges").into_iter().filter(|e|integer(get(e,"child")).is_some_and(|c|chosen.contains(&c))).collect()))]);put(&mut out,"historical",Bool(true));Ok(out)
}

#[cfg(test)] mod graph_tests{
 use super::*;
 #[test]fn escaped_envelope_budget_preserves_raw_result(){let text="\\".repeat(1000);assert_eq!(envelope(text.clone(),Err("large".into()),1100),text);}
 #[test]fn missing_graph_keeps_display(){let wire=envelope("actual observed text".into(),Err("bounded".into()),65536);let(display,graphs,note)=unpack(&wire);assert_eq!(display,"actual observed text");assert_eq!(graphs,Null);assert_eq!(note,"bounded");}
}
