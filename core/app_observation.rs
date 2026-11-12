//! Explicit, read-only package observations; no model or tool authority.
use super::*;
use std::sync::atomic::{AtomicU64,Ordering};
static SERIAL:AtomicU64=AtomicU64::new(1);
fn enabled(st:&Json)->bool{get(st,"apps_enabled")!=&Bool(false)}
fn package(v:&str)->bool{!v.is_empty()&&v.len()<=255&&v.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'.'||c==b'_')}
fn excluded(st:&Json,p:&str)->bool{array(st,"apps_excluded").iter().any(|v|v==&s(p))}
fn label(v:&str)->String{v.chars().map(|c|if c.is_control()||matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'){ '\u{fffd}'}else{c}).collect()}
pub(super) fn handles(a:&str)->bool{matches!(a,"apps_view"|"apps_catalog"|"apps_result"|"apps_disable"|"apps_enable"|"apps_exclude"|"apps_include"|"apps_refresh"|"apps_usage_enable"|"apps_usage_disable"|"apps_usage_settings"|"apps_usage_day"|"apps_usage_week")||a.starts_with("app_pick:")||a.starts_with("apps_page:")}
pub(super) fn expected(st:&Json,i:&Json)->bool{
 let e=get(st,"apps_expected");enabled(st)&&boolean(st,"busy")&&!text(e,"ticket").is_empty()&&text(e,"ticket")==text(i,"ticket")&&text(e,"package")==text(i,"package")&&(!excluded(st,&text(e,"package"))) && (text(e,"operation")!="usage" || (boolean(st,"apps_usage_enabled") && text(st,"apps_selected")==text(e,"package") && text(i,"operation")=="usage" && text(i,"begin_ms")==text(e,"begin_ms") && text(i,"end_ms")==text(e,"end_ms")))
}
pub(super) fn interrupt(st:&mut Json){put(st,"apps_expected",Null);}
pub(super) fn controls(a:&str)->bool{matches!(a,"apps_disable"|"apps_usage_disable"|"apps_exclude")||a.starts_with("app_pick:")}
fn cancel(st:&mut Json)->Json{let pending=get(st,"apps_expected")!=&Null;interrupt(st);if pending{put(st,"busy",Bool(false));obj(vec![("kind",s("cancel"))])}else{Null}}
fn millis(v:&Json,k:&str)->Result<u64>{let t=string(v,k,20)?;if t.is_empty()||!t.bytes().all(|c|c.is_ascii_digit()){return Err(format!("Invalid usage {k}"));}let n=t.parse::<u64>().map_err(|_|format!("Invalid usage {k}"))?;if n>i64::MAX as u64{return Err(format!("Invalid usage {k}"));}Ok(n)}
fn usage_start(st:&mut Json,i:&Json,window:u64,p:&str)->Result<Json>{
 if !boolean(st,"apps_usage_enabled"){return Err("Enable usage observations before reading selected app usage".into());}
 let end=millis(i,"usage_now_ms")?;let begin=end.checked_sub(window).ok_or("System time cannot represent this usage window")?;
 let mut next=st.clone();let mut effect=start(&mut next,"usage",p)?;
 put(&mut effect,"begin_ms",s(&begin.to_string()));put(&mut effect,"end_ms",s(&end.to_string()));
 put(&mut next,"apps_expected",effect.clone());admission(&next)?;*st=next;Ok(effect)
}
fn usage(v:&Json,e:&Json)->Result<Json>{
 let p=text(e,"package");if string(v,"package",255)?!=p{return Err("Usage observation belongs to another package".into());}
 let begin=millis(v,"begin_ms")?;let end=millis(v,"end_ms")?;let observed=millis(v,"observed_ms")?;
 if text(v,"begin_ms")!=text(e,"begin_ms")||text(v,"end_ms")!=text(e,"end_ms")||end<=begin||!matches!(end-begin,86_400_000|604_800_000){return Err("Usage observation has unexpected time bounds".into());}
 let mut out=obj(vec![("package",s(&p)),("begin_ms",s(&begin.to_string())),("end_ms",s(&end.to_string())),("observed_ms",s(&observed.to_string()))]);let mut total=0u64;
 for key in ["resumed","paused","service_started","service_stopped"]{let n=match get(v,key){Num(n) if !n.is_empty()&&n.bytes().all(|c|c.is_ascii_digit())=>n.parse::<u64>().map_err(|_|"Invalid usage count")?,_=>return Err("Invalid usage count".into())};total=total.checked_add(n).ok_or("Usage count overflow")?;if total>10000{return Err("Usage observation exceeds 10000 retained events; no partial result accepted".into());}put(&mut out,key,Num(n.to_string()));}
 Ok(out)
}
fn admission(next:&Json)->Result<()> {
 let bytes=super::super::mcp_schema::state_bytes(next);
 if bytes>6*1024*1024-8192{return Err("Encrypted history has insufficient room for this app observation; existing data retained".into());}
 Ok(())
}
fn start(st:&mut Json,kind:&str,p:&str)->Result<Json>{
 if !enabled(st){return Err("App inspection is disabled".into());}
 if !p.is_empty()&&excluded(st,p){return Err("This app is excluded from detail and usage inspection".into());}
 let n=SERIAL.try_update(Ordering::SeqCst,Ordering::SeqCst,|n|n.checked_add(1)).map_err(|_|"App observation ticket space exhausted")?;
 let e=obj(vec![("kind",s("app_observation")),("operation",s(kind)),("package",s(p)),("ticket",s(&format!("apps-{n}")))]);
 let mut next=st.clone();put(&mut next,"apps_expected",e.clone());put(&mut next,"busy",Bool(true));put(&mut next,"dialog",s("apps"));admission(&next)?;*st=next;Ok(e)
}
fn string(v:&Json,k:&str,max:usize)->Result<String>{match get(v,k){Str(s) if s.len()<=max=>Ok(s.clone()),_=>Err(format!("Invalid app observation {k}"))}}
fn catalog(v:&Json)->Result<Json>{
 let entries=match get(v,"entries"){Arr(a) if a.len()<=1024=>a,_=>return Err("App catalog exceeds 1024 entries or has invalid shape".into())};
 let mut out=Vec::new();let mut seen=std::collections::BTreeSet::new();
 for e in entries{let p=string(e,"package",255)?;let name=string(e,"label",256)?;
  if !package(&p)||!seen.insert(p.clone())||!matches!(get(e,"system"),Bool(_)){return Err("Invalid or duplicate app catalog identity".into());}
  out.push(obj(vec![("package",s(&p)),("label",s(&name)),("system",get(e,"system").clone())]));
 }
 out.sort_by_key(|e|(label(&text(e,"label")).to_lowercase(),text(e,"package")));Ok(Arr(out))
}
fn details(v:&Json,p:&str)->Result<Json>{
 if string(v,"package",255)?!=p{return Err("App observation belongs to another package".into());}
 let version=string(v,"version",256)?;let code=string(v,"version_code",32)?;
 if code.is_empty()||!code.bytes().all(|c|c.is_ascii_digit())||!matches!(get(v,"uid"),Num(n) if n.parse::<u32>().is_ok())||!matches!(get(v,"system"),Bool(_)){return Err("Invalid app version or UID".into());}
 let perms=match get(v,"permissions"){Arr(a) if a.len()<=512=>a,_=>return Err("Permission observation exceeds 512 declarations or has invalid shape".into())};
 let mut out=Vec::new();let mut seen=std::collections::BTreeSet::new();
 for perm in perms{let name=string(perm,"name",512)?;if name.is_empty()||!seen.insert(name.clone())||!matches!(get(perm,"granted"),Bool(_)|Null){return Err("Invalid permission observation".into());}out.push(obj(vec![("name",s(&name)),("granted",get(perm,"granted").clone())]));}
 out.sort_by_key(|v|text(v,"name"));
 let observed=string(v,"observed_ms",32)?;if observed.parse::<u64>().is_err(){return Err("Invalid observation timestamp".into());}
 Ok(obj(vec![("package",s(p)),("version",s(&version)),("version_code",s(&code)),("uid",get(v,"uid").clone()),("system",get(v,"system").clone()),("observed_ms",s(&observed)),("permissions",Arr(out))]))
}
pub(super) fn event(st:&mut Json,a:&str,i:&Json)->Result<Json>{
 if a=="apps_result"{
  let e=get(st,"apps_expected").clone();put(st,"apps_expected",Null);put(st,"busy",Bool(false));
  if let Str(error)=get(i,"error"){put(st,"notice",s(&format!("App observation failed: {}",label(&error.chars().take(512).collect::<String>()))));return Ok(Null);}
  let data=get(i,"data");
  let bytes=super::super::mcp_schema::state_bytes(st);let policy=super::super::resources::policy(get(st,"device"),get(st,"resources"),bytes);
  if data.string().len()>number(&policy,"response_bytes") as usize{return Err("Current memory budget cannot adopt this app observation; existing data retained".into());}
  let mut next=st.clone();
  if text(&e,"operation")=="catalog"{let c=catalog(data)?;put(&mut next,"apps_catalog",c);put(&mut next,"apps_page",Num("0".into()));}
  else if text(&e,"operation")=="usage"{let d=usage(data,&e)?;put(&mut next,"apps_usage",d);}else{let d=details(data,&text(&e,"package"))?;put(&mut next,"apps_details",d);}
  put(&mut next,"notice",s(if text(&e,"operation")=="usage"{"Android-retained lifecycle records; history may be incomplete. These counts are not launches, duration, performance or a spyware verdict. Nothing was sent to an LLM."}else{"Read-only Android package observation. No app content was read or sent to an LLM."}));admission(&next)?;*st=next;return Ok(Null);
 }
 if a=="apps_view"{put(st,"dialog",s("apps"));return Ok(Null);}
 if a=="apps_disable"{let pending=get(st,"apps_expected")!=&Null;put(st,"apps_expected",Null);if pending{put(st,"busy",Bool(false));}put(st,"apps_enabled",Bool(false));put(st,"apps_usage_enabled",Bool(false));put(st,"apps_usage",Null);put(st,"apps_catalog",Null);put(st,"apps_details",Null);put(st,"apps_selected",Null);put(st,"dialog",s("apps"));put(st,"notice",s("App inspection disabled and current observations cleared; this does not revoke independent Android or file permissions."));return Ok(if pending{obj(vec![("kind",s("cancel"))])}else{Null});}
 if a=="apps_enable"{put(st,"apps_enabled",Bool(true));put(st,"dialog",s("apps"));return Ok(Null);}
 if !enabled(st){return Err("App inspection is disabled".into());}
 if a=="apps_usage_enable"{put(st,"apps_usage_enabled",Bool(true));put(st,"notice",s("Usage observations enabled. Android usage access and a separate explicit read are still required. No collection starts automatically."));return Ok(Null);}
 if a=="apps_usage_disable"{let effect=if text(get(st,"apps_expected"),"operation")=="usage"{cancel(st)}else{Null};put(st,"apps_usage_enabled",Bool(false));put(st,"apps_usage",Null);put(st,"notice",s("Usage observations disabled and cleared. Android's separate usage-access grant is unchanged. Already-running system queries cannot be forcibly stopped; late results are discarded."));return Ok(effect);}
 if a=="apps_usage_settings"{return Ok(obj(vec![("kind",s("app_usage_settings"))]));}
 if a=="apps_catalog"{return start(st,"catalog","");}
 if let Some(index)=a.strip_prefix("apps_page:"){let page=index.parse::<usize>().map_err(|_|"Invalid app catalog page")?;if page%20!=0||page>=array(st,"apps_catalog").len(){return Err("Invalid app catalog page".into());}put(st,"apps_page",Num(page.to_string()));return Ok(Null);}
 if let Some(p)=a.strip_prefix("app_pick:"){
  if !package(p)||!array(st,"apps_catalog").iter().any(|e|text(e,"package")==p){return Err("Select an app from the current catalog".into());}
  let effect=cancel(st);put(st,"apps_selected",s(p));put(st,"apps_details",Null);put(st,"apps_usage",Null);
  put(st,"dialog",s("apps"));return Ok(effect);
 }
 let p=text(st,"apps_selected");if !package(&p){return Err("Select an app first".into());}
 if a=="apps_exclude"{let mut list=array(st,"apps_excluded");if !excluded(st,&p){if list.len()>=1024{return Err("App exclusion list exceeds 1024 entries".into());}list.push(s(&p));}let effect=cancel(st);put(st,"apps_excluded",Arr(list));put(st,"apps_details",Null);put(st,"apps_usage",Null);return Ok(effect);}
 if a=="apps_include"{let list=array(st,"apps_excluded").into_iter().filter(|v|v!=&s(&p)).collect();put(st,"apps_excluded",Arr(list));return Ok(Null);}
 if a=="apps_usage_day"{return usage_start(st,i,86_400_000,&p);}if a=="apps_usage_week"{return usage_start(st,i,604_800_000,&p);}
 if a=="apps_refresh"{return start(st,"details",&p);}Err("Unknown app observation action".into())
}
pub(super) fn modal_for(st:&Json)->Json{
 let mut fields=vec![field_ui("scope","Inspection scope",if enabled(st){"Enabled; collection runs only when you request the catalog or read/refresh one selected app. Current Android user; metadata and separately enabled, explicitly requested usage observations."}else{"Disabled; no app metadata requests. Existing exclusions are retained."},"readonly"),field_ui("limits","Observation limits","Labels are app-supplied, untrusted text. Permission grants do not prove use, effective AppOps access or spying. No private files, traffic, passwords or app content are collected. Optional usage reads collect only selected-package lifecycle counts. Results stay in RLLM state; they are not automatically shared with a model. Exclusions prevent details and usage reads, not basic catalog identities. Shared UIDs are not isolation.","readonly")];
 let mut buttons=vec![button(if enabled(st){"Disable inspection and clear observations"}else{"Enable app inspection"},if enabled(st){"apps_disable"}else{"apps_enable"})];
 if enabled(st){
  fields.push(field_ui("usage_scope","Usage observations",if boolean(st,"apps_usage_enabled"){"Enabled for explicit selected-app reads only. Requires Android 15 or later and Android usage access. Exact package-filtered query; no older all-app query fallback. 24 hours / 7 days are rolling query windows, not guaranteed history retention. At most 10000 events and 2 seconds of iteration; Android's initial system query allocation/time are outside these bounds. Cancellation discards late results but cannot forcibly stop a system query."}else{"Disabled. Enable separately to read retained lifecycle event counts for one selected, nonexcluded app. No automatic collection or LLM sharing."},"readonly"));
  buttons.push(button(if boolean(st,"apps_usage_enabled"){"Disable usage and clear observation"}else{"Enable usage observations"},if boolean(st,"apps_usage_enabled"){"apps_usage_disable"}else{"apps_usage_enable"}));
  buttons.push(button("Open Android usage access settings","apps_usage_settings"));
  buttons.push(button("Read / refresh app catalog","apps_catalog"));let p=text(st,"apps_selected");if !p.is_empty(){fields.push(field_ui("package","Selected package",&p,"readonly"));buttons.push(button(if excluded(st,&p){"Include this app's observations"}else{"Exclude this app's observations"},if excluded(st,&p){"apps_include"}else{"apps_exclude"}));if !excluded(st,&p){buttons.push(button("Read / refresh selected app details","apps_refresh"));if boolean(st,"apps_usage_enabled"){buttons.push(button("Read selected app usage · last 24 hours","apps_usage_day"));buttons.push(button("Read selected app usage · last 7 days","apps_usage_week"));}}}
  let d=get(st,"apps_details");if d!=&Null{let mut report=format!("Android PackageManager observation, timestamp {} ms since Unix epoch\nPackage: {}\nVersion: {} ({})\nUID: {}\nSystem app: {}\nRequested permissions:\n",text(d,"observed_ms"),text(d,"package"),label(&text(d,"version")),text(d,"version_code"),get(d,"uid").string(),boolean(d,"system"));for e in array(d,"permissions"){report.push_str(&format!("{} — {}\n",label(&text(&e,"name")),match get(&e,"granted"){Bool(true)=>"granted",Bool(false)=>"not granted",_=>"unknown"}));}fields.push(field_ui("observation","Read-only observation",&report,"readonly"));}
  let u=get(st,"apps_usage");if u!=&Null{fields.push(field_ui("usage_observation","Retained lifecycle event counts",&format!("Package: {}\nQuery range [{} , {}) ms since Unix epoch\nObserved: {} ms since Unix epoch\nActivity resumes: {}\nActivity pauses: {}\nForeground-service starts: {}\nForeground-service stops: {}\nAndroid-retained events; history may be incomplete. Zero means no matching retained records returned, not no usage. Not launch counts, duration, CPU/network performance or a spyware verdict.",text(u,"package"),text(u,"begin_ms"),text(u,"end_ms"),text(u,"observed_ms"),get(u,"resumed").string(),get(u,"paused").string(),get(u,"service_started").string(),get(u,"service_stopped").string()),"readonly"));}
  let apps=array(st,"apps_catalog");let offset=number(st,"apps_page").max(0) as usize;for e in apps.iter().skip(offset).take(20){let p=text(e,"package");buttons.push(button(&format!("{}\n{}{}",label(&text(e,"label")),p,if excluded(st,&p){" · observations excluded"}else{""}),&format!("app_pick:{p}")));}
  if offset>=20{buttons.push(button("Previous apps",&format!("apps_page:{}",offset-20)));}if offset+20<apps.len(){buttons.push(button("More apps",&format!("apps_page:{}",offset+20)));}
 }
 let mut value=modal("App inspection",fields,buttons);put(&mut value,"presentation",s("list"));value
}
