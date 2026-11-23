//! Complete held-file observations and streaming reproduction of observed bytes.
use super::*;
use super::super::js_evidence;
use crate::host_fs::{MetadataExt,OpenOptionsExt};
use std::io::{Seek,SeekFrom};
use std::time::Instant;
#[path="clean_room.rs"] mod clean_room;
const CHUNK:usize=65536;
const METHOD:&str="SHA-256 of every byte read from the complete held regular file";
fn u64_value(v:&Json)->Result<u64>{match v{Num(s)=>s.parse().map_err(|_|"Invalid complete-file byte count".into()),_=>Err("Complete-file byte count must be an integer".into())}}
fn hex(v:&[u8])->String{v.iter().map(|b|format!("{b:02x}")).collect()}
pub(super) fn normalize(value:&str)->Result<Vec<u8>>{let mut p=PathBuf::new();for c in Path::new(value).components(){match c{std::path::Component::Normal(n)=>p.push(n),std::path::Component::CurDir=>{},_=>return Err("Selected file must stay within the workspace".into())}}let raw=p.as_os_str().as_encoded_bytes().to_vec();reconstruction::raw(&hex(&raw))?;Ok(raw)}
struct Guard<'a>{access:&'a operation::InspectionAccess,policy:String,tool:String,args:Json}
impl<'a> Guard<'a>{
 fn new(tool:&str,args:&Json,access:&'a operation::InspectionAccess)->Result<Self>{let p=harness_rules::gateway_current(&Null)?;harness_rules::gateway_check(&p,tool,args)?;Ok(Self{access,policy:harness_rules::digest(&p),tool:tool.into(),args:args.clone()})}
 fn check(&self)->Result<()>{if self.access.cancelled(){return Err("Complete-file operation cancelled; no automatic replay".into());}if Instant::now()>=self.access.deadline{return Err("Complete-file operation deadline expired; no completed evidence".into());}let p=harness_rules::gateway_current(&Null)?;if harness_rules::digest(&p)!=self.policy{return Err("Current rules changed during the complete-file operation".into());}harness_rules::gateway_check(&p,&self.tool,&self.args)?;harness_rules::gateway_check(&p,"read",&obj(vec![("path",get(&self.args,"path").clone())]))?;if self.tool=="compare"{harness_rules::gateway_check(&p,"read",&obj(vec![("path",get(&self.args,"other").clone())]))?;}if self.tool=="reconstruct"{harness_rules::gateway_check(&p,"write",&obj(vec![("path",get(&self.args,"output").clone())]))?;}Ok(())}
}
fn open_at(parent:&fs::File,name:&[u8],directory:bool)->Result<fs::File>{let flags=0x80000|0x20000|0x800|if directory{0x10000}else{0};crate::host_fs::open_at(parent,name,flags).map_err(|e|format!("Cannot open held selected file: {e}"))}
struct Held{root_path:PathBuf,root_identity:Json,components:Vec<Vec<u8>>,bindings:Vec<(u64,u64)>,handles:Vec<fs::File>,path:Vec<u8>,identity:Json}
impl Held{
 fn open(root:&Path,path:Vec<u8>,access:&operation::InspectionAccess)->Result<Self>{
  let r=fs::OpenOptions::new().read(true).custom_flags(0x10000|0x20000).open(root).map_err(|e|e.to_string())?;
  let m=r.metadata().map_err(|e|e.to_string())?;
  if access.root_identity.is_some_and(|(d,i)|d!=m.dev()||i!=m.ino()){return Err("Selected workspace identity changed".into());}
  let root_identity=s(&format!("{}:{}",m.dev(),m.ino()));
  let components:Vec<Vec<u8>>=path.split(|b|*b==b'/').map(|p|p.to_vec()).collect();
  if components.is_empty()||components.len()>512{return Err("Selected file path exceeds the 1024-byte workspace path allowance".into());}
  // Keep root and selected file open. Intermediate identities are re-walked
  // from the held root; path depth cannot exhaust one descriptor per segment.
  let mut parent=r.try_clone().map_err(|e|e.to_string())?;let mut bindings=vec![];
  for(i,c)in components.iter().enumerate(){let current=open_at(&parent,c,i+1<components.len())?;let metadata=current.metadata().map_err(|e|e.to_string())?;bindings.push((metadata.dev(),metadata.ino()));parent=current;}
  let metadata=parent.metadata().map_err(|e|e.to_string())?;
  if !metadata.is_file(){return Err("File observation requires a selected regular file".into());}
  let identity=reconstruction::identity(&metadata);
  let out=Self{root_path:root.into(),root_identity,components,bindings,handles:vec![r,parent],path,identity};out.binding()?;Ok(out)
 }
 fn binding(&self)->Result<()>{
  reconstruction::same_root(&self.root_path,&self.handles[0],&self.root_identity)?;
  let mut parent=self.handles[0].try_clone().map_err(|e|e.to_string())?;
  for(i,c)in self.components.iter().enumerate(){let current=open_at(&parent,c,i+1<self.components.len())?;let m=current.metadata().map_err(|e|e.to_string())?;if self.bindings[i]!=(m.dev(),m.ino()){return Err("Selected file component binding changed".into());}parent=current;}
  if reconstruction::identity(&parent.metadata().map_err(|e|e.to_string())?)!=self.identity||reconstruction::identity(&self.handles.last().unwrap().metadata().map_err(|e|e.to_string())?)!=self.identity{return Err("Selected file changed during observation".into());}
  reconstruction::same_root(&self.root_path,&self.handles[0],&self.root_identity)
 }
 fn rewind(&mut self)->Result<()>{self.binding()?;self.handles.last_mut().unwrap().seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;Ok(())}
 fn scan(&mut self,g:&Guard,mut consume:impl FnMut(&[u8])->Result<()>)->Result<(u64,String)>{let expected=u64_value(get(&self.identity,"size"))?;let mut count=0u64;let mut digest=incremental_digest::Digest::new();let mut buffer=[0u8;CHUNK];loop{g.check()?;self.binding()?;let n=self.handles.last_mut().unwrap().read(&mut buffer).map_err(|e|e.to_string())?;g.check()?;self.binding()?;if n==0{break;}count=count.checked_add(n as u64).ok_or("Complete-file byte count overflow")?;if count>expected{return Err("Selected file grew during observation".into());}digest.update(&buffer[..n])?;consume(&buffer[..n])?;g.check()?;}if count!=expected{return Err("Selected file ended before its observed length".into());}self.binding()?;g.check()?;Ok((count,digest.finish()))}
}
/// Ordinary observations share the same held component chain as full digests.
/// Nonblocking opens reject special files before any read can wait for a peer.
pub(super) struct Source<'a>{held:Held,guard:Guard<'a>}
impl<'a> Source<'a>{
 pub(super) fn open(root:&Path,relative:&str,tool:&str,args:&Json,access:&'a operation::InspectionAccess)->Result<Self>{
  if !access.read{return Err("File observation requires read permission".into());}
  let guard=Guard::new(tool,args,access)?;guard.check()?;
  let held=Held::open(root,normalize(relative)?,access)?;
  let out=Self{held,guard};out.check()?;Ok(out)
 }
 pub(super) fn file(&self)->Result<fs::File>{self.check()?;self.held.handles.last().unwrap().try_clone().map_err(|e|e.to_string())}
 pub(super) fn check(&self)->Result<()>{self.guard.check()?;self.held.binding()}
}
pub(super) fn validate_mode(args:&Json)->Result<()>{if get(args,"content")!=&Null&&text(args,"content")!="complete_digest"{return Err("Inspection content mode must be complete_digest when selected".into());}Ok(())}
fn observe(root:&Path,args:&Json,access:&operation::InspectionAccess,tool:&str,selected:&str)->Result<(String,Json)>{if !access.read{return Err("Complete-file observation requires current read permission".into());}if access.budget<CHUNK+32768{return Err("Current response allowance cannot admit a 64 KiB complete-file buffer".into());}let g=Guard::new(tool,args,access)?;g.check()?;let mut held=Held::open(root,normalize(selected)?,access)?;let(n,digest)=held.scan(&g,|_|Ok(()))?;
 let node=obj(vec![("id",Num("0".into())),("path_hex",s(&hex(&held.path))),("path",s(&String::from_utf8_lossy(&held.path))),("kind",s("File")),("identity",held.identity.clone()),("coverage",s("Complete")),("sample",obj(vec![("offset",Num("0".into())),("length",Num(n.to_string())),("complete_file",Bool(true)),("sha256",s(&digest)),("method",s(METHOD))])),("format_candidates",Arr(vec![])),("notes",Arr(vec![s("Every byte was read; content bytes were not retained. This is not an atomic filesystem snapshot.")]))]);
 let graph=obj(vec![("schema",Num("1".into())),("kind",s("complete_file")),("selected_hex",s(&hex(&held.path))),("workspace_identity",reconstruction::identity(&held.handles[0].metadata().map_err(|e|e.to_string())?)),("coverage",s("Complete")),("sampled_bytes",Num(n.to_string())),("bytes_read",Num(n.to_string())),("nodes",Arr(vec![node])),("edges",Arr(vec![])),("notes",Arr(vec![]))]);validate_graph(&graph)?;g.check()?;Ok((format!("Complete selected-file observation: {}. Read all {n} bytes in 64 KiB chunks; SHA-256 {digest}. Held source/component identities and name bindings remained stable across the scan. No content bytes retained; no atomic snapshot or behavior claim.",selected),graph))}
