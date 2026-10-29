//! Application storage and current-access projection for investigation claims.
use super::*;
#[path="investigation_claims.rs"] mod claims;
const KEY:&str="investigation_claims";
const LIMIT:usize=256*1024;

fn citation(v:&Json)->Result<claims::Citation>{
 let record=get(v,"record").text()?.to_string();
 fn index(v:&Json)->Result<Option<u32>>{match v{Null=>Ok(None),Num(n)=>n.parse().map(Some).map_err(|_|"Invalid citation index".into()),_=>Err("Invalid citation index".into())}}
 let c=claims::Citation{record,snapshot:index(get(v,"snapshot"))?,node:index(get(v,"node"))?};c.validate()?;Ok(c)
}
fn citation_json(c:&claims::Citation)->Json{obj(vec![("record",s(&c.record)),("snapshot",c.snapshot.map(|n|Num(n.to_string())).unwrap_or(Null)),("node",c.node.map(|n|Num(n.to_string())).unwrap_or(Null))])}
fn kind(v:&str)->Result<claims::Kind>{match v{"File"=>Ok(claims::Kind::File),"Directory"=>Ok(claims::Kind::Directory),"Symlink"=>Ok(claims::Kind::Symlink),"Other"=>Ok(claims::Kind::Other),_=>Err("Unknown observed kind".into())}}
fn criterion(v:&Json)->Result<Option<claims::Criterion>>{
 if v==&Null{return Ok(None);}
 Ok(Some(match text(v,"type").as_str(){
 "kind"=>claims::Criterion::Kind{source:citation(get(v,"source"))?,expected:kind(&text(v,"expected"))?},
 "size"=>claims::Criterion::Size{source:citation(get(v,"source"))?,expected:match get(v,"expected"){Num(n)=>n.parse().map_err(|_|"Expected byte length must be an unsigned integer")?,_=>return Err("Expected byte length must be an unsigned integer".into())}},
 "complete_capture_digest_equal"=>claims::Criterion::CompleteCaptureDigestEqual{left:citation(get(v,"left"))?,right:citation(get(v,"right"))?},
 _=>return Err("Unsupported claim criterion; saved claim remains unchanged".into()),
 }))
}
fn criterion_json(c:&Option<claims::Criterion>)->Json{match c{
 None=>Null,
 Some(claims::Criterion::Kind{source,expected})=>obj(vec![("type",s("kind")),("source",citation_json(source)),("expected",s(&format!("{expected:?}")))]),
 Some(claims::Criterion::Size{source,expected})=>obj(vec![("type",s("size")),("source",citation_json(source)),("expected",Num(expected.to_string()))]),
 Some(claims::Criterion::CompleteCaptureDigestEqual{left,right})=>obj(vec![("type",s("complete_capture_digest_equal")),("left",citation_json(left)),("right",citation_json(right))]),
}}
fn origin_json(o:&claims::Origin)->Json{match o{claims::Origin::User=>obj(vec![("type",s("user"))]),claims::Origin::Model{agent,connection}=>obj(vec![("type",s("model")),("agent",s(agent)),("connection",s(connection))])}}
fn origin(v:&Json)->Result<claims::Origin>{match text(v,"type").as_str(){"user"=>Ok(claims::Origin::User),"model"=>Ok(claims::Origin::Model{agent:get(v,"agent").text()?.into(),connection:get(v,"connection").text()?.into()}),_=>Err("Unknown claim origin".into())}}
fn revision(v:&Json)->Result<claims::Revision>{
 let id=match get(v,"id"){Num(n)=>n.parse().map_err(|_|"Invalid claim ID")?,_=>return Err("Invalid claim ID".into())};
 let previous=match get(v,"previous"){Null=>None,Num(n)=>Some(n.parse().map_err(|_|"Invalid prior claim ID")?),_=>return Err("Invalid prior claim ID".into())};
 Ok(claims::Revision{id,previous,question:get(v,"question").text()?.into(),hypothesis:get(v,"hypothesis").text()?.into(),origin:origin(get(v,"origin"))?,citations:get(v,"citations").array()?.iter().map(citation).collect::<Result<_>>()?,criterion:criterion(get(v,"criterion"))?})
}
fn revision_json(r:&claims::Revision)->Json{obj(vec![("id",Num(r.id.to_string())),("previous",r.previous.map(|id|Num(id.to_string())).unwrap_or(Null)),("question",s(&r.question)),("hypothesis",s(&r.hypothesis)),("origin",origin_json(&r.origin)),("citations",Arr(r.citations.iter().map(citation_json).collect())),("criterion",criterion_json(&r.criterion))])}
fn store(st:&Json)->Result<claims::Claims>{
 let v=get(st,KEY);if v==&Null{return Ok(claims::Claims::default());}
 if get(v,"schema")!=&Num("1".into())||super::super::mcp_schema::state_bytes(v)>LIMIT{return Err("Unsupported or oversized claim storage; existing claims are retained".into());}
 claims::Claims::restore(get(v,"items").array()?.iter().map(revision).collect::<Result<_>>()?)
}
fn evidence(st:&Json,c:&claims::Citation,model:bool)->Result<Json>{
 c.validate()?;let mut args=obj(vec![("id",s(&c.record))]);
 if let Some(side)=c.snapshot{put(&mut args,"snapshot",Num(side.to_string()));}
 if let Some(node)=c.node{put(&mut args,"node",Num(node.to_string()));}
 parse(&observation_records::query(st,&args,model)?)
}
fn fact(st:&Json,c:&claims::Citation,model:bool)->Option<claims::Fact>{
 let v=evidence(st,c,model).ok()?;let nodes=array(&v,"nodes");if nodes.len()!=1{return None;}
 let n=&nodes[0];let identity=get(n,"identity");
 let size=if let Num(v)=get(identity,"size"){v.parse::<u64>().ok()}else{None};
 let p=get(n,"sample");let sample=(||{
  let Num(offset)=get(p,"offset")else{return None;};let Num(length)=get(p,"length")else{return None;};
  let Bool(complete_file)=get(p,"complete_file")else{return None;};let hash=text(p,"sha256");if hash.len()!=64{return None;}
  let mut sha256=[0u8;32];for (i,pair)in hash.as_bytes().chunks_exact(2).enumerate(){let s=std::str::from_utf8(pair).ok()?;sha256[i]=u8::from_str_radix(s,16).ok()?;}
  Some(claims::Sample{offset:offset.parse().ok()?,length:length.parse().ok()?,complete_file:*complete_file,sha256})
 })();
 Some(claims::Fact{kind:kind(&text(n,"kind")).ok(),size,changed_during_observation:text(n,"coverage")=="Changed"||text(&v,"coverage")=="Changed",sample})
}

