//! One-time import of earlier connection identifiers; no active command aliases.
use super::*;
fn previous()->String{[114u8,101,97].iter().map(|b|*b as char).collect()}
fn needed(v:&Json,old:&str,prefix:&str,depth:usize)->bool{
 if depth>16{return false;}
 match v{
  Arr(rows)=>rows.iter().any(|r|needed(r,old,prefix,depth+1)),
  Obj(m)=>{
   m.keys().any(|k|k==old||k.starts_with(prefix)||k=="investigation_skill")||m.get("tool")==Some(&s(old))||
   matches!(m.get("tools"),Some(Str(v))if v.split(',').any(|t|t.trim()==old))||
   ["dialog","form_dialog"].iter().any(|key|matches!(m.get(*key),Some(Str(v))if v.starts_with(prefix)||v=="investigation_skill"||v=="analysis_advanced"))||
   ["agents","pending","harness_policy","rules","decisions","approval_rules","binding","agent","child","task_run","run","delegation_stack","refinement_pending","edit","inquiry_records","items","evidence"].iter().any(|key|m.get(*key).is_some_and(|v|needed(v,old,prefix,depth+1)))
  },
  _=>false
 }
}
fn token(v:&mut Json,old:&str){if v==&s(old){*v=s("mcp");}}
fn preferences(v:&mut Json){
 if let Obj(m)=v{if let Some(previous)=m.remove("investigation_skill"){m.get_or_insert("investigation_preferences".into(),previous);}}
}
fn agent(v:&mut Json,old:&str){
 preferences(v);
 if let Obj(m)=v{if let Some(Str(tools))=m.get_mut("tools"){if tools.split(',').any(|t|t.trim()==old){*tools=tools.split(',').map(|t|if t.trim()==old{"mcp"}else{t.trim()}).collect::<Vec<_>>().join(",");}}}
}
fn request(v:&mut Json,old:&str){
 if let Obj(m)=v{if let Some(t)=m.get_mut("tool"){token(t,old);}for key in ["experiment_operation","experiment_definition"]{if let Some(Obj(p))=m.get_mut(key){if let Some(t)=p.get_mut("tool"){token(t,old);}}}}
}
fn rules(v:&mut Json,old:&str){
 if let Obj(m)=v{for key in ["rules","decisions"]{if let Some(Arr(rows))=m.get_mut(key){for row in rows{if let Obj(r)=row{if let Some(t)=r.get_mut("tool"){token(t,old);}}}}}}
}
fn run(v:&mut Json,old:&str){
 if let Obj(m)=v{if let Some(a)=m.get_mut("agent"){agent(a,old);}for key in ["pending","experiment_operation"]{if let Some(p)=m.get_mut(key){request(p,old);}}}
}
fn binding(v:&mut Json,old:&str,prior:&Json,current:&Json){
 if let Obj(m)=v{
  if let Some(x)=m.remove(old){m.get_or_insert("mcp".into(),x);}
  for key in ["agent","child"]{if let Some(a)=m.get_mut(key){agent(a,old);}}
  if m.get("harness")==Some(prior){m.insert("harness".into(),current.clone());}
 }
}
fn hosted_mode(v:&mut Json){
 const STORED:[(&str,&str);4]=[
  ("minimax_messages_v1",crate::hosted_search::MODE),
  ("messages_search_v1",crate::hosted_search::MODE),
  ("responses_search_v1",crate::hosted_search::RESPONSES_SEARCH),
  ("google_search_v1",crate::hosted_search::GOOGLE_SEARCH),
 ];
 let Obj(fields)=v else{return};
 let Some(current)=fields.get("hosted_search").and_then(|value|match value{Str(text)=>Some(text.clone()),_=>None}) else{return};
 let Some(next)=STORED.iter().find(|pair|current==pair.0).map(|pair|pair.1) else{return};
 fields.insert("hosted_search".into(),s(next));
}
pub(super) fn apply(st:&mut Json){
 if let Obj(fields)=st{
  if let Some(Arr(profiles))=fields.get_mut("profiles"){for profile in profiles{hosted_mode(profile);}}
  if let Some(edit)=fields.get_mut("edit"){hosted_mode(edit);}
  if let Some(Obj(pending))=fields.get_mut("hosted_request"){if let Some(profile)=pending.get_mut("profile"){hosted_mode(profile);}}
 }
 let old=previous();let prefix=format!("{old}_");
 if !needed(st,&old,&prefix,0){return;}
 // Derive old provenance from the actual imported policy. Only a matching
 // historical binding can be updated; revoked/unrelated records stay revoked.
 let prior_binding=harness_rules::binding(st);let prior_policy=harness_rules::policy_binding(st);
 let mut observed=vec![];
 for row in array(get(st,"investigation_records"),"items"){observed.push((text(&row,"id"),harness_rules::observation_binding(st,&text(&row,"tool"),get(&row,"args"))));}
 let mut interrupted=false;
 if let Obj(m)=st{
  if let Some(previous)=m.remove("investigation_skill"){
   if m.get("investigation_preferences").is_some_and(|current|current!=&previous){
    let retained=m.get_or_insert("investigation_saved_preferences".into(),Arr(vec![]));if let Arr(rows)=retained{if !rows.contains(&previous){rows.push(previous);}}
   }else{m.get_or_insert("investigation_preferences".into(),previous);}
  }
  let names=m.keys().filter(|k|*k==&old||k.starts_with(&prefix)).cloned().collect::<Vec<_>>();
  for name in names{
   let value=m.remove(&name).unwrap();let target=if name==old{"mcp".into()}else{format!("mcp_client_{}",&name[prefix.len()..])};
   if target=="mcp"&&m.get(&target).is_some_and(|v|v!=&value)&&value!=Null{
    let retained=m.get_or_insert("mcp_saved_connections".into(),Arr(vec![]));if let Arr(rows)=retained{if !rows.contains(&value){rows.push(value);}}
   }else if !m.contains_key(&target){if target=="mcp_client_expected"&&matches!(&value,Str(v)if v.starts_with(&prefix)){interrupted=true;}m.insert(target,value);}
  }
  for key in ["dialog","form_dialog"]{if let Some(Str(v))=m.get_mut(key){if v.starts_with(&prefix){*v=format!("mcp_client_{}",&v[prefix.len()..]);}else if v=="investigation_skill"{*v="investigation_preferences".into();}else if v=="analysis_advanced"{*v="mcp_client_menu".into();}}}
  if let Some(t)=m.get_mut("active_tool"){token(t,&old);}
  if let Some(Arr(rows))=m.get_mut("agents"){for a in rows{agent(a,&old);}}
  if let Some(a)=m.get_mut("edit"){agent(a,&old);}
  if let Some(p)=m.get_mut("pending"){request(p,&old);}
  if let Some(p)=m.get_mut("harness_policy"){rules(p,&old);}
  for key in ["task_run","refinement_pending"]{if let Some(r)=m.get_mut(key){run(r,&old);}}
  if let Some(Arr(frames))=m.get_mut("delegation_stack"){for frame in frames{if let Obj(f)=frame{if let Some(r)=f.get_mut("run"){run(r,&old);}}}}
  if interrupted{
   for key in ["mcp_client_expected","mcp_client_connection","mcp_client_protocol","mcp_client_tools_capability","mcp_client_session","mcp_client_request_id","mcp_client_resume_token"]{m.insert(key.into(),Null);}
   m.insert("busy".into(),Bool(false));m.insert("notice".into(),s("The previous MCP operation was interrupted by a connection-format update. Nothing was replayed."));
  }
 }
 let current_binding=harness_rules::binding(st);let current_policy=harness_rules::policy_binding(st);
 let updated=observed.iter().map(|(id,prior)|{
  let row=array(get(st,"investigation_records"),"items").into_iter().find(|r|text(r,"id")==*id).unwrap_or(Null);
  (id.clone(),prior.clone(),harness_rules::observation_binding(st,&text(&row,"tool"),get(&row,"args")))
 }).collect::<Vec<_>>();
 if let Obj(m)=st{
  if let Some(Arr(rows))=m.get_mut("approval_rules"){for row in rows{
   let automatic=text(row,"tool")==old&&text(row,"label")==approval_rules::description(&old,get(row,"scope"));
   if let Obj(r)=row{if let Some(t)=r.get_mut("tool"){token(t,&old);}if automatic{let scope=r.get("scope").cloned().unwrap_or(Null);r.insert("label".into(),s(&approval_rules::description("mcp",&scope)));}if let Some(b)=r.get_mut("binding"){binding(b,&old,&prior_binding,&current_binding);}}
  }}
  if let Some(Obj(store))=m.get_mut("investigation_records"){if let Some(Arr(rows))=store.get_mut("items"){for row in rows{if let Obj(r)=row{
   if let Some((_,prior,current))=updated.iter().find(|(id,_,_)|r.get("id")==Some(&s(id))){if r.get("policy")==Some(prior){r.insert("policy".into(),current.clone());}}
  }}}}
 }
 inquiry::migrate_connection(st,&old,&prior_policy,&current_policy);
}