pub(super) fn inspect(root:&Path,args:&Json,access:&operation::InspectionAccess)->Result<String>{let(display,g)=observe(root,args,access,"inspect",&text(args,"path"))?;Ok(if access.graph{observation_graph::envelope(display,Ok(vec![g]),access.budget)}else{display})}
/// Read one selected file through the same held scan as a complete-file observation.
/// Bytes above the archive reader limit are not parsed as a capture.
pub(super) fn saved_file_capture(root:&Path,relative:&str,access:&operation::InspectionAccess)->Result<Json>{
    if !access.read{return Err("Complete-file observation requires current read permission".into());}
    if access.budget<CHUNK+32768{return Err("Current response allowance cannot admit a 64 KiB complete-file buffer".into());}
    let args=obj(vec![("path",s(relative))]);
    let g=Guard::new("inspect",&args,access)?;
    g.check()?;
    let mut held=Held::open(root,normalize(relative)?,access)?;
    let expected=u64_value(get(&held.identity,"size"))?;
    if expected>super::super::archive::UNPACK_BYTE_LIMIT as u64{
        g.check()?;
        return Ok(super::super::har_evidence::file_limit());
    }
    let mut kept=Vec::new();
    let (n,_digest)=held.scan(&g,|chunk|{kept.extend_from_slice(chunk);Ok(())})?;
    if n!=expected||kept.len() as u64!=n{return Err("Selected file changed during saved-capture observation".into());}
    g.check()?;
    Ok(super::super::har_evidence::file_capture_scoped(&kept,true,access.cancellation_token))
}
/// JavaScript and ASAR bytes use the same reader limit as a saved capture.
/// A file above the limit is a named refusal. The bytes are not parsed.
pub(super) fn javascript_observation(bytes:&[u8],file_len:u64,complete:bool,kind:js_evidence::Kind,module_label:&str,map_file:&dyn Fn(&str)->Option<js_evidence::MapFile>)->String{
    if file_len>super::super::archive::UNPACK_BYTE_LIMIT as u64||bytes.len()>super::super::archive::UNPACK_BYTE_LIMIT{
        return "JavaScript/Electron observation\nRefusal: javascript file exceeds the reader limit. No listing returned.\n".to_string();
    }
    js_evidence::observe_with(bytes,kind,complete,module_label,map_file).text
}
pub(super) fn compare(root:&Path,args:&Json,access:&operation::InspectionAccess)->Result<String>{let(a,ga)=observe(root,args,access,"compare",&text(args,"path"))?;let(b,gb)=observe(root,args,access,"compare",&text(args,"other"))?;let left=&array(&ga,"nodes")[0];let right=&array(&gb,"nodes")[0];let equal=get(get(left,"sample"),"length")==get(get(right,"sample"),"length")&&get(get(left,"sample"),"sha256")==get(get(right,"sample"),"sha256");let display=format!("Sequential independent complete-file observations, not an atomic pair. Lengths and SHA-256 {}. Digest equality is not byte-by-byte proof.\n{a}\n{b}",if equal{"match"}else{"differ"});Ok(if access.graph{observation_graph::envelope(display,Ok(vec![ga,gb]),access.budget)}else{display})}
pub(super) fn validate_graph(g:&Json)->Result<()>{let fail=||"Malformed complete-file observation; textual record remains available".to_string();if super::super::mcp_schema::state_bytes(g)>16384||get(g,"schema")!=&Num("1".into())||text(g,"kind")!="complete_file"||text(g,"coverage")!="Complete"||get(g,"workspace_identity")==&Null||!observation_graph::valid_identity(get(g,"workspace_identity"))||!matches!(get(g,"notes"),Arr(a)if a.is_empty())||!matches!(get(g,"edges"),Arr(a)if a.is_empty()){return Err(fail());}let nodes=get(g,"nodes").array()?;if nodes.len()!=1{return Err(fail());}let n=&nodes[0];let raw=reconstruction::raw(&text(n,"path_hex"))?;let sample=get(n,"sample");let len=u64_value(get(sample,"length"))?;let hash=text(sample,"sha256");if get(n,"id")!=&Num("0".into())||text(g,"selected_hex")!=text(n,"path_hex")||text(n,"path")!=String::from_utf8_lossy(&raw)||text(n,"kind")!="File"||text(n,"coverage")!="Complete"||get(n,"identity")==&Null||!observation_graph::valid_identity(get(n,"identity"))||len>u64::MAX/8||len!=u64_value(get(get(n,"identity"),"size"))?||len!=u64_value(get(g,"sampled_bytes"))?||len!=u64_value(get(g,"bytes_read"))?||get(sample,"offset")!=&Num("0".into())||get(sample,"complete_file")!=&Bool(true)||text(sample,"method")!=METHOD||hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))||!matches!(get(n,"format_candidates"),Arr(a)if a.is_empty())||!matches!(get(n,"notes"),Arr(a)if a.len()<=4&&a.iter().all(|s|matches!(s,Str(t)if t.len()<=512))){return Err(fail());}Ok(())}
pub(super) fn is_complete(node:&Json)->bool{text(get(node,"sample"),"method")==METHOD}
pub(super) fn reconstruct(root:&Path,v:&Json,access:&operation::InspectionAccess)->Result<String>{if !access.read{return Err("Streaming reconstruction requires read permission".into());}if access.budget<2*CHUNK+32768{return Err("Current response allowance cannot admit streaming copy/readback buffers".into());}let args=get(v,"args");let bound=get(v,"reconstruction_source");let node=get(bound,"node");let g=Guard::new("reconstruct",args,access)?;if g.policy!=text(bound,"policy"){return Err("Reconstruction policy changed since review".into());}g.check()?;let mut held=Held::open(root,reconstruction::raw(&text(node,"path_hex"))?,access)?;let sample=get(node,"sample");if !is_complete(node)||get(sample,"complete_file")!=&Bool(true)||get(sample,"offset")!=&Num("0".into())||held.identity!=*get(node,"identity")||u64_value(get(sample,"length"))?!=u64_value(get(&held.identity,"size"))?{return Err("Fresh source does not match the complete observed file".into());}
 let(n,hash)=held.scan(&g,|_|Ok(()))?;if hash!=text(sample,"sha256"){return Err("Fresh entire source digest differs; no output created".into());}held.rewind()?;g.check()?;held.binding()?;
 if text(args,"mode")=="clean_room"{return write_clean_room(&mut held,&g,args,bound,n,&hash);}
 let output=text(args,"output");let mut target=create_exclusive(&held.handles[0],&output)?;
 let checked=(||->Result<()>{let(written,second)=held.scan(&g,|chunk|{g.check()?;target.write_all(chunk).map_err(|e|e.to_string())?;g.check()})?;if written!=n||second!=hash{return Err("Selected source changed between verification and copy".into());}g.check()?;target.sync_all().map_err(|e|e.to_string())?;g.check()?;let before=reconstruction::identity(&target.metadata().map_err(|e|e.to_string())?);held.rewind()?;target.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;let mut buffer=[0u8;CHUNK];let(read,third)=held.scan(&g,|chunk|{g.check()?;target.read_exact(&mut buffer[..chunk.len()]).map_err(|e|e.to_string())?;g.check()?;if chunk!=&buffer[..chunk.len()]{return Err("Created file readback differs from the source".into());}Ok(())})?;let mut end=[0u8;1];if target.read(&mut end).map_err(|e|e.to_string())?!=0||read!=n||third!=hash||before!=reconstruction::identity(&target.metadata().map_err(|e|e.to_string())?){return Err("Created file changed during readback".into());}let named=open_at(&held.handles[0],output.as_bytes(),false)?;if before!=reconstruction::identity(&named.metadata().map_err(|e|e.to_string())?){return Err("Created output name binding changed".into());}held.binding()?;g.check()?;Ok(())})();checked.map_err(|e|format!("New file {output} retained but streaming reconstruction unconfirmed: {e}; no automatic replay"))?;
 Ok(format!("Streaming reconstruction created new file {output}; all {n} bytes read back exactly in 64 KiB chunks. SHA-256 {hash}. Complete source record {}, snapshot {}, node {}, raw path {}. Entire fresh source was verified before creation and again across copy/readback. This reproduces observed bytes, not inferred behavior.",text(bound,"record"),get(bound,"snapshot").string(),get(node,"id").string(),text(node,"path_hex")))}
