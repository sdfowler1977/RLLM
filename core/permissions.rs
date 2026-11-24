//! Harness permission modes plus per-connection limits that default to the harness mode.
use super::*;

const MODES:[&str;3]=["review","limited","yolo"];
const ACCESS:[&str;3]=["inherit","ask","off"];
fn mode_label(key:&str)->&'static str{match key{"yolo"=>"YOLO","limited"=>"Limited",_=>"Ask"}}
pub(super) fn mode(st:&Json)->String{let m=text(st,"permission_mode");if MODES.contains(&m.as_str()){m}else{"review".into()}}
pub(super) fn label(st:&Json)->&'static str{mode_label(&mode(st))}
// The connection whose limits apply is the one this conversation would run on, so a
// child task is bounded by the child connection rather than the chat connection.
fn role(st:&Json)->&'static str{if get(st,"task_run")!=&Null{"subagent"}else{"primary"}}
fn index(st:&Json)->Option<usize>{harness::route(st,role(st)).ok()}
fn limits(st:&Json)->Json{match index(st){Some(i)=>get(get(st,"permissions"),&i.to_string()).clone(),None=>Null}}
fn connection(st:&Json)->String{
 match index(st){Some(i)=>match array(st,"profiles").into_iter().nth(i){Some(p)=>interaction::connection_label(&p),None=>"this connection".into()},None=>"this connection".into()}
}
fn web_class(t:&str)->bool{matches!(t,"web_search"|"web_read")}
fn file_class(t:&str)->bool{matches!(t,"list"|"read"|"write"|"search"|"inspect"|"compare"|"reconstruct"|"capture"|"listing"|"regions"|"packets")}
fn access(l:&Json,class:&str)->&'static str{match text(l,class).as_str(){"ask"=>"ask","off"=>"off",_=>"inherit"}}
fn effective(st:&Json)->(String,Json){let l=limits(st);let m=text(&l,"mode");(if MODES.contains(&m.as_str()){m}else{mode(st)},l)}
pub(super) fn requires_review(st:&Json,p:&Json)->bool{
 let l=limits(st);if l==Null{return false;}let t=text(p,"tool");
 (access(&l,"web")=="ask"&&web_class(&t))||(access(&l,"file")=="ask"&&file_class(&t))
}
pub(super) fn check(st:&Json,t:&str)->Result<()>{
 let l=limits(st);if l==Null{return Ok(());}
 if access(&l,"web")=="off"&&web_class(t){return Err(format!("Web access is off for {} (App settings → Permissions)",connection(st)));}
 if access(&l,"file")=="off"&&file_class(t){return Err(format!("File access is off for {} (App settings → Permissions)",connection(st)));}
 Ok(())
}
pub(super) fn stands_down(st:&Json,p:&Json)->bool{
 if get(p,"tool")==&Null||boolean(p,"format_review"){return false;}
 if harness_rules::review_required(st,p)||requires_review(st,p){return false;}
 let t=text(p,"tool");let(mode,l)=effective(st);
 // Web search is a tool, not a model: every permission mode runs it without a
 // review dialog, because a model without web evidence is not useful. Only a
 // written harness rule, the app-wide search review setting or that connection's
 // explicit web "ask" can gate it.
 if web_class(&t){return search::automatic(st)&&access(&l,"web")!="ask";}
 match mode.as_str(){"yolo"=>true,"limited"=>harness_rules::readonly(&t,get(p,"args")),_=>false}
}
fn options(class:&str)->Vec<(&'static str,&'static str)>{
 if class=="mode"{vec![("Ask before every operation","review"),("Limited: read-only runs free","limited"),("YOLO: no review dialog","yolo")]}
 else{vec![("Follow the permission mode","inherit"),("Always ask first","ask"),("Off: refuse the operation","off")]}
}
pub(super) fn dialog(st:&Json)->Json{
 modal("Permission mode",vec![field_ui("current","Current mode",label(st),"readonly"),
  field_ui("description","What each mode allows","Ask reviews every proposed operation except web search and page reads, which run without a dialog. Limited lets read-only operations run and reviews anything that writes, captures or uploads. YOLO runs a proposed operation without a review dialog. Web search is never reviewed unless you set that connection's web limit to Always ask. Harness rules you wrote, per-connection limits and Workspace source access still apply. Set per-connection limits in App settings → Permissions.","readonly")],
  MODES.into_iter().map(|key|button(mode_label(key),&format!("permission_mode:{key}"))).collect())
}
pub(super) fn settings(st:&Json)->Json{
 let mut fields=vec![interaction::choice("mode","Harness permission mode",&mode(st),&options("mode"))];
 for(i,p)in array(st,"profiles").into_iter().enumerate(){
  let current=get(get(st,"permissions"),&i.to_string());let name=format!("{} · {}",text(&p,"name"),text(&p,"model"));
  for class in ["mode","web","file"]{let value=text(&current,class);let value=if ACCESS.contains(&value.as_str())||MODES.contains(&value.as_str()){value}else{"inherit".into()};
   fields.push(interaction::choice(&format!("{class}_{i}"),&format!("{name} — {class} permission"),&value,&options(class)));}
 }
 modal("Permissions",fields,vec![button("Save permissions","permissions_save")])
}
fn save_settings(st:&mut Json,inputs:&Json)->Result<()>{
 if interaction::locked(st){return Err("Finish or cancel the current task or tool review first".into());}
 let harness=text(inputs,"mode");if !MODES.contains(&harness.as_str()){return Err("Choose a supported permission mode".into());}
 let mut saved=obj(vec![]);
 for i in 0..array(st,"profiles").len(){
  let mut row=obj(vec![]);
  for class in ["mode","web","file"]{
   let value=text(inputs,&format!("{class}_{i}"));
   if value.is_empty()||value=="inherit"{continue;}
   let allowed:&[&str]=if class=="mode"{&MODES}else{&ACCESS};
   if !allowed.contains(&value.as_str()){return Err(format!("Choose a supported {class} permission"));}
   put(&mut row,class,s(&value));
  }
  if row.string()!="{}"{put(&mut saved,&i.to_string(),row);}
 }
 put(st,"permission_mode",s(&harness));put(st,"permissions",saved);
 put(st,"dialog",Null);put(st,"notice",s(&format!("Permissions saved. Harness mode: {}",mode_label(&harness))));Ok(())
}
pub(super) fn set_mode(st:&mut Json,key:&str)->Result<()>{
 if boolean(st,"busy")||get(st,"task_run")!=&Null||get(st,"pending")!=&Null{return Err("Finish the current request before changing permission mode".into());}
 if !MODES.contains(&key){return Err("Choose a supported permission mode".into());}
 put(st,"permission_mode",s(key));put(st,"dialog",Null);put(st,"notice",s(&format!("Permission mode set to {}",mode_label(key))));Ok(())
}
pub(super) fn handles(a:&str)->bool{matches!(a,"permissions"|"permission_modes"|"permissions_save")||a.starts_with("permission_mode:")}
pub(super) fn event(st:&mut Json,a:&str,inputs:&Json)->Result<()>{
 match a{
  "permissions"|"permission_modes"=>{put(st,"dialog",s(a));}
  "permissions_save"=>save_settings(st,inputs)?,
  other=>set_mode(st,other.trim_start_matches("permission_mode:"))?,
 }
 Ok(())
}
