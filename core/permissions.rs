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
fn file_class(t:&str)->bool{matches!(t,"list"|"read"|"write"|"search"|"inspect"|"compare"|"reconstruct"|"capture")}
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

#[cfg(test)]
mod checks {
 use super::*;
 fn state() -> Json {
  let mut st = blank();
  put(&mut st, "profiles", Arr(vec![obj(vec![("name", s("MiniMax")), ("endpoint", s("http://127.0.0.1:9224/v1")), ("model", s("minimax"))]), obj(vec![("name", s("Home")), ("endpoint", s("http://127.0.0.1:11434")), ("model", s("llama"))])]));
  put(&mut st, "selected", Num("0".into()));new_chat(&mut st).unwrap();
  st
 }
 fn limit(st:&mut Json,connection:usize,class:&str,value:&str){
  let mut saved=get(st,"permissions").clone();if saved==Null{saved=obj(vec![]);}
  let mut row=get(&saved,&connection.to_string()).clone();if row==Null{row=obj(vec![]);}
  put(&mut row,class,s(value));put(&mut saved,&connection.to_string(),row);put(st,"permissions",saved);
 }
 fn proposal(tool:&str,args:Json)->Json{obj(vec![("tool",s(tool)),("args",args)])}
 #[test] fn default_mode_keeps_every_operation_on_the_review_path(){
  let st=state();
  assert_eq!(mode(&st),"review");assert_eq!(label(&st),"Ask");
  assert!(!stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
  assert!(!stands_down(&st,&proposal("read",obj(vec![("path",s("a.txt"))]))));
  assert!(check(&st,"web_search").is_ok());
 }
 #[test] fn yolo_stands_down_and_limited_spares_only_read_only_operations(){
  let mut st=state();set_mode(&mut st,"yolo").unwrap();
  assert!(stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
  set_mode(&mut st,"limited").unwrap();assert_eq!(label(&st),"Limited");
  assert!(stands_down(&st,&proposal("read",obj(vec![("path",s("a.txt"))]))));
  assert!(stands_down(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
  assert!(!stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
  assert!(!stands_down(&st,&proposal("capture",obj(vec![("url",s("http://127.0.0.1/x"))]))));
  assert!(set_mode(&mut st,"reckless").is_err());
 }
 #[test] fn web_search_never_asks_for_review_under_any_mode(){
  let mut st=state();
  for key in MODES{
   set_mode(&mut st,key).unwrap();
   assert!(stands_down(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
   assert!(stands_down(&st,&proposal("web_read",obj(vec![("url",s("https://example.test/page"))]))));
  }
  put(&mut st,"web",obj(vec![("search_without_review",Bool(false))]));
  assert!(!stands_down(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
  put(&mut st,"web",Null);
  limit(&mut st,0,"web","ask");
  assert!(!stands_down(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
  assert!(!stands_down(&st,&proposal("web_read",obj(vec![("url",s("https://example.test/page"))]))));
  assert!(stands_down(&st,&proposal("read",obj(vec![("path",s("a.txt"))]))));
 }
 #[test] fn per_connection_mode_defaults_to_the_harness_mode_and_can_be_tighter(){
  let mut st=state();set_mode(&mut st,"yolo").unwrap();
  assert!(stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
  limit(&mut st,0,"mode","review");
  assert!(!stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
  assert!(!stands_down(&st,&proposal("read",obj(vec![("path",s("a.txt"))]))));
  limit(&mut st,1,"mode","limited");
  put(&mut st,"selected",Num("1".into()));
  assert!(stands_down(&st,&proposal("read",obj(vec![("path",s("a.txt"))]))));
  assert!(!stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
 }
 #[test] fn per_connection_ask_and_off_target_only_the_named_classes(){
  let mut st=state();
  limit(&mut st,0,"web","ask");limit(&mut st,0,"file","off");
  assert!(requires_review(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
  assert!(!requires_review(&st,&proposal("read",obj(vec![("path",s("a.txt"))]))));
  assert!(check(&st,"web_read").is_ok());
  assert_eq!(check(&st,"write").err().unwrap(),"File access is off for MiniMax (App settings → Permissions)");
  assert!(check(&st,"model").is_ok());assert!(check(&st,"mcp").is_ok());
  set_mode(&mut st,"yolo").unwrap();
  assert!(!stands_down(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
  limit(&mut st,0,"web","off");
  assert_eq!(check(&st,"web_search").err().unwrap(),"Web access is off for MiniMax (App settings → Permissions)");
  assert_eq!(check(&st,"web_read").err().unwrap(),"Web access is off for MiniMax (App settings → Permissions)");
  put(&mut st,"selected",Num("1".into()));
  assert!(check(&st,"web_search").is_ok());assert!(check(&st,"list").is_ok());
  assert!(!requires_review(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
 }
 #[test] fn a_reviewed_operation_never_stands_down(){
  let mut st=state();set_mode(&mut st,"yolo").unwrap();
  assert!(!stands_down(&st,&obj(vec![("tool",s("write")),("args",obj(vec![("path",s("a.txt"))])),("format_review",Bool(true))])));
 }
 #[test] fn permission_settings_modal_carries_one_choice_per_connection_and_saves(){
  let st=state();let model=settings(&st);let fields=array(&model,"fields");
  assert_eq!(fields.len(),1+3*2);
  assert_eq!(get(&fields[0],"kind"),&s("choice"));
  let keys:Vec<String>=fields.iter().map(|f|text(f,"key")).collect();
  assert_eq!(keys,vec!["mode","mode_0","web_0","file_0","mode_1","web_1","file_1"]);
  let mut saved=st.clone();
  save_settings(&mut saved,&obj(vec![("mode",s("yolo")),("mode_0",s("limited")),("web_0",s("ask")),("file_0",s("inherit")),("mode_1",s("yolo")),("web_1",s("off")),("file_1",s("off"))])).unwrap();
  assert_eq!(mode(&saved),"yolo");
  assert_eq!(get(get(&saved,"permissions"),"0"),&obj(vec![("mode",s("limited")),("web",s("ask"))]));
  assert_eq!(get(get(&saved,"permissions"),"1"),&obj(vec![("mode",s("yolo")),("web",s("off")),("file",s("off"))]));
  assert!(get(&saved,"permissions").string().contains("\"0\""));
 }
 #[test] fn permission_settings_refuse_bad_choices_and_a_locked_session(){
  let st=state();
  assert!(save_settings(&mut st.clone(),&obj(vec![("mode",s("always"))])).is_err());
  assert!(save_settings(&mut st.clone(),&obj(vec![("mode",s("yolo")),("web_0",s("sometimes"))])).is_err());
  let mut busy=st.clone();put(&mut busy,"pending",proposal("write",obj(vec![("path",s("a.txt"))])));
  assert_eq!(save_settings(&mut busy,&obj(vec![("mode",s("yolo"))])).err().unwrap(),"Finish or cancel the current task or tool review first");
 }
 #[test] fn the_mode_menu_and_settings_entry_are_wired(){
  let st=state();let menu=dialog(&st);
  assert_eq!(array(&menu,"buttons").iter().map(|b|text(b,"event")).collect::<Vec<_>>(),vec!["permission_mode:review","permission_mode:limited","permission_mode:yolo"]);
  assert!(app_settings_menu().string().contains("\"permissions\""));
  for action in ["permissions","permission_modes","permission_mode:yolo","permissions_save"]{assert!(handles(action));}
  assert!(!handles("permissions_other"));
  let mut st=state();event(&mut st,"permission_mode:limited",&obj(vec![])).unwrap();
  assert_eq!(mode(&st),"limited");assert_eq!(get(&st,"dialog"),&Null);
 }
 #[test] fn an_unconfigured_connection_has_no_limits_to_apply(){
  let mut st=blank();
  assert_eq!(limits(&st),Null);assert!(check(&st,"web_search").is_ok());assert!(!requires_review(&st,&proposal("web_search",obj(vec![("query",s("now"))]))));
  set_mode(&mut st,"yolo").unwrap();assert!(stands_down(&st,&proposal("write",obj(vec![("path",s("a.txt"))]))));
 }
}