/// Origin is supplied by an authenticated caller, never decoded from args.
pub(super) fn append(st:&mut Json,args:&Json,model_origin:Option<(&str,&str)>)->Result<u64>{
 harness_rules::check(st,"hypotheses",&obj(vec![("create",args.clone())]),model_origin.is_some())?;
 let Obj(fields)=args else{return Err("Claim requires an object".into());};
 if fields.keys().any(|k|!["question","hypothesis","previous","citations","criterion"].contains(&k.as_str())){return Err("Unknown claim field; origin and identity are owned by the harness".into());}
 let previous=match get(args,"previous"){Null=>None,Num(n)=>Some(n.parse().map_err(|_|"Invalid prior claim ID")?),_=>return Err("Invalid prior claim ID".into())};
 if let Some(id)=previous{query(st,Some(id),model_origin.is_some())?;}
 let citations=match get(args,"citations"){Null=>vec![],Arr(items)=>items.iter().map(citation).collect::<Result<_>>()?,_=>return Err("Claim citations must be an array".into())};
 let draft=claims::Draft{previous,question:get(args,"question").text()?.into(),hypothesis:get(args,"hypothesis").text()?.into(),citations,criterion:criterion(get(args,"criterion"))?};
 let author=match model_origin{None=>claims::Origin::User,Some((agent,connection))=>claims::Origin::Model{agent:agent.into(),connection:connection.into()}};
 let mut records=store(st)?;let next=match get(get(st,KEY),"next_id"){Null=>records.revisions().last().map_or(Some(1),|r|r.id.checked_add(1)).ok_or("Claim identity space exhausted")?,Num(n)=>n.parse::<u64>().map_err(|_|"Malformed claim identity counter")?,_=>return Err("Malformed claim identity counter".into())};let following=next.checked_add(1).ok_or("Claim identity space exhausted")?;
 let record=records.append_at(draft,author,next,|c|evidence(st,c,model_origin.is_some()).is_ok())?.clone();
 let mut current=get(st,KEY).clone();if current==Null{current=obj(vec![("schema",Num("1".into())),("items",Arr(vec![]))]);}
 put(&mut current,"next_id",Num(following.to_string()));let mut items=array(&current,"items");let mut entry=revision_json(&record);
 put(&mut entry,"workspace",get(st,"approval_root").clone());put(&mut entry,"workspace_identity",get(st,"approval_root_identity").clone());put(&mut entry,"recorded_at",s(&now()));items.push(entry);put(&mut current,"items",Arr(items));
 let bytes=super::super::mcp_schema::state_bytes(&current);let projected=super::super::mcp_schema::state_bytes(st).saturating_sub(super::super::mcp_schema::state_bytes(get(st,KEY))).saturating_add(bytes);
 if bytes>LIMIT||projected>6*1024*1024-256*1024{return Err("Claim storage allowance reached; previous claims and current work are unchanged".into());}
 put(st,KEY,current);Ok(record.id)
}

