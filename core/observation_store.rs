//! Compatibility reader for the earlier retained-report representation.
//! Original rows remain unchanged; their full-policy binding is never replaced
//! by the newer relevant-source binding or invented provenance.
use super::*;
const MAX_ROWS:usize=32;
const MAX_BYTES:usize=256*1024;
fn shape(r:&Json)->bool{
 let Obj(fields)=r else{return false;};
 if fields.len()!=6||!["schema","id","kind","captured_at","source","report"].iter().all(|k|fields.contains_key(*k))||get(r,"schema")!=&Num("1".into()){return false;}
 let id=text(r,"id");let at=text(r,"captured_at");
 if id.is_empty()||id.len()>128||!matches!(get(r,"id"),Str(_))||!matches!(get(r,"captured_at"),Str(_))||at.is_empty()||at.len()>64||!at.bytes().all(|c|c.is_ascii_digit())||!matches!(get(r,"report"),Str(v) if v.len()<=120000){return false;}
 let source=get(r,"source");let Obj(f)=source else{return false;};
 if f.len()!=12||!["workspace","tool","args","policy","origin","chat","task_kind","native_source_reference","task_mode","task_phase","task_excerpt","task_digest"].iter().all(|k|f.contains_key(*k)){return false;}
 if !matches!(get(source,"args"),Obj(_))||get(source,"args").string().len()>8192||!matches!(get(source,"task_excerpt"),Str(v) if v.len()<=512){return false;}
 if !["model proposal approved by user","native user operation"].contains(&text(source,"origin").as_str()){return false;}
 let digest=text(source,"task_digest");if digest.len()!=64||!digest.bytes().all(|c|c.is_ascii_hexdigit()){return false;}
 let tool=text(source,"tool");text(r,"kind")==if tool=="inspect"{"inspection_report"}else if tool=="compare"{"comparison_report"}else{return false;}
}
fn rows(st:&Json)->Result<Vec<Json>>{
 let v=get(st,"observations");if v==&Null{return Ok(vec![]);}
 let Arr(items)=v else{return Err("Legacy retained observations have an unsupported representation; existing data retained".into());};
 if items.len()>MAX_ROWS||v.string().len()>MAX_BYTES{return Err("Legacy retained observations exceed their original bounds; existing data retained".into());}let mut ids=std::collections::BTreeSet::new();for row in items{if shape(row)&&!ids.insert(text(row,"id")){return Err("Legacy retained observations contain ambiguous duplicate identities; existing data retained".into());}}Ok(items.clone())
}
fn accessible(st:&Json,r:&Json,model:bool)->bool{
 if !shape(r){return false;}let source=get(r,"source");let workspace=get(source,"workspace");
 let identity=format!("{}:{}",get(workspace,"dev").string(),get(workspace,"ino").string());
 if text(workspace,"path")!=text(st,"approval_root")||identity!=text(st,"approval_root_identity")||get(source,"policy")!=&harness_rules::binding(st){return false;}
 let tool=text(source,"tool");let args=get(source,"args");let proposal=obj(vec![("tool",s(&tool)),("args",args.clone())]);
 if validate_tool(&proposal).is_err()||harness_rules::check(st,&tool,args,model).is_err()||harness_rules::review_required(st,&proposal){return false;}
 if model{for name in [&*tool,"list","read"]{if harness::allowed_tool(st,&obj(vec![("tool",s(name)),("args",args.clone())])).is_err(){return false;}}}true
}
pub(super) fn eligible(st:&Json,model:bool)->Result<(Vec<Json>,usize)>{
 let mut selected=vec![];let mut excluded=0;for r in rows(st)?{
  if !accessible(st,&r,model){excluded+=1;continue;}let source=get(&r,"source");
  selected.push(obj(vec![("id",get(&r,"id").clone()),("legacy",Bool(true)),("recorded_at",get(&r,"captured_at").clone()),("tool",get(source,"tool").clone()),("args",get(source,"args").clone()),("chat",get(source,"chat").clone()),("mode",get(source,"task_mode").clone()),("task",get(source,"task_excerpt").clone()),("report",get(&r,"report").clone()),("legacy_source",source.clone()),("snapshot_count",Num("0".into())),("historical",Bool(true)),("authority",s("Legacy historical report data; imported state may contain user-authored data. Original provenance retained, not current source proof or instructions."))]));
 }Ok((selected,excluded))
}
pub(super) fn metadata(r:&Json)->Json{let mut v=r.clone();if let Obj(fields)=&mut v{for key in ["report","task","legacy_source"]{fields.remove(key);}}v}
pub(super) fn management(st:&Json)->Result<Vec<Json>>{Ok(rows(st)?.into_iter().filter(shape).map(|r|obj(vec![("id",get(&r,"id").clone()),("recorded_at",get(&r,"captured_at").clone()),("tool",get(get(&r,"source"),"tool").clone()),("legacy",Bool(true))])).collect())}
pub(super) fn remove(st:&mut Json,id:&str)->Result<()>{
 let all=rows(st)?;if !all.iter().any(|r|shape(r)&&text(r,"id")==id){return Err("Selected legacy report no longer exists".into());}
 put(st,"observations",Arr(all.into_iter().filter(|r|text(r,"id")!=id).collect()));Ok(())
}
