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
 if let Obj(m)=v{if let Some(previous)=m.remove("investigation_skill"){m.entry("investigation_preferences".into()).or_insert(previous);}}
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
  if let Some(x)=m.remove(old){m.entry("mcp".into()).or_insert(x);}
  for key in ["agent","child"]{if let Some(a)=m.get_mut(key){agent(a,old);}}
  if m.get("harness")==Some(prior){m.insert("harness".into(),current.clone());}
 }
}
pub(super) fn apply(st:&mut Json){
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
    let retained=m.entry("investigation_saved_preferences".into()).or_insert(Arr(vec![]));if let Arr(rows)=retained{if !rows.contains(&previous){rows.push(previous);}}
   }else{m.entry("investigation_preferences".into()).or_insert(previous);}
  }
  let names=m.keys().filter(|k|*k==&old||k.starts_with(&prefix)).cloned().collect::<Vec<_>>();
  for name in names{
   let value=m.remove(&name).unwrap();let target=if name==old{"mcp".into()}else{format!("mcp_client_{}",&name[prefix.len()..])};
   if target=="mcp"&&m.get(&target).is_some_and(|v|v!=&value)&&value!=Null{
    let retained=m.entry("mcp_saved_connections".into()).or_insert(Arr(vec![]));if let Arr(rows)=retained{if !rows.contains(&value){rows.push(value);}}
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

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn import_preserves_settings_tools_and_user_text_without_active_aliases(){
  let old=previous();let config=obj(vec![("endpoint",s("http://127.0.0.1:9000/mcp")),("key",s("private"))]);
  let mut st=obj(vec![("version",Num("1".into())),(&old,config.clone()),("agents",Arr(vec![obj(vec![("tools",s(&format!("read,{old},write"))),("instructions",s(&old))])])),("pending",obj(vec![("tool",s(&old)),("args",obj(vec![("name",s(&old))]))])),("chats",Arr(vec![obj(vec![("text",s(&old))])]))]);
  apply(&mut st);assert_eq!(get(&st,"mcp"),&config);assert_eq!(get(&st,&old),&Null);assert_eq!(text(&array(&st,"agents")[0],"tools"),"read,mcp,write");assert_eq!(text(&array(&st,"agents")[0],"instructions"),old);assert_eq!(text(get(&st,"pending"),"tool"),"mcp");assert_eq!(text(get(get(&st,"pending"),"args"),"name"),old);let once=st.clone();apply(&mut st);assert_eq!(st,once);
 }
 #[test]fn old_owners_cannot_resume_and_unrelated_work_is_preserved(){
  let old=previous();let key=format!("{old}_expected");let mut st=obj(vec![("busy",Bool(true)),(&key,s(&format!("{old}_server:old"))),("draft",s("Keep this draft"))]);apply(&mut st);assert_eq!(get(&st,"mcp_client_expected"),&Null);assert_eq!(get(&st,"busy"),&Bool(false));assert_eq!(text(&st,"draft"),"Keep this draft");
  let mut ordinary=obj(vec![("busy",Bool(true)),("model_expected",s("current-model-owner"))]);let before=ordinary.clone();apply(&mut ordinary);assert_eq!(ordinary,before);
 }
 #[test]fn existing_current_connection_is_not_overwritten(){
  let old=previous();let current=obj(vec![("endpoint",s("http://current.test/mcp"))]);let prior=obj(vec![("endpoint",s("http://previous.test/mcp"))]);let mut st=obj(vec![("mcp",current.clone()),(&old,prior.clone())]);apply(&mut st);assert_eq!(get(&st,"mcp"),&current);assert_eq!(array(&st,"mcp_saved_connections"),vec![prior]);
 }
 #[test]fn optional_preferences_are_imported_without_turning_off_the_core_method(){
  let mut st=obj(vec![("dialog",s("investigation_skill")),("investigation_skill",obj(vec![("enabled",Bool(false)),("instructions",s("My extra guidance"))])),("agents",Arr(vec![obj(vec![("tools",s("read")),("investigation_skill",Bool(false))])]))]);apply(&mut st);
  assert_eq!(text(&st,"dialog"),"investigation_preferences");assert_eq!(get(get(&st,"investigation_preferences"),"enabled"),&Bool(false));assert_eq!(get(&array(&st,"agents")[0],"investigation_preferences"),&Bool(false));
 }
 #[test]fn rule_meaning_and_revoked_provenance_do_not_change(){
  let old=previous();let mut st=obj(vec![("harness_policy",obj(vec![("schema",Num("1".into())),("rules",Arr(vec![obj(vec![("id",s("deny-remote")),("text",s("Do not call the remote connection")),("scope",s("tool")),("tool",s(&old)),("decision",s("deny")),("enabled",Bool(true))])]))]))]);
  inquiry::execute(&mut st,&obj(vec![("action",s("start")),("question",s("Preserve the original question"))]),false).unwrap();let first=array(get(&st,"inquiry_records"),"items")[0].clone();let mut revoked=first.clone();put(&mut revoked,"id",s("revoked-question"));put(&mut revoked,"policy",s("revoked-policy"));put(&mut st,"inquiry_records",obj(vec![("schema",Num("1".into())),("items",Arr(vec![first,revoked]))]));apply(&mut st);assert_eq!(harness_rules::decision(&st,"mcp",&obj(vec![])).0,"deny");assert_eq!(get(&array(get(&st,"inquiry_records"),"items")[0],"policy"),&harness_rules::policy_binding(&st));assert_eq!(text(&array(get(&st,"inquiry_records"),"items")[1],"policy"),"revoked-policy");
 }
}