pub(super) fn query(st:&Json,id:Option<u64>,model:bool)->Result<Json>{
 harness_rules::check(st,"hypotheses",&obj(vec![("id",id.map(|n|Num(n.to_string())).unwrap_or(Null))]),model)?;
 let records=store(st)?;let raw=array(get(st,KEY),"items");let mut items=vec![];let mut unavailable=0;
 for (r,v)in records.revisions().iter().zip(raw.iter()){
  if id.is_some_and(|id|r.id!=id){continue;}
  if get(v,"workspace")!=get(st,"approval_root")||get(v,"workspace_identity")!=get(st,"approval_root_identity")||r.citations.iter().any(|c|evidence(st,c,model).is_err()){unavailable+=1;continue;}
  let mut out=if id.is_some(){revision_json(r)}else{let end=r.question.floor_char_boundary(r.question.len().min(256));obj(vec![("id",Num(r.id.to_string())),("question",s(&r.question[..end])),("question_truncated",Bool(end<r.question.len())),("origin",origin_json(&r.origin))])};put(&mut out,"authority",s("An authored hypothesis, not a user rule, measured fact or proof of the prose claim"));
  if id.is_some(){if let Some(c)=&r.criterion{let outcome=claims::evaluate(c,|c|fact(st,c,model));put(&mut out,"evaluation",obj(vec![("verdict",s(&format!("{:?}",outcome.verdict))),("explanation",s(outcome.explanation)),("historical",Bool(true))]));}}
  items.push(out);
 }
 if id.is_some()&&items.is_empty(){return Err("Claim unavailable under current workspace or evidence access".into());}
 let out=obj(vec![("claims",Arr(items)),("unavailable",Num(unavailable.to_string()))]);
 let policy=super::super::resources::policy(get(st,"device"),get(st,"resources"),super::super::mcp_schema::state_bytes(st));
 if super::super::mcp_schema::state_bytes(&out)>number(&policy,"response_bytes").min(LIMIT){return Err("Claims exceed the current response allowance; request an exact claim ID".into());}Ok(out)
}

