//! Manually refreshed remote observations; no server reading is a phone measurement.
use super::*;
const LIMIT: usize = 524_288;
const PREFIX: &str = "server_metrics:";
fn finite(v:&Json)->Option<f64>{match v {Num(n)=>n.parse::<f64>().ok().filter(|n|n.is_finite()&&*n>=0.0),_=>None}}
fn number_value(v:&Json)->Json{finite(v).map(|n|Num(n.to_string())).unwrap_or(Null)}
fn bounded_string(v:&Json,max:usize)->Json{if let Str(s)=v{let mut n=s.len().min(max);while !s.is_char_boundary(n){n-=1;}Str(s[..n].chars().filter(|c|!c.is_control()||*c=='\n').collect())}else{Null}}
fn select(v:&Json,numeric:&[&str],strings:&[&str])->Json{let mut out=obj(vec![]);for key in numeric{put(&mut out,key,number_value(get(v,key)));}for key in strings{put(&mut out,key,bounded_string(get(v,key),256));}out}
fn summarize(body:&str)->Result<Json>{
 if body.len()>LIMIT{return Err("Server metrics exceed 512 KiB".into());}
 let raw=parse(body)?;
 if !matches!(raw,Obj(_)) || !["live","hardware","totals"].iter().any(|k|matches!(get(&raw,k),Obj(_))) {return Err("Expected metrics JSON containing live, hardware or totals objects".into());}
 let mut live=select(get(&raw,"live"),&["queued","waiting","admitting","running","parallel","outside_slots","tok_s","prefill_tok_s_mean","tok_s_mean","generated","prompt_read","prompt_total"],&["state","phase"]);
 let slots=array(get(&raw,"live"),"slots").into_iter().take(32).map(|v|select(&v,&["id","slot","prompt_tokens","generated","held_tokens","elapsed_s","tok_s"],&["state"])).collect();put(&mut live,"slots",Arr(slots));
 let mut hardware=select(get(&raw,"hardware"),&["cpu","ram_used","ram_total","gpu_mem_used","gpu_mem_total","gpu_power","gpu_pcie_rx_mb","gpu_pcie_tx_mb"],&[]);
 let gpus=array(get(&raw,"hardware"),"gpus").into_iter().take(16).map(|v|select(&v,&["index","util","temp","power","mem_used","mem_total"],&["name"])).collect();put(&mut hardware,"gpus",Arr(gpus));
 let requests=array(&raw,"requests");let kept=requests.into_iter().rev().take(64).collect::<Vec<_>>().into_iter().rev().map(|v|select(&v,&["time","duration_s","output_tokens","decode_tok_s","prompt_ms","decode_ms","hit_rate"],&["finish"])).collect();
 Ok(obj(vec![("live",live),("hardware",hardware),("requests",Arr(kept)),("totals",select(get(&raw,"totals"),&["requests","prompt_tokens","output_tokens","drafts_offered","drafts_accepted","reused"],&[]))]))
}
pub(super) fn handles(action:&str)->bool{matches!(action,"server_monitor"|"server_monitor_settings"|"server_monitor_save"|"server_monitor_refresh")||action.starts_with("server_monitor_panel:")}
pub(super) fn event(st:&mut Json,action:&str,inputs:&Json)->Result<Json>{
 match action {
  "server_monitor_settings"=>{put(st,"edit",get(st,"server_monitor").clone());put(st,"dialog",s(action));},
  "server_monitor_save"=>{
   let e=if get(inputs,"url")!=&Null{inputs}else{get(st,"edit")};let url=text(e,"url").trim().to_string();let key=text(e,"key");
   if url.len()>2048||key.len()>8192||key.chars().any(char::is_control){return Err("Metrics URL or credential exceeds its limit or contains control characters".into());}
   if !url.is_empty(){network::url(&url,url.starts_with("http://"))?;}
   put(st,"server_monitor",obj(vec![("url",s(&url)),("key",s(&key))]));
   interrupt(st,"Connection settings changed; retained sample belongs to its recorded source");if get(st,"server_monitor_sample")!=&Null{put(st,"server_monitor_error",s("Connection settings changed; retained sample belongs to its recorded source"));}put(st,"dialog",s("server_monitor"));
  },
  "server_monitor_refresh"=>{
   let config=get(st,"server_monitor");let url=text(config,"url");if url.is_empty(){return Err("Enter the server metrics JSON URL first".into());}
   let key=text(config,"key");let mut headers=vec![("Accept","application/json".into())];if !key.is_empty(){headers.push(("Authorization",format!("Bearer {key}")));}
   let mut e=network::wire(&url,url.starts_with("http://"),"GET","",headers)?;
   let purpose=format!("{PREFIX}{}",now());put(&mut e,"purpose",s(&purpose));put(&mut e,"limit",Num((LIMIT+65536).to_string()));
   put(st,"server_monitor_pending",obj(vec![("purpose",s(&purpose)),("url",s(&url))]));put(st,"busy",Bool(true));put(st,"dialog",s("server_monitor"));put(st,"notice",s("Reading server metrics…"));return Ok(e);
  },
  _=>{if let Some(panel)=action.strip_prefix("server_monitor_panel:"){if !["overview","hardware","flow","performance","history"].contains(&panel){return Err("Unknown server monitor panel".into());}put(st,"server_monitor_panel",s(panel));}put(st,"dialog",s("server_monitor"));}
 }Ok(Null)
}
pub(super) fn is_result(inputs:&Json)->bool{text(inputs,"purpose").starts_with(PREFIX)}
pub(super) fn interrupt(st:&mut Json,reason:&str){if get(st,"server_monitor_pending")!=&Null{put(st,"server_monitor_error",s(reason));}put(st,"server_monitor_pending",Null);}
pub(super) fn network_event(st:&mut Json,action:&str,inputs:&Json)->Result<Json>{
 let pending=get(st,"server_monitor_pending").clone();
 if pending==Null||text(&pending,"purpose")!=text(inputs,"purpose")||text(&pending,"url")!=text(get(st,"server_monitor"),"url"){return Ok(Null);}
 put(st,"server_monitor_pending",Null);put(st,"busy",Bool(false));
 let accepted=(||->Result<Json>{
  if action=="failure"{return Err("Server metrics request failed; previous sample retained".into());}
  let encoded=text(inputs,"data");if encoded.len()>(LIMIT+65536)*4/3+4{return Err("Server metrics response exceeds its limit".into());}
  let raw=network::unbase64(&encoded)?;let(status,body)=network::decoded(&raw)?;if status!=200{return Err(format!("Metrics returned HTTP {status}; previous sample retained"));}
  let mut sample=summarize(&body)?;put(&mut sample,"url",s(&text(&pending,"url")));put(&mut sample,"received_ns",s(&now()));Ok(sample)
 })();
 match accepted{
  Ok(sample)=>{
   let mut next=st.clone();let mut history=array(st,"server_monitor_history");
   history.push(obj(vec![("url",get(&sample,"url").clone()),("received_ns",get(&sample,"received_ns").clone()),("tok_s",get(get(&sample,"live"),"tok_s").clone()),("queued",get(get(&sample,"live"),"queued").clone()),("running",get(get(&sample,"live"),"running").clone()),("cpu",get(get(&sample,"hardware"),"cpu").clone())]));
   if history.len()>32{history.drain(..history.len()-32);}
   put(&mut next,"server_monitor_sample",sample);put(&mut next,"server_monitor_history",Arr(history));put(&mut next,"server_monitor_error",Null);
   if next.string().len()>6*1024*1024-4096{put(st,"server_monitor_error",s("Not enough saved-state capacity; previous server sample retained"));}
   else{*st=next;put(st,"notice",s("Server-reported metrics received; these are not independent device hardware measurements"));}
  },
  Err(error)=>{put(st,"server_monitor_error",s(&error));put(st,"notice",s(&error));}
 }
 Ok(Null)
}
fn shown(v:&Json,unit:&str)->String{finite(v).map(|n|format!("{n:.2}{unit}")).unwrap_or("Unavailable".into())}
fn val(row:&Json,key:&str,unit:&str)->String{shown(get(row,key),unit)}
fn words(row:&Json,key:&str)->String{let s=text(row,key);if s.is_empty(){"Unavailable".into()}else{s}}
fn stale(sample:&Json)->bool{match(text(sample,"received_ns").parse::<u128>(),now().parse::<u128>()){(Ok(t),Ok(n))=>n<t||n-t>60_000_000_000,_=>true}}
fn age(sample:&Json)->String{let timestamp=text(sample,"received_ns").parse::<u128>().ok();let clock=now().parse::<u128>().ok();match(timestamp,clock){(Some(t),Some(n))if n>=t=>format!("{:.1} s",(n-t)as f64/1e9),_=>"Unavailable".into()}}
fn quantile(rows:&[Json],key:&str,p:usize)->String{let mut values=rows.iter().filter_map(|r|finite(get(r,key))).collect::<Vec<_>>();if values.is_empty(){return "Unavailable".into();}values.sort_by(|a,b|a.total_cmp(b));format!("{:.3} s",values[(values.len()*p+99)/100-1])}
fn row(key:&str,title:&str,value:String)->Json{field_ui(key,title,&value,"readonly")}
fn local_record(st:&Json)->Vec<Json>{
 let rows=array(st,"requests");
 let completed=rows.iter().filter(|r|text(r,"outcome")=="success").count();
 vec![
  row("basis","Calculated here","Elapsed time, first text, character counts, and character÷4 estimates come from harness requests this app completed. A token count is kept only when that count was already inside the harness reply. These local counts stay beside the server-reported sample.".into()),
  row("count","Retained requests",format!("{} retained · {completed} completed",rows.len())),
 ]
}
pub(super) fn modal(st:&Json)->Json{
 if text(st,"dialog")=="server_monitor_settings"{
  let e=get(st,"edit");
  let mut fields=vec![field_ui("url","Exact metrics JSON URL",&text(e,"url"),"uri"),field_ui("key","Separate bearer key (optional)",&text(e,"key"),"secret"),field_ui("scope","Source","Enter the direct server /metrics URL or your monitor's /monitor/metrics URL. Refresh is manual. Model, search and tool keys are never reused. Server readings are reported values, not device measurements.","readonly")];
  fields.extend(local_record(st));
  return super::modal("Server monitor connection",fields,vec![button("Save monitor connection","server_monitor_save")]);
 }
 let sample=get(st,"server_monitor_sample");let live=get(sample,"live");let hw=get(sample,"hardware");let config=get(st,"server_monitor");
 let mut fields=vec![row("source","Source and freshness",format!("Configured: {}\nRecorded source: {}\nSample age: {}\n{}\nManual snapshot; values may have changed. All values below are reported by the server.",endpoint_summary(&text(config,"url")),endpoint_summary(&text(sample,"url")),age(sample),if !text(st,"server_monitor_error").is_empty(){text(st,"server_monitor_error")}else if sample==&Null{"No sample received".into()}else if text(config,"url")!=text(sample,"url"){"STALE: source changed".into()}else if stale(sample){"STALE: more than 60 seconds old or clock unavailable; refresh for current values".into()}else{"Last received sample; refresh for current values".into()}))];
 fields.extend(local_record(st));
 match text(st,"server_monitor_panel").as_str(){
  "hardware"=>{
   fields.push(row("host","Server hardware",format!("CPU: {}\nRAM used / total: {} / {}\nGPU memory used / total: {} / {}\nGPU power: {}",val(hw,"cpu"," %"),val(hw,"ram_used"," bytes"),val(hw,"ram_total"," bytes"),val(hw,"gpu_mem_used"," bytes"),val(hw,"gpu_mem_total"," bytes"),val(hw,"gpu_power"," W"))));
   for(i,g)in array(hw,"gpus").iter().enumerate(){fields.push(row(&format!("gpu_{i}"),&format!("GPU {} · {}",val(g,"index",""),words(g,"name")),format!("Utilization: {}\nTemperature: {}\nPower: {}\nMemory used / total: {} / {}",val(g,"util"," %"),val(g,"temp"," °C"),val(g,"power"," W"),val(g,"mem_used"," bytes"),val(g,"mem_total"," bytes"))));}
  },
  "flow"=>{
   fields.push(row("queue","Request flow",format!("Queued: {}\nWaiting: {}\nAdmitting: {}\nRunning: {}\nParallel: {}\nOutside slots: {}",val(live,"queued",""),val(live,"waiting",""),val(live,"admitting",""),val(live,"running",""),val(live,"parallel",""),val(live,"outside_slots",""))));
   for(i,slot)in array(live,"slots").iter().enumerate(){fields.push(row(&format!("slot_{i}"),&format!("Reported slot {i}"),format!("State: {}\nPrompt tokens: {}\nGenerated tokens: {}\nHeld tokens: {}\nElapsed: {}\nReported rate: {}",words(slot,"state"),val(slot,"prompt_tokens",""),val(slot,"generated",""),val(slot,"held_tokens",""),val(slot,"elapsed_s"," s"),val(slot,"tok_s"," tokens/s"))));}
  },
  "performance"=>{let requests=array(sample,"requests");fields.push(row("performance","Server performance",format!("Reported decode: {}\nReported prefill mean: {}\nReported mean token rate: {}\nRecent duration p50 / p95: {} / {}\n{} retained request records; nearest-rank percentiles use valid server durations only.",val(live,"tok_s"," tokens/s"),val(live,"prefill_tok_s_mean"," tokens/s"),val(live,"tok_s_mean"," tokens/s"),quantile(&requests,"duration_s",50),quantile(&requests,"duration_s",95),requests.len())));},
  "history"=>{for(i,h)in array(st,"server_monitor_history").iter().rev().enumerate(){fields.push(row(&format!("sample_{i}"),&format!("Manual sample · {} old",age(h)),format!("{}\nReported decode: {}\nQueued: {}; running: {}; CPU: {}",endpoint_summary(&text(h,"url")),val(h,"tok_s"," tokens/s"),val(h,"queued",""),val(h,"running",""),val(h,"cpu"," %"))));}},
  _=>{fields.push(row("overview","Server overview",format!("State: {}\nPhase: {}\nRunning: {}\nQueued: {}\nReported decode: {}\nCompleted requests: {}",words(live,"state"),words(live,"phase"),val(live,"running",""),val(live,"queued",""),val(live,"tok_s"," tokens/s"),val(get(sample,"totals"),"requests",""))));}
 }
 super::modal("Server monitor",fields,vec![button("Refresh now","server_monitor_refresh"),button("Connection","server_monitor_settings"),button("Overview","server_monitor_panel:overview"),button("GPU and memory","server_monitor_panel:hardware"),button("Request flow","server_monitor_panel:flow"),button("Performance","server_monitor_panel:performance"),button("Sample history","server_monitor_panel:history")])
}
#[cfg(any(test, rllm_module_tests))]
mod checks{
 use super::*;
 fn field_value(view:&Json,key:&str)->String{array(view,"fields").into_iter().find(|f|text(f,"key")==key).map(|f|text(&f,"value")).unwrap_or_default()}
 fn field_label(view:&Json,key:&str)->String{array(view,"fields").into_iter().find(|f|text(f,"key")==key).map(|f|text(&f,"label")).unwrap_or_default()}
 #[cfg_attr(not(rllm_module_tests), test)]
 fn refresh_returns_a_request_and_the_dialog_keeps_server_views_and_local_counts(){
  let mut st=obj(vec![
   ("server_monitor",obj(vec![("url",s("http://metrics.test/metrics")),("key",s("monitor-only"))])),
   ("requests",Arr(vec![obj(vec![("outcome",s("success"))]),obj(vec![("outcome",s("stopped"))])])),
  ]);
  let effect=event(&mut st,"server_monitor_refresh",&Null).unwrap();
  assert_ne!(effect,Null);
  assert_eq!(text(&effect,"kind"),"network");
  assert!(text(&effect,"purpose").starts_with(PREFIX));
  assert!(text(&effect,"wire").starts_with("GET "));
  assert!(text(&effect,"wire").contains("metrics.test"));
  assert!(text(&effect,"wire").contains("monitor-only"));
  assert_eq!(text(&st,"dialog"),"server_monitor");
  let view=modal(&st);
  assert_eq!(text(&view,"title"),"Server monitor");
  let buttons:Vec<String>=array(&view,"buttons").iter().map(|b|text(b,"event")).collect();
  for name in ["server_monitor_refresh","server_monitor_panel:overview","server_monitor_panel:hardware","server_monitor_panel:flow","server_monitor_panel:performance","server_monitor_panel:history"]{
   assert!(buttons.iter().any(|event|event==name),"{name}");
  }
  assert_eq!(field_value(&view,"count"),"2 retained · 1 completed");
  assert!(field_value(&view,"basis").contains("harness requests"));
  assert!(field_value(&view,"overview").contains("Reported decode"));
  event(&mut st,"server_monitor_panel:hardware",&Null).unwrap();
  let hardware=modal(&st);
  assert!(field_value(&hardware,"host").contains("GPU memory"));
  assert_eq!(field_value(&hardware,"count"),"2 retained · 1 completed");
  event(&mut st,"server_monitor_panel:flow",&Null).unwrap();
  assert!(field_value(&modal(&st),"queue").contains("Outside slots"));
  event(&mut st,"server_monitor_panel:performance",&Null).unwrap();
  let performance=field_value(&modal(&st),"performance");
  assert!(performance.contains("p50"));
  assert!(performance.contains("p95"));
  event(&mut st,"server_monitor_panel:history",&Null).unwrap();
  assert_eq!(text(&st,"server_monitor_panel"),"history");
  assert_eq!(field_value(&modal(&st),"count"),"2 retained · 1 completed");
  let body=r#"{"live":{"state":"busy","phase":"decode","queued":1,"running":2,"tok_s":3.5},"hardware":{"cpu":11,"gpus":[{"index":0,"name":"adapter","util":10}]},"totals":{"requests":9},"requests":[{"duration_s":1.5}]}"#;
  let wire=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",body.len());
  network_event(&mut st,"network_result",&obj(vec![("purpose",get(&effect,"purpose").clone()),("data",s(&network::b64(wire.as_bytes())))])).unwrap();
  let history=modal(&st);
  assert!(field_value(&history,"sample_0").contains("metrics.test"));
  assert_eq!(field_value(&history,"count"),"2 retained · 1 completed");
  event(&mut st,"server_monitor_panel:hardware",&Null).unwrap();
  assert!(field_label(&modal(&st),"gpu_0").contains("adapter"));
  event(&mut st,"server_monitor_panel:overview",&Null).unwrap();
  assert!(field_value(&modal(&st),"overview").contains("busy"));
  event(&mut st,"server_monitor_panel:performance",&Null).unwrap();
  assert!(field_value(&modal(&st),"performance").contains("1 retained request"));
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn missing_and_invalid_measurements_are_not_zero(){let s=summarize(r#"{"live":{"tok_s":0,"queued":-1},"hardware":{"cpu":"40","ram_used":1e999}}"#).unwrap();assert_eq!(get(get(&s,"live"),"tok_s"),&Num("0".into()));assert_eq!(get(get(&s,"live"),"queued"),&Null);assert_eq!(get(get(&s,"hardware"),"cpu"),&Null);assert_eq!(get(get(&s,"hardware"),"ram_used"),&Null);assert!(summarize("{}").is_err());}
 #[cfg_attr(not(rllm_module_tests), test)]
 fn shape_and_history_inputs_are_bounded(){let raw=obj(vec![("live",obj(vec![("slots",Arr(vec![obj(vec![]);200]))])),("hardware",obj(vec![("gpus",Arr(vec![obj(vec![]);200]))])),("requests",Arr(vec![obj(vec![("duration_s",Num("1".into()))]);200]))]);let sample=summarize(&raw.string()).unwrap();assert_eq!(array(get(&sample,"live"),"slots").len(),32);assert_eq!(array(get(&sample,"hardware"),"gpus").len(),16);assert_eq!(array(&sample,"requests").len(),64);assert_eq!(quantile(&array(&sample,"requests"),"duration_s",95),"1.000 s");}
 #[cfg_attr(not(rllm_module_tests), test)]
 fn credentials_and_callback_purpose_are_isolated(){let mut st=obj(vec![("server_monitor",obj(vec![("url",s("http://metrics.test/metrics")),("key",s("monitor-only"))])),("profiles",Arr(vec![obj(vec![("key",s("model-secret"))])]))]);let effect=event(&mut st,"server_monitor_refresh",&Null).unwrap();assert!(text(&effect,"wire").contains("monitor-only"));assert!(!text(&effect,"wire").contains("model-secret"));let unchanged=st.clone();network_event(&mut st,"failure",&obj(vec![("purpose",s("server_metrics:obsolete"))])).unwrap();assert_eq!(st,unchanged);let purpose=text(&effect,"purpose");network_event(&mut st,"failure",&obj(vec![("purpose",s(&purpose))])).unwrap();assert_eq!(get(&st,"server_monitor_pending"),&Null);assert!(!boolean(&st,"busy"));}
 #[cfg_attr(not(rllm_module_tests), test)]
 fn exact_source_success_retention_and_manual_history_limit(){
  let mut st=obj(vec![("server_monitor",obj(vec![("url",s("http://metrics.test/metrics"))]))]);
  let body=r#"{"live":{"tok_s":0,"queued":2},"hardware":{"cpu":0},"requests":[{"duration_s":2},{"duration_s":10}]}"#;
  let wire=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",body.len());
  for _ in 0..35{let effect=event(&mut st,"server_monitor_refresh",&Null).unwrap();network_event(&mut st,"network_result",&obj(vec![("purpose",get(&effect,"purpose").clone()),("data",s(&network::b64(wire.as_bytes())))])).unwrap();}
  assert_eq!(array(&st,"server_monitor_history").len(),32);let saved=get(&st,"server_monitor_sample").clone();assert_eq!(text(&saved,"url"),"http://metrics.test/metrics");assert_eq!(get(get(&saved,"live"),"tok_s"),&Num("0".into()));
  let effect=event(&mut st,"server_monitor_refresh",&Null).unwrap();let old=get(&effect,"purpose").clone();
  event(&mut st,"server_monitor_save",&obj(vec![("url",s("http://different.test/metrics")),("key",s("new-secret"))])).unwrap();
  let before=st.clone();network_event(&mut st,"network_result",&obj(vec![("purpose",old),("data",s(&network::b64(wire.as_bytes())))])).unwrap();assert_eq!(st,before);assert_eq!(get(&st,"server_monitor_sample"),&saved);
  let e=event(&mut st,"server_monitor_refresh",&Null).unwrap();let failed="HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nshorter";
  network_event(&mut st,"network_result",&obj(vec![("purpose",get(&e,"purpose").clone()),("data",s(&network::b64(failed.as_bytes())))])).unwrap();assert_eq!(get(&st,"server_monitor_sample"),&saved);assert!(!text(&st,"server_monitor_error").is_empty());
 }
 #[cfg_attr(not(rllm_module_tests), test)]
 fn unrelated_cancellation_does_not_change_sample_status(){
  let mut st=obj(vec![("server_monitor_sample",obj(vec![("url",s("http://metrics.test/metrics"))]))]);
  interrupt(&mut st,"Metrics cancelled");assert_eq!(get(&st,"server_monitor_error"),&Null);
  put(&mut st,"server_monitor_pending",obj(vec![("purpose",s("server_metrics:pending"))]));
  interrupt(&mut st,"Metrics cancelled");assert_eq!(text(&st,"server_monitor_error"),"Metrics cancelled");assert_eq!(get(&st,"server_monitor_pending"),&Null);
 }
 #[cfg(rllm_module_tests)]
 #[no_mangle]
 pub extern "C" fn x2_remote_monitor_tests(){
  refresh_returns_a_request_and_the_dialog_keeps_server_views_and_local_counts();
  missing_and_invalid_measurements_are_not_zero();
  shape_and_history_inputs_are_bounded();
  credentials_and_callback_purpose_are_isolated();
  exact_source_success_retention_and_manual_history_limit();
  unrelated_cancellation_does_not_change_sample_status();
 }
}
