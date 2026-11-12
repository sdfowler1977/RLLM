//! Source-bound retained encoding facts; interpretations never become policy.
use super::*;
use std::os::unix::fs::MetadataExt;
const CAP:usize=256*1024;
const REPORT_METHOD:&str="Retained binary inspection report UTF-8";
const IR_METHOD:&str="Retained AArch64 encoding and local IR UTF-8";
fn hex(bytes:&[u8])->String{bytes.iter().map(|b|format!("{b:02x}")).collect()}
fn integer(v:&Json)->Result<u64>{match v{Num(n)=>n.parse().map_err(|_|"Invalid binary observation integer".into()),_=>Err("Binary observation requires an integer".into())}}
fn digest(v:&str)->bool{v.len()==64&&v.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn representation(content:&str,method:&str)->Json{obj(vec![("bytes",Num(content.len().to_string())),("sha256",s(&harness_rules::sha256(content.as_bytes()))),("method",s(method)),("complete",Bool(true))])}
fn logical_path(source:&str,offset:u64,address:u64)->String{format!("{source}/encoding/{offset:016x}-{address:016x}")}
fn prefix(bits:u32,offset:u64,address:u64,block:u64,mnemonic:&str,coordinate:&str)->String{format!("AArch64 encoding word0x{bits:08x}, file offset{offset}, {coordinate}0x{address:x}, candidate block0x{block:x}, mnemonic {mnemonic}; ")}
fn payload_digest(graph:&Json)->String{let mut payload=graph.clone();if let Obj(fields)=&mut payload{fields.remove("payload_sha256");if let Some(Arr(nodes))=fields.get_mut("nodes"){if let Some(Obj(root))=nodes.first_mut(){root.remove("representation");}}}harness_rules::sha256(payload.string().as_bytes())}
fn footer(digest:&str)->String{format!("\nBinary observation payload SHA-256: {digest}.\n")}
fn coordinate(label:&str)->Result<&'static str>{match label{"declared VA "=>Ok("ELF declared VA"),"RVA "=>Ok("PE RVA"),_=>Err("Unsupported binary coordinate namespace".into())}}
fn optional_paths(graph:&mut Json,statements:&[instruction::Statement],cancelled:&dyn Fn()->bool)->Result<()>{
 if cancelled(){return Err("Binary path projection cancelled".into());}
 // Existing encoding nodes, relationships and declarations get their original
 // space first. Additional path interpretations cannot evict captured words.
 let mut room=CAP.saturating_sub(super::super::mcp_schema::state_bytes(graph)).saturating_sub(1024);
 if let Obj(fields)=graph{if let Some(Arr(nodes))=fields.get_mut("nodes"){
  for(index,node)in nodes.iter_mut().enumerate().skip(1){
   if index%16==0&&cancelled(){return Err("Binary path projection cancelled".into());}let Some(word)=statements.get(index-1)else{break;};if word.path_facts.is_empty(){continue;}
   let old=text(node,"content");let content=format!("{}{}\n",old.trim_end_matches('\n'),word.path_facts);if content.len()>2048{continue;}
   let mut replacement=node.clone();put(&mut replacement,"content",s(&content));put(&mut replacement,"representation",representation(&content,IR_METHOD));
   let extra=super::super::mcp_schema::state_bytes(&replacement).saturating_sub(super::super::mcp_schema::state_bytes(node));if extra>room{continue;}room-=extra;*node=replacement;
  }
 }}Ok(())
}
pub(super) fn snapshot(root:&Path,selected:&str,access:&operation::InspectionAccess,identity:&fs::Metadata,bytes:&[u8],complete:bool,observation:&control_flow::Observation,display:&str)->Result<(String,Json)>{
 if access.cancelled(){return Err("Binary evidence projection cancelled".into());}
 let source=file_evidence::normalize(selected)?;let path=String::from_utf8_lossy(&source).into_owned();let coord=coordinate(&observation.coordinate)?;
 let workspace=fs::metadata(root).map_err(|e|e.to_string())?;
 if !workspace.is_dir()||access.root_identity!=Some((workspace.dev(),workspace.ino())){return Err("Current binary observation workspace identity is unavailable".into());}
 if bytes.len()>4*1024*1024||bytes.len()as u64>identity.len()||complete!=(bytes.len()as u64==identity.len())||display.len()>128*1024-128||observation.statements.len()>512{return Err("Binary evidence source/report exceeds its observed bounds".into());}
 let node=obj(vec![("id",Num("0".into())),("path",s(&path)),("path_hex",s(&hex(&source))),("kind",s("BinarySource")),("coverage",s("Partial")),("identity",reconstruction::identity(identity)),("sample",obj(vec![("offset",Num("0".into())),("length",Num(bytes.len().to_string())),("complete_file",Bool(complete)),("sha256",s(&harness_rules::sha256(bytes))),("method",s("SHA-256 of exactly the held inspection sample"))])),("representation",representation(display,REPORT_METHOD)),("notes",Arr(vec![s("Source encoding and mapping observations with recorded coordinates and captured coverage.")]))]);
 let mut nodes=vec![node];let mut edges=Vec::new();let mut cost=super::super::mcp_schema::state_bytes(&nodes[0]);
 for (i,word)in observation.statements.iter().take(499).enumerate(){
  if i%16==0&&access.cancelled(){return Err("Binary evidence projection cancelled".into());}
  let name=logical_path(&path,word.offset,word.address);let content=format!("{}{}\n",prefix(word.word,word.offset,word.address,word.block,word.mnemonic,coord),word.text);
  let id=nodes.len();let encoding=obj(vec![("isa",s("AArch64")),("order",s("little")),("bits",Num(word.word.to_string())),("bytes",Num("4".into())),("file_offset",Num(word.offset.to_string())),("coordinate_kind",s(coord)),("address",Num(word.address.to_string())),("block",Num(word.block.to_string())),("mnemonic",s(word.mnemonic)),("recognized",Bool(word.mnemonic!="unclassified"))]);
  let node=obj(vec![("id",Num(id.to_string())),("path",s(&name)),("path_hex",s(&hex(name.as_bytes()))),("kind",s("Encoding")),("coverage",s("Partial")),("identity",Null),("sample",Null),("encoding",encoding),("content",s(&content)),("representation",representation(&content,IR_METHOD))]);
  let edge=obj(vec![("relation",s("contains")),("parent",Num("0".into())),("child",Num(id.to_string()))]);let extra=super::super::mcp_schema::state_bytes(&node)+super::super::mcp_schema::state_bytes(&edge);
  if cost.saturating_add(extra)>CAP-16384{break;}cost+=extra;nodes.push(node);edges.push(edge);
 }
 let retained=nodes.len()-1;let mut graph=obj(vec![("schema",Num("1".into())),("kind",s("binary")),("selected_hex",s(&hex(&source))),("workspace_identity",reconstruction::identity(&workspace)),("coverage",s("Partial")),("coordinate_kind",s(coord)),("observed_words",Num(observation.statements.len().to_string())),("retained_words",Num(retained.to_string())),("omitted_words",Num((observation.statements.len()-retained).to_string())),("sampled_bytes",Num(bytes.len().to_string())),("nodes",Arr(nodes)),("edges",Arr(edges)),("notes",Arr(vec![s("Encoding nodes use logical source paths. Local IR records its block-entry conditions. The complete_digest workflow reads complete selected source bytes.")]))]);
 put(&mut graph,"decoder_version",Num("3".into()));
 let flow=binary_flow::derive(&array(&graph,"nodes"),coord,&||access.cancelled())?;put(&mut graph,"control_flow",flow);
 let room=CAP.saturating_sub(super::super::mcp_schema::state_bytes(&graph)).saturating_sub(1024);
 let declarations=binary_declarations::derive(observation,identity.len(),bytes.len()as u64,coord,room,&||access.cancelled())?;if declarations!=Null{put(&mut graph,"declarations",declarations);}else{put(&mut graph,"declaration_note",s("Typed declaration projection did not fit; existing encodings and textual declarations remain available"));}
 optional_paths(&mut graph,&observation.statements,&||access.cancelled())?;
 let hash=payload_digest(&graph);put(&mut graph,"payload_sha256",s(&hash));let display=format!("{display}{}",footer(&hash));if let Obj(fields)=&mut graph{if let Some(Arr(nodes))=fields.get_mut("nodes"){put(&mut nodes[0],"representation",representation(&display,REPORT_METHOD));}}
 validate(&graph)?;if access.cancelled(){return Err("Binary evidence projection cancelled".into());}Ok((display,graph))
}
fn checked_representation(node:&Json,method:&str,content:Option<&str>)->Result<()>{
 let rep=get(node,"representation");let bytes=integer(get(rep,"bytes"))?;let hash=text(rep,"sha256");
 if text(rep,"method")!=method||get(rep,"complete")!=&Bool(true)||!digest(&hash)||bytes>128*1024{return Err("Malformed binary evidence representation".into());}
 if let Some(content)=content{if bytes!=content.len()as u64||harness_rules::sha256(content.as_bytes())!=hash{return Err("Binary evidence representation changed".into());}}
 Ok(())
}
pub(super) fn validate(graph:&Json)->Result<()>{
 let fail=||"Malformed binary observation; textual evidence remains available".to_string();
 if get(graph,"schema")!=&Num("1".into())||text(graph,"kind")!="binary"||text(graph,"coverage")!="Partial"||super::super::mcp_schema::state_bytes(graph)>CAP||get(graph,"workspace_identity")==&Null||!observation_graph::valid_identity(get(graph,"workspace_identity")){return Err(fail());}
 let coord=text(graph,"coordinate_kind");if !["ELF declared VA","PE RVA"].contains(&coord.as_str()){return Err(fail());}
 let selected=file_evidence::normalize(&text(&array(graph,"nodes").first().cloned().unwrap_or(Null),"path"))?;if text(graph,"selected_hex")!=hex(&selected){return Err(fail());}
 let nodes=get(graph,"nodes").array()?;let edges=get(graph,"edges").array()?;
 let observed=integer(get(graph,"observed_words"))?;let retained=integer(get(graph,"retained_words"))?;let omitted=integer(get(graph,"omitted_words"))?;let sampled=integer(get(graph,"sampled_bytes"))?;
 if nodes.is_empty()||nodes.len()>500||observed>512||retained!=nodes.len()as u64-1||retained>observed||omitted!=observed-retained||sampled>4*1024*1024||edges.len()!=nodes.len()-1{return Err(fail());}
 let root=&nodes[0];let sample=get(root,"sample");let size=integer(get(get(root,"identity"),"size"))?;
 if get(root,"id")!=&Num("0".into())||text(root,"path")!=String::from_utf8_lossy(&selected)||text(root,"path_hex")!=text(graph,"selected_hex")||text(root,"kind")!="BinarySource"||text(root,"coverage")!="Partial"||get(root,"identity")==&Null||!observation_graph::valid_identity(get(root,"identity"))||get(sample,"offset")!=&Num("0".into())||integer(get(sample,"length"))?!=sampled||sampled>size||get(sample,"complete_file")!=&Bool(sampled==size)||!digest(&text(sample,"sha256"))||text(sample,"method")!="SHA-256 of exactly the held inspection sample"{return Err(fail());}
 checked_representation(root,REPORT_METHOD,None)?;let source=text(root,"path");let mut previous=None;let mut addresses=std::collections::BTreeSet::new();
 for(i,node)in nodes.iter().enumerate().skip(1){
  let encoding=get(node,"encoding");let offset=integer(get(encoding,"file_offset"))?;let address=integer(get(encoding,"address"))?;let block=integer(get(encoding,"block"))?;let bits=integer(get(encoding,"bits"))?;let name=logical_path(&source,offset,address);let mnemonic=text(encoding,"mnemonic");
  if get(node,"id")!=&Num(i.to_string())||text(node,"kind")!="Encoding"||text(node,"coverage")!="Partial"||text(node,"path")!=name||text(node,"path_hex")!=hex(name.as_bytes())||get(node,"identity")!=&Null||get(node,"sample")!=&Null||text(encoding,"isa")!="AArch64"||text(encoding,"order")!="little"||get(encoding,"bytes")!=&Num("4".into())||bits>u32::MAX as u64||text(encoding,"coordinate_kind")!=coord||address%4!=0||block%4!=0||block>address||offset.checked_add(4).is_none_or(|n|n>sampled)||previous.is_some_and(|p|p>=(address,offset))||mnemonic.is_empty()||mnemonic.len()>16||!mnemonic.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.')||get(encoding,"recognized")!=&Bool(mnemonic!="unclassified"){return Err(fail());}
  if coord=="PE RVA"&&address>u32::MAX as u64{return Err(fail());}addresses.insert(address);if !addresses.contains(&block){return Err(fail());}previous=Some((address,offset));
  let content=get(node,"content").text()?;if content.len()>2048||!content.starts_with(&prefix(bits as u32,offset,address,block,&mnemonic,&coord))||!content.ends_with('\n'){return Err(fail());}checked_representation(node,IR_METHOD,Some(content))?;
  let edge=&edges[i-1];if text(edge,"relation")!="contains"||get(edge,"parent")!=&Num("0".into())||get(edge,"child")!=&Num(i.to_string()){return Err(fail());}
 }
 let decoder=if get(graph,"decoder_version")==&Null{1}else{integer(get(graph,"decoder_version"))?};if ![1,2,3].contains(&decoder){return Err(fail());}
 if get(graph,"control_flow")!=&Null&&get(graph,"control_flow")!=&binary_flow::derive_version(nodes,&coord,decoder,&||false)?{return Err(fail());}
 if get(graph,"declarations")!=&Null{binary_declarations::validate(get(graph,"declarations"),size,sampled,&coord)?;}
 let hash=text(graph,"payload_sha256");if !digest(&hash)||payload_digest(graph)!=hash{return Err(fail());}Ok(())
}
pub(super) fn validate_record(graph:&Json,record:&Json)->Result<()>{
 if text(record,"tool")!="inspect"||get(get(record,"args"),"content")!=&Null{return Err("Binary observation source operation differs from its retained record".into());}
 let nodes=get(graph,"nodes").array()?;let root=nodes.first().ok_or("Binary observation has no source node")?;let report=get(record,"report").text()?;checked_representation(root,REPORT_METHOD,Some(report))?;if !report.ends_with(&footer(&text(graph,"payload_sha256"))){return Err("Binary observation payload differs from the retained report".into());}Ok(())
}
pub(super) fn is_binary(v:&Json)->bool{text(v,"kind")=="binary"}
#[cfg(test)]mod path_checks{
 use super::*;
 fn statement()->instruction::Statement{instruction::Statement{offset:0,address:0,word:0,block:0,mnemonic:"ADD",text:"local unknown".into(),path_facts:"\nConditional path from a captured predecessor: encoded integer 0x8; alternate entries unknown.".into()}}
 fn graph()->Json{obj(vec![("nodes",Arr(vec![Null,obj(vec![("content",s("local unknown\n")),("representation",representation("local unknown\n",IR_METHOD))])]))])}
 #[test]fn optional_path_projection_cannot_evict_existing_evidence(){let mut g=graph();put(&mut g,"padding",s(""));let count=super::super::super::mcp_schema::state_bytes(&g);put(&mut g,"padding",s(&"x".repeat(CAP-1024-count)));let old=g.clone();optional_paths(&mut g,&[statement()],&||false).unwrap();assert_eq!(g,old);}
 #[test]fn projected_path_is_covered_by_its_exact_representation(){let mut g=graph();optional_paths(&mut g,&[statement()],&||false).unwrap();let node=&array(&g,"nodes")[1];let content=text(node,"content");assert!(content.contains("Conditional path"));checked_representation(node,IR_METHOD,Some(&content)).unwrap();}
 #[test]fn cancelled_path_projection_is_withheld(){let mut g=graph();let old=g.clone();assert!(optional_paths(&mut g,&[statement()],&||true).is_err());assert_eq!(old,g);}
}