pub(super) fn validate_tool(args:&Json)->Result<()>{
 let Obj(fields)=args else{return Err("Hypothesis operation requires an object".into());};
 if fields.keys().any(|k|!["id","create"].contains(&k.as_str())){return Err("Hypotheses accepts id or create only; empty arguments list claims".into());}
 if get(args,"id")!=&Null&&get(args,"create")!=&Null{return Err("Choose a query or a new claim revision".into());}
 if get(args,"id")!=&Null&&!matches!(get(args,"id"),Num(n) if n.parse::<u64>().ok().is_some_and(|n|n>0)){return Err("Claim ID must be a positive integer".into());}
 if get(args,"create")!=&Null{
  let Obj(d)=get(args,"create")else{return Err("Claim creation requires an object".into());};
  if d.keys().any(|k|!["question","hypothesis","previous","citations","criterion"].contains(&k.as_str())){return Err("Unknown claim field; origin and identity are harness-owned".into());}
  for k in ["question","hypothesis"]{let value=get(get(args,"create"),k).text()?;if value.trim().is_empty()||value.len()>claims::MAX_TEXT{return Err("Question and hypothesis must be nonempty and within 8192 UTF-8 bytes".into());}}
  if super::super::mcp_schema::state_bytes(get(args,"create"))>32768{return Err("Claim request exceeds the current request allowance".into());}
 }
 Ok(())
}
pub(super) fn execute(st:&mut Json,args:&Json,model:bool)->Result<String>{
 validate_tool(args)?;
 if get(args,"create")==&Null{let id=if let Num(n)=get(args,"id"){Some(n.parse().map_err(|_|"Invalid claim ID")?)}else{None};return Ok(query(st,id,model)?.string());}
 let agent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};
 let agent_name=text(&agent,"name");let connection=if model{
  let binding=get(get(st,"pending"),"claim_author");let index=match get(binding,"index"){Num(n)=>n.parse::<usize>().map_err(|_|"Invalid claim author identity")?,_=>return Err("Claim proposal has no captured author identity; request a fresh proposal".into())};
  let profile=array(st,"profiles").get(index).cloned().ok_or("Claim author connection is unavailable")?;
  if get(binding,"index")!=get(st,"request_index")||text(binding,"profile")!=harness_rules::digest(&profile)||text(binding,"agent")!=harness_rules::digest(&agent){return Err("Claim author context changed; request a fresh proposal".into());}
  format!("connection-index:{index}")
 }else{String::new()};
 let id=append(st,get(args,"create"),if model{Some((&agent_name,&connection))}else{None})?;
 Ok(query(st,Some(id),model)?.string())
}
pub(super) fn own(st:&Json,proposal:&mut Json)->Result<()>{
 if text(proposal,"tool")!="hypotheses"{return Ok(());}
 let index=match get(st,"request_index"){Num(n)=>n.parse::<usize>().map_err(|_|"Invalid authoring connection")?,_=>return Err("Authoring connection is unavailable".into())};
 let profile=array(st,"profiles").get(index).cloned().ok_or("Authoring connection is unavailable")?;
 let agent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};
 put(proposal,"claim_author",obj(vec![("index",Num(index.to_string())),("profile",s(&harness_rules::digest(&profile))),("agent",s(&harness_rules::digest(&agent)))]));Ok(())
}