fn shown_number(v:&Json)->String{match v{Num(n)|Str(n)=>n.clone(),other=>other.string()}}
fn create_exclusive(root:&fs::File,output:&str)->Result<fs::File>{
 crate::host_fs::open_at(root,output.as_bytes(),0x80000|0x20000|0x80|0x40|2).map_err(|e|format!("Cannot create new streaming output: {e}"))
}
fn write_clean_room(held:&mut Held,g:&Guard,args:&Json,bound:&Json,n:u64,hash:&str)->Result<String>{
 if n>clean_room::SUBJECT_BYTE_CAP as u64{return Err(format!("Refusal: {}. {n} exceeds {}. No output was written.",clean_room::SUBJECT_BYTE_NAME,clean_room::SUBJECT_BYTE_CAP));}
 let mut retained=Vec::new();
 let(read,second)=held.scan(g,|chunk|{g.check()?;if retained.len().saturating_add(chunk.len())>clean_room::SUBJECT_BYTE_CAP{return Err(format!("Refusal: {}. {} exceeds {}. No output was written.",clean_room::SUBJECT_BYTE_NAME,n,clean_room::SUBJECT_BYTE_CAP));}retained.extend_from_slice(chunk);Ok(())})?;
 if read!=n||second!=hash{return Err("Selected source changed before clean-room generation; no output created".into());}
 g.check()?;held.binding()?;
 let path=String::from_utf8(held.path.clone()).map_err(|_|String::from("Refusal: clean-room subject path is not UTF-8. No output was written."))?;
 let identity=&held.identity;
 let citation=clean_room::Citation{path:path.clone(),dev:shown_number(get(identity,"dev")),ino:shown_number(get(identity,"ino")),size:shown_number(get(identity,"size")),sha256:hash.to_string(),record:text(bound,"record"),snapshot:shown_number(get(bound,"snapshot")),node:shown_number(get(get(bound,"node"),"id")),path_hex:text(get(bound,"node"),"path_hex")};
 let outcome=clean_room::rebuild(&retained,&path,&citation)?;
 let output=text(args,"output");
 let generated=outcome.generated.as_bytes();
 let memory_items=outcome.items;let memory_matched=outcome.matched;let memory_missing=outcome.missing;let memory_unknown=outcome.unknown;let memory_lines=outcome.lines.clone();
 let mut target=create_exclusive(&held.handles[0],&output)?;
 let checked=(||->Result<clean_room::Outcome>{
  let mut written=0usize;
  while written<generated.len(){g.check()?;let end=(written+CHUNK).min(generated.len());target.write_all(&generated[written..end]).map_err(|e|e.to_string())?;written=end;g.check()?;}
  g.check()?;target.sync_all().map_err(|e|e.to_string())?;g.check()?;
  let before=reconstruction::identity(&target.metadata().map_err(|e|e.to_string())?);
  target.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;
  let mut buffer=[0u8;CHUNK];let mut disk=Vec::with_capacity(generated.len());
  while disk.len()<generated.len(){g.check()?;let end=(disk.len()+CHUNK).min(generated.len());let expect=&generated[disk.len()..end];target.read_exact(&mut buffer[..expect.len()]).map_err(|e|e.to_string())?;g.check()?;if expect!=&buffer[..expect.len()]{return Err("Created file readback differs from the generated clean-room bytes".into());}disk.extend_from_slice(&buffer[..expect.len()]);}
  let mut end=[0u8;1];
  if target.read(&mut end).map_err(|e|e.to_string())?!=0||disk.len()!=generated.len()||before!=reconstruction::identity(&target.metadata().map_err(|e|e.to_string())?){return Err("Created file changed during readback".into());}
  let named=open_at(&held.handles[0],output.as_bytes(),false)?;
  if before!=reconstruction::identity(&named.metadata().map_err(|e|e.to_string())?){return Err("Created output name binding changed".into());}
  held.binding()?;g.check()?;
  let observed=clean_room::verify_artifact(&retained,&path,&disk,&citation)?;
  if observed.items!=memory_items||observed.matched!=memory_matched||observed.missing!=memory_missing||observed.unknown!=memory_unknown||observed.lines!=memory_lines{return Err("disk observation does not match the generated buffer".into());}
  Ok(observed)
 })();
 let observed=checked.map_err(|e|format!("New file {output} retained but clean-room reconstruction unconfirmed: {e}; no automatic replay"))?;
 let out_hash=harness_rules::sha256(observed.generated.as_bytes());
 Ok(format!("Clean-room reconstruction created new file {output}; {} generated bytes read back exactly. Output SHA-256: {out_hash}. Reference SHA-256 {hash}. Spec items: {}. Generated file: {output}.\n{}Verification matched {}, missing {}, unknown {} on the {} bytes read back from disk. Known: complete source identity dev {} ino {} size {} matched the held file before generation. Unknown: the new file is a generated skeleton from the observer specification, not a copy of the reference body. Record {}, snapshot {}, node {}, raw path {}.",generated.len(),observed.items,observed.lines,observed.matched,observed.missing,observed.unknown,observed.generated.len(),citation.dev,citation.ino,citation.size,citation.record,citation.snapshot,citation.node,citation.path_hex))
}
#[cfg(test)]mod checks{use super::*;
 #[test]fn deadline_precedes_reads_and_is_not_reset(){let access=operation::InspectionAccess{list:true,read:true,graph:false,budget:1024*1024,root_identity:None,cancellation_token:super::super::super::execution::cancellation_token(),deadline:Instant::now()-std::time::Duration::from_secs(1)};let args=obj(vec![("path",s("missing")),("content",s("complete_digest"))]);let g=Guard::new("inspect",&args,&access).unwrap();assert!(g.check().unwrap_err().contains("deadline"));assert!(inspect(Path::new("/unopened"),&args,&access).unwrap_err().contains("deadline"));}
 #[test]fn complete_graph_declared_length_and_shape_are_checked(){let bad=obj(vec![("schema",Num("1".into())),("kind",s("complete_file"))]);assert!(validate_graph(&bad).is_err());assert!(validate_mode(&obj(vec![("content",s("all_apps"))])).is_err());assert!(normalize("../other").is_err());assert!(normalize("/other").is_err());assert!(normalize("./safe/file").is_ok());}
 #[test]fn clean_room_route_cites_generates_and_verifies(){
  let dir=std::env::temp_dir().join(format!("rllm-clean-room-{}",std::process::id()));
  let _=fs::remove_dir_all(&dir);fs::create_dir_all(&dir).unwrap();let root=dir.canonicalize().unwrap();
  struct DropDir(std::path::PathBuf);impl Drop for DropDir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
  let _drop=DropDir(dir.clone());
  let source="const {app} = require(\"electron\");\nconst helper = require(\"./helper.js\");\nipcMain.on(\"window:open\", function () {});\nrouter.get(\"/status\", function () {});\nexport function openWindow() {}\n";
  fs::write(root.join("feature.js"),source).unwrap();
  let file_meta=fs::metadata(root.join("feature.js")).unwrap();let root_meta=fs::metadata(&root).unwrap();
  let bytes=fs::read(root.join("feature.js")).unwrap();let hash=harness_rules::sha256(&bytes);let path_hex=hex(b"feature.js");
  let node=obj(vec![("id",Num("0".into())),("path_hex",s(&path_hex)),("path",s("feature.js")),("kind",s("File")),("identity",reconstruction::identity(&file_meta)),("coverage",s("Complete")),("sample",obj(vec![("offset",Num("0".into())),("length",Num(file_meta.len().to_string())),("complete_file",Bool(true)),("sha256",s(&hash)),("method",s(METHOD))])),("format_candidates",Arr(vec![])),("notes",Arr(vec![]))]);
  let policy=harness_rules::digest(&harness_rules::gateway_current(&Null).unwrap());
  let workspace=root.to_string_lossy().to_string();
  let bound=obj(vec![("schema",Num("1".into())),("record",s("rec-clean")),("snapshot",Num("0".into())),("node",node),("workspace",s(&workspace)),("workspace_identity",s(&format!("{}:{}",root_meta.dev(),root_meta.ino()))),("coverage",s("Complete")),("policy",s(&policy))]);
  let args=obj(vec![("record",s("rec-clean")),("snapshot",Num("0".into())),("node",Num("0".into())),("mode",s("clean_room")),("output",s("feature-rebuild.js")),("path",s("feature.js")),("source_tool",s("read")),("source_args",obj(vec![("path",s("feature.js"))]))]);
  super::super::super::execution::reset();
  let access=operation::InspectionAccess{list:true,read:true,graph:false,budget:2*1024*1024,root_identity:Some((root_meta.dev(),root_meta.ino())),cancellation_token:super::super::super::execution::cancellation_token(),deadline:Instant::now()+std::time::Duration::from_secs(30)};
  let response=reconstruction::run(&root,&obj(vec![("args",args),("reconstruction_source",bound)]),&access).unwrap_or_else(|e|panic!("{e}"));
  assert!(response.contains("Spec items: 5"),"{response}");
  assert!(response.contains("Generated file: feature-rebuild.js"),"{response}");
  assert!(response.contains("Verification matched 5, missing 0, unknown 0"),"{response}");
  assert!(response.contains("matched edge require electron"),"{response}");
  assert!(response.contains("matched site ipcMain.on window:open"),"{response}");
  assert!(response.contains("matched site router.get /status"),"{response}");
  assert!(response.contains("known:"),"{response}");
  assert!(response.contains("unknown:"),"{response}");
  assert!(response.contains(&hash),"{response}");
  let generated=fs::read(root.join("feature-rebuild.js")).unwrap();
  assert_ne!(generated,bytes);
 }
 struct HeldRoot(std::path::PathBuf);
 impl Drop for HeldRoot{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn drive_clean_room(label:&str,file_name:&str,source:&[u8],output:&str)->(HeldRoot,std::path::PathBuf,Result<String>){
  let dir=std::env::temp_dir().join(format!("rllm-clean-room-{label}-{}",std::process::id()));
  let _=fs::remove_dir_all(&dir);fs::create_dir_all(&dir).unwrap();let root=dir.canonicalize().unwrap();let held=HeldRoot(dir);
  fs::write(root.join(file_name),source).unwrap();
  let file_meta=fs::metadata(root.join(file_name)).unwrap();let root_meta=fs::metadata(&root).unwrap();
  let bytes=fs::read(root.join(file_name)).unwrap();let hash=harness_rules::sha256(&bytes);let path_hex=hex(file_name.as_bytes());
  let node=obj(vec![("id",Num("0".into())),("path_hex",s(&path_hex)),("path",s(file_name)),("kind",s("File")),("identity",reconstruction::identity(&file_meta)),("coverage",s("Complete")),("sample",obj(vec![("offset",Num("0".into())),("length",Num(file_meta.len().to_string())),("complete_file",Bool(true)),("sha256",s(&hash)),("method",s(METHOD))])),("format_candidates",Arr(vec![])),("notes",Arr(vec![]))]);
  let policy=harness_rules::digest(&harness_rules::gateway_current(&Null).unwrap());
  let workspace=root.to_string_lossy().to_string();
  let bound=obj(vec![("schema",Num("1".into())),("record",s("rec-clean")),("snapshot",Num("0".into())),("node",node),("workspace",s(&workspace)),("workspace_identity",s(&format!("{}:{}",root_meta.dev(),root_meta.ino()))),("coverage",s("Complete")),("policy",s(&policy))]);
  let args=obj(vec![("record",s("rec-clean")),("snapshot",Num("0".into())),("node",Num("0".into())),("mode",s("clean_room")),("output",s(output)),("path",s(file_name)),("source_tool",s("read")),("source_args",obj(vec![("path",s(file_name))]))]);
  super::super::super::execution::reset();
  let access=operation::InspectionAccess{list:true,read:true,graph:false,budget:2*1024*1024,root_identity:Some((root_meta.dev(),root_meta.ino())),cancellation_token:super::super::super::execution::cancellation_token(),deadline:Instant::now()+std::time::Duration::from_secs(30)};
  let result=reconstruction::run(&root,&obj(vec![("args",args),("reconstruction_source",bound)]),&access);
  (held,root,result)
 }
 #[test]fn clean_room_route_refuses_its_own_skeleton_as_a_copy(){
  let citation=clean_room::Citation{path:"package.json".into(),dev:"1".into(),ino:"2".into(),size:"3".into(),sha256:"ab".repeat(32),record:"rec".into(),snapshot:"0".into(),node:"0".into(),path_hex:"70".into()};
  let skeleton=clean_room::rebuild(b"{\"name\":\"desk\",\"version\":\"1.0.0\",\"main\":\"main.js\"}\n","package.json",&citation).unwrap().generated;
  let (held,root,result)=drive_clean_room("copy","package.json",skeleton.as_bytes(),"package-rebuild.json");
  let error=result.unwrap_err();
  assert!(error.contains("clean-room copy"),"{error}");
  assert!(error.contains("No output was written."),"{error}");
  assert!(!error.contains("Verification matched"),"{error}");
  assert!(!root.join("package-rebuild.json").exists(),"wrote {}",root.join("package-rebuild.json").display());
  drop(held);
 }
 #[test]fn clean_room_route_refuses_an_over_cap_observation_without_writing(){
  let mut source=String::new();
  for index in 0..=clean_room::SPEC_ITEM_CAP{source.push_str(&format!("require(\"m{index}\");\n"));}
  let (held,root,result)=drive_clean_room("cap","feature.js",source.as_bytes(),"feature-rebuild.js");
  let error=result.unwrap_err();
  assert!(error.contains("clean-room item cap"),"{error}");
  assert!(error.contains("No output was written."),"{error}");
  assert!(!error.contains("Verification matched"),"{error}");
  assert!(!root.join("feature-rebuild.js").exists(),"wrote {}",root.join("feature-rebuild.js").display());
  drop(held);
 }
 #[test]fn clean_room_route_reports_counts_from_the_bytes_read_back(){
  let source="const {app} = require(\"electron\");\nconst helper = require(\"./helper.js\");\nipcMain.on(\"window:open\", function () {});\nrouter.get(\"/status\", function () {});\nexport function openWindow() {}\n";
  let (held,root,result)=drive_clean_room("disk","feature.js",source.as_bytes(),"feature-rebuild.js");
  let response=result.unwrap_or_else(|error|panic!("{error}"));
  let generated=fs::read(root.join("feature-rebuild.js")).unwrap();
  assert!(response.contains(&format!("Verification matched 5, missing 0, unknown 0 on the {} bytes read back from disk",generated.len())),"{response}");
  assert!(response.contains("matched edge require electron"),"{response}");
  drop(held);
 }
 #[test]fn javascript_reader_limit_names_the_refusal_and_returns_no_listing(){
  let bytes=b"require(\"kept-specifier\");\n";
  let over=javascript_observation(bytes,u64::MAX,true,super::super::super::js_evidence::Kind::Script,"note.js",&|_|None);
  assert!(over.contains("Refusal: javascript file exceeds the reader limit. No listing returned."),"{over}");
  assert!(!over.contains("kept-specifier"),"{over}");
  assert!(!over.contains("Kind: javascript module"),"{over}");
  let kept=javascript_observation(bytes,bytes.len() as u64,true,super::super::super::js_evidence::Kind::Script,"note.js",&|_|None);
  assert!(kept.contains("Kind: javascript module"),"{kept}");
  assert!(kept.contains("kept-specifier"),"{kept}");
  assert!(kept.contains("specifier \"kept-specifier\" bare."),"{kept}");
 }
}
#[cfg(any(test, rllm_module_tests))]
mod scoped_capture_checks {
    use super::*;
    #[cfg(rllm_module_tests)]
    #[no_mangle]
    pub extern "C" fn g7_file_evidence_scoped_capture_test() {
        public_inspection_is_not_refused_by_unrelated_global_cancellation();
    }
    #[cfg_attr(not(rllm_module_tests), test)]
    fn public_inspection_is_not_refused_by_unrelated_global_cancellation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                super::super::super::execution::reset();
            }
        }
        let _reset = Reset;
        super::super::super::execution::reset();
        let dir = std::env::temp_dir().join(format!("rllm-saved-scope-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        struct DropDir(std::path::PathBuf);
        impl Drop for DropDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _drop = DropDir(dir.clone());
        let root = dir.canonicalize().unwrap();
        let raw = br#"{"log":{"entries":[{"request":{"method":"GET","url":"https://e.test/saved-scope","headers":[{"name":"Content-Type","value":"text/plain"}]},"response":{"status":200,"content":{"mimeType":"text/plain","size":1},"bodySize":1}}]}}"#;
        fs::write(root.join("saved-scope.json"), raw).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let scope = super::super::super::execution::CancellationScope::new(flag.clone()).expect("scoped inspection");
        let token = scope.token();
        super::super::super::execution::cancel();
        assert!(super::super::super::execution::cancelled());
        assert!(!super::super::super::execution::token_cancelled(token));
        let access = operation::InspectionAccess {
            list: true,
            read: true,
            graph: false,
            budget: 2 * 1024 * 1024,
            root_identity: None,
            cancellation_token: token,
            deadline: Instant::now() + std::time::Duration::from_secs(30),
        };
        let view = saved_file_capture(&root, "saved-scope.json", &access).unwrap_or_else(|error| panic!("{error}"));
        let text = view.string();
        assert!(text.contains("https://e.test/saved-scope"), "{text}");
        assert!(!text.contains("partial"), "{text}");
        assert!(!text.contains("cancelled"), "{text}");
        assert!(!text.contains("unusable"), "{text}");
    }
}