pub(super) fn handles(action:&str)->bool{["investigation_claims","claims_next","claims_previous","claim_new","claim_save","claim_from_node","claim_delete","claim_delete_confirm","claims_manage","claims_manage_next","claims_manage_previous"].contains(&action)||action.starts_with("claim_view:")||action.starts_with("claim_revision:")||action.starts_with("claim_manage:")}
fn criterion_fields(edit:&mut Json,citations:&Json,criterion:&Json){
 let first=match text(criterion,"type").as_str(){"kind"|"size"=>get(criterion,"source").clone(),"complete_capture_digest_equal"=>get(criterion,"left").clone(),_=>if let Arr(items)=citations{items.first().cloned().unwrap_or(Null)}else{Null}};
 for (field,key)in[("source_record","record"),("source_snapshot","snapshot"),("source_node","node")]{let value=get(&first,key);put(edit,field,s(&if let Str(v)=value{v.clone()}else if value==&Null{String::new()}else{value.string()}));}
 put(edit,"criterion_method",s(&if criterion==&Null{"none".into()}else{text(criterion,"type")}));
 let expected=get(criterion,"expected");put(edit,"expected",s(&if let Str(v)=expected{v.clone()}else if expected==&Null{String::new()}else{expected.string()}));
 let right=get(criterion,"right");for (field,key)in[("other_record","record"),("other_snapshot","snapshot"),("other_node","node")]{let value=get(right,key);put(edit,field,s(&if let Str(v)=value{v.clone()}else if value==&Null{String::new()}else{value.string()}));}
}
fn edited_criterion(st:&Json)->Result<(Json,Json)>{
 let e=get(st,"edit");
 fn reference(e:&Json,prefix:&str)->Result<Json>{let record=text(e,&format!("{prefix}_record"));if record.is_empty(){return Err("Choose an exact saved observation record".into());}let mut v=obj(vec![("record",s(&record))]);for k in ["snapshot","node"]{let raw=text(e,&format!("{prefix}_{k}"));let n=raw.parse::<u32>().map_err(|_|"Observation snapshot and node must be nonnegative integers")?;put(&mut v,k,Num(n.to_string()));}citation(&v)?;Ok(v)}
 let method=text(e,"criterion_method");let mut citations=get(st,"claim_citations").clone();if citations==Null{citations=Arr(vec![]);}
 if method.is_empty()||method=="none"{return Ok((citations,Null));}
 let source=reference(e,"source")?;let mut list=vec![source.clone()];
 let criterion=match method.as_str(){
 "kind"=>{kind(&text(e,"expected"))?;obj(vec![("type",s("kind")),("source",source),("expected",get(e,"expected").clone())])},
 "size"=>{let n=text(e,"expected").parse::<u64>().map_err(|_|"Expected byte length must be an unsigned integer")?;obj(vec![("type",s("size")),("source",source),("expected",Num(n.to_string()))])},
 "complete_capture_digest_equal"=>{let right=reference(e,"other")?;if right!=source{list.push(right.clone());}obj(vec![("type",s("complete_capture_digest_equal")),("left",source),("right",right)])},
 _=>return Err("Choose a supported criterion".into()),
 };if let Arr(old)=citations{for c in old{if !list.contains(&c){list.push(c);}}}Ok((Arr(list),criterion))
}
pub(super) fn event(st:&mut Json,action:&str)->Result<()>{
 if interaction::locked(st){return Err("Finish or cancel the current operation before editing investigation claims".into());}
 if ["claims_manage","claims_manage_next","claims_manage_previous"].contains(&action){store(st)?;let old=number(st,"claims_management_offset");put(st,"claims_management_offset",Num(match action{"claims_manage_next"=>old.saturating_add(12).min(60),"claims_manage_previous"=>old.saturating_sub(12),_=>0}.to_string()));put(st,"dialog",s("claims_management"));return Ok(());}
 if let Some(raw)=action.strip_prefix("claim_manage:"){
  if text(st,"dialog")!="claims_management"{return Err("Open claim storage management first".into());}let id=raw.parse::<u64>().map_err(|_|"Invalid claim ID")?;if !store(st)?.revisions().iter().any(|r|r.id==id){return Err("Claim no longer exists".into());}put(st,"claim_id",Num(id.to_string()));put(st,"claim_management",Bool(true));put(st,"dialog",s("claim_delete"));return Ok(());
 }
 if action=="claim_delete"||action=="claim_delete_confirm"{
  if text(st,"dialog")!=if action=="claim_delete"{"claim_view"}else{"claim_delete"}{return Err("Open the exact claim before confirming deletion".into());}
  let id=match get(st,"claim_id"){Num(n)=>n.parse::<u64>().map_err(|_|"Invalid claim ID")?,_=>return Err("Select a claim first".into())};if !boolean(st,"claim_management"){query(st,Some(id),false)?;}
  harness_rules::check(st,"hypotheses",&obj(vec![("delete",Num(id.to_string()))]),false)?;let records=store(st)?;if !records.revisions().iter().any(|r|r.id==id){return Err("Selected claim no longer exists".into());}if records.revisions().iter().any(|r|r.previous==Some(id)){return Err("A later revision references this claim. Delete its later revisions individually first.".into());}
  if action=="claim_delete"{put(st,"dialog",s("claim_delete"));return Ok(());}
  let mut current=get(st,KEY).clone();if get(&current,"next_id")==&Null{let next=records.revisions().last().and_then(|r|r.id.checked_add(1)).ok_or("Claim identity space exhausted")?;put(&mut current,"next_id",Num(next.to_string()));}
  let items=array(&current,"items").into_iter().filter(|r|get(r,"id")!=&Num(id.to_string())).collect();put(&mut current,"items",Arr(items));put(st,KEY,current);put(st,"claim_id",Null);put(st,"dialog",s("investigation_claims"));return Ok(());
 }
 if ["investigation_claims","claims_next","claims_previous"].contains(&action){query(st,None,false)?;let old=number(st,"claims_offset");let offset=match action{"claims_next"=>old.saturating_add(12).min(60),"claims_previous"=>old.saturating_sub(12),_=>0};put(st,"claims_offset",Num(offset.to_string()));put(st,"dialog",s("investigation_claims"));return Ok(());}
 if let Some(raw)=action.strip_prefix("claim_view:"){
  let id=raw.parse::<u64>().map_err(|_|"Invalid claim ID")?;query(st,Some(id),false)?;put(st,"claim_management",Bool(false));put(st,"claim_id",Num(id.to_string()));put(st,"dialog",s("claim_view"));return Ok(());
 }
 if let Some(raw)=action.strip_prefix("claim_revision:"){
  let id=raw.parse::<u64>().map_err(|_|"Invalid claim ID")?;let view=query(st,Some(id),false)?;let r=array(&view,"claims").into_iter().next().ok_or("Claim unavailable")?;
  put(st,"claim_previous",Num(id.to_string()));put(st,"claim_citations",get(&r,"citations").clone());put(st,"claim_criterion",get(&r,"criterion").clone());
  let mut edit=obj(vec![("question",get(&r,"question").clone()),("hypothesis",get(&r,"hypothesis").clone())]);criterion_fields(&mut edit,get(&r,"citations"),get(&r,"criterion"));put(st,"edit",edit);put(st,"dialog",s("claim_editor"));return Ok(());
 }
 if action=="claim_new"||action=="claim_from_node"{
  let citations=if action=="claim_from_node"{
   if text(st,"dialog")!="investigation_graph"{return Err("Open an exact observed node before forming a claim".into());}
   let q=get(st,"investigation_graph_query");let c=citation(&obj(vec![("record",get(q,"id").clone()),("snapshot",get(q,"snapshot").clone()),("node",get(q,"node").clone())]))?;
   if c.node.is_none(){return Err("Select one observed node before forming a claim".into());}evidence(st,&c,false)?;vec![citation_json(&c)]
  }else{vec![]};
  let mut edit=obj(vec![]);criterion_fields(&mut edit,&Arr(citations.clone()),&Null);put(st,"claim_previous",Null);put(st,"claim_citations",Arr(citations));put(st,"claim_criterion",Null);put(st,"edit",edit);put(st,"dialog",s("claim_editor"));return Ok(());
 }
 if action=="claim_save"{
  if text(st,"dialog")!="claim_editor"{return Err("Open the claim editor before saving a revision".into());}
  let(citations,criterion)=edited_criterion(st)?;let args=obj(vec![("question",get(get(st,"edit"),"question").clone()),("hypothesis",get(get(st,"edit"),"hypothesis").clone()),("previous",get(st,"claim_previous").clone()),("citations",citations),("criterion",criterion)]);
  let id=append(st,&args,None)?;put(st,"claim_id",Num(id.to_string()));put(st,"dialog",s("claim_view"));return Ok(());
 }Err("Unknown investigation claim action".into())
}
pub(super) fn dialog(st:&Json)->Json{
 match text(st,"dialog").as_str(){
 "claims_management"=>{let mut buttons=vec![];let summary=match store(st){Ok(records)=>{let offset=number(st,"claims_management_offset");for r in records.revisions().iter().skip(offset).take(12){buttons.push(button(&format!("Revision {}",r.id),&format!("claim_manage:{}",r.id)));}if offset>0{buttons.push(button("Previous revisions","claims_manage_previous"));}if offset+12<records.revisions().len(){buttons.push(button("Next revisions","claims_manage_next"));}format!("{} retained revisions across workspaces. IDs only; no question, hypothesis or source contents are shown. Select an exact leaf revision for confirmed deletion.",records.revisions().len())},Err(e)=>e};buttons.push(button("Back to hypotheses","investigation_claims"));modal("Hypothesis storage",vec![field_ui("status","Retained revisions",&summary,"readonly")],buttons)},
 "claim_delete"=>modal("Delete this hypothesis revision?",vec![field_ui("id","Exact revision ID",&get(st,"claim_id").string(),"readonly"),field_ui("effect","Effect","Permanently delete only this hypothesis revision. Saved observations, source files, other revisions and conversation history remain. Earlier exports or backups are unchanged.","readonly")],vec![button("Delete this revision","claim_delete_confirm"),button("Keep revision","investigation_claims")]),
 "claim_editor"=>{let e=get(st,"edit");let mut fields=vec![field_ui("question","Question",&text(e,"question"),"multiline"),field_ui("hypothesis","Hypothesis",&text(e,"hypothesis"),"multiline"),interaction::choice("criterion_method","Historical criterion",&text(e,"criterion_method"),&[("None — untested hypothesis","none"),("Observed kind","kind"),("Observed byte length","size"),("Complete capture digests match","complete_capture_digest_equal")])];for(k,label)in[("source_record","Source record ID"),("source_snapshot","Source snapshot (starts at 0)"),("source_node","Source node ID"),("expected","Expected kind (File/Directory/Symlink/Other) or byte length"),("other_record","Comparison record ID (digest comparison)"),("other_snapshot","Comparison snapshot"),("other_node","Comparison node ID")]{fields.push(field_ui(k,label,&text(e,k),"text"));}fields.push(field_ui("authority","Meaning","A criterion checks the selected historical data only. It does not prove the prose hypothesis or become a rule. Saving creates a new revision; previous revisions remain.","readonly"));modal("Investigation hypothesis",fields,vec![button("Save hypothesis revision","claim_save"),button("Back to hypotheses","investigation_claims")])},
 "claim_view"=>{
  let id=match get(st,"claim_id"){Num(n)=>n.parse().ok(),_=>None};
  match id.ok_or_else(||"Claim unavailable".to_string()).and_then(|id|query(st,Some(id),false)){
   Ok(v)=>{let r=array(&v,"claims").into_iter().next().unwrap_or(Null);modal("Investigation hypothesis",vec![field_ui("question","Question",&text(&r,"question"),"readonly"),field_ui("hypothesis","Hypothesis",&text(&r,"hypothesis"),"readonly"),field_ui("origin","Authored by",&get(&r,"origin").string(),"readonly"),field_ui("citation","Evidence references",&get(&r,"citations").string(),"readonly"),field_ui("evaluation","Criterion result",&if get(&r,"evaluation")==&Null{"No deterministic criterion; this remains an unevaluated hypothesis.".into()}else{get(&r,"evaluation").string()},"readonly")],vec![button("Plan an experiment","experiment_new"),button("Create revised hypothesis",&format!("claim_revision:{}",get(&r,"id").string())),button("Delete this revision","claim_delete"),button("Back to hypotheses","investigation_claims")])},
   Err(e)=>modal("Investigation hypothesis",vec![field_ui("status","Unavailable",&e,"readonly")],vec![button("Back to hypotheses","investigation_claims")])
  }
 },
 _=>{
  let mut buttons=vec![button("New hypothesis","claim_new"),button("Manage retained hypotheses","claims_manage"),button("Back to investigation","investigation")];
  let summary=match query(st,None,false){Ok(v)=>{let rows=array(&v,"claims");let offset=number(st,"claims_offset");for r in rows.iter().rev().skip(offset).take(12){buttons.push(button(&format!("{} · {}{}",get(r,"id").string(),text(r,"question"),if boolean(r,"question_truncated"){ "..." }else{""}),&format!("claim_view:{}",get(r,"id").string())));}if offset>0{buttons.push(button("Newer hypotheses","claims_previous"));}if offset+12<rows.len(){buttons.push(button("Older hypotheses","claims_next"));}format!("{} accessible claim revisions; {} unavailable. Page starts at {} (newest first). Hypotheses are authored interpretations, never rules or proof.",rows.len(),number(&v,"unavailable"),offset)},Err(e)=>e};
  modal("Investigation hypotheses",vec![field_ui("status","Saved claims",&summary,"readonly")],buttons)
 }
 }
}

/// Evaluates a supplied criterion against accessible retained observations only.
/// Experiment callers replace every cited input with this run's exact record/node.
pub(super) fn evaluate_criterion(st:&Json,value:&Json,model:bool)->Result<Json>{
 let c=criterion(value)?.ok_or("Experiment requires a deterministic criterion")?;
 let outcome=claims::evaluate(&c,|source|fact(st,source,model));
 Ok(obj(vec![("verdict",s(&format!("{:?}",outcome.verdict))),("explanation",s(outcome.explanation)),("historical",Bool(true))]))
}
