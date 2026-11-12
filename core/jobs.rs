//! Server job identities survive restart; submission is never a status request.
use super::*;
fn config(st:&Json)->&Json{get(st,"service_connections")}
fn route(st:&Json,id:&str)->Result<Json>{array(config(st),"routes").into_iter().find(|r|text(r,"id")==id).ok_or("Job route is absent".into())}
fn origin(st:&Json,r:&Json)->Result<String>{let h=array(config(st),"hosts").into_iter().find(|h|text(h,"id")==text(r,"host")).ok_or("Job host is absent")?;let u=network::url(&text(&h,"url"),boolean(&h,"plain"))?;Ok(format!("{}://{}:{}",if u.tls{"https"}else{"http"},u.host.to_ascii_lowercase(),u.port))}
pub(super) fn acceptance(r:&Json,code:u16)->bool{let codes=get(get(r,"job"),"accept_codes");if codes==&Null{code==202}else{array(get(r,"job"),"accept_codes").contains(&Num(code.to_string()))}}
fn scalar(v:&Json)->bool{matches!(v,Str(t)if !t.is_empty()&&t.len()<=128)||matches!(v,Bool(_)|Num(_))}
pub(super) fn validate_config(cfg:&Json)->Result<()>{
    for r in array(cfg,"routes"){
        let job=get(&r,"job");if job==&Null{continue;}
        if !matches!(job,Obj(_))||!text(job,"id").starts_with('/')||!text(job,"state").starts_with('/'){return Err("Job mapping requires id and state JSON pointers".into());}
        let mut terminals=std::collections::BTreeSet::new();for field in ["success","failure"]{let values=get(job,field).array()?;if values.is_empty()||values.len()>16{return Err("Job mapping requires bounded success and failure states".into());}for value in values{if !scalar(value)||!terminals.insert(value.string()){return Err("Job terminal states must be distinct, bounded strings, numbers or booleans".into());}}}
        if get(job,"accept_codes")!=&Null{let codes=get(job,"accept_codes").array()?;if codes.is_empty()||codes.len()>4||codes.iter().any(|v|!matches!(v,Num(n)if n.parse::<u16>().is_ok_and(|n|(200..=299).contains(&n)))){return Err("Job acceptance codes require one to four explicit HTTP success statuses".into());}}
        for key in ["status_input","result_input","cancel_input"]{if get(job,key)!=&Null&&!matches!(get(job,key),Obj(_)){return Err("Job follow-up input templates must be objects".into());}}
        for key in ["status_route","result_route","cancel_route"]{let id=text(job,key);if id.is_empty()&&key=="cancel_route"{continue;}let target=array(cfg,"routes").into_iter().find(|r|text(r,"id")==id).ok_or("Job status/result/cancel route is absent")?;if id==text(&r,"id")||get(&target,"job")!=&Null{return Err("A job follow-up cannot use a submission route".into());}}
    }
    Ok(())
}
fn record(st:&Json,id:&str)->Result<Json>{array(st,"service_jobs").into_iter().find(|j|text(j,"id")==id&&get(j,"root")==get(st,"approval_root_identity")).ok_or("Job is absent from this workspace".into())}
fn save(st:&mut Json,job:Json)->Result<()>{let mut rows=array(st,"service_jobs");if let Some(i)=rows.iter().position(|j|text(j,"id")==text(&job,"id")){rows[i]=job;}else{if rows.len()>=128{return Err("Job history reached 128 entries; archive/export existing job records before another submission".into());}rows.push(job);}put(st,"service_jobs",Arr(rows));Ok(())}
pub(super) fn admission(st:&Json,r:&Json)->Result<()>{if get(r,"job")!=&Null{let rows=array(st,"service_jobs");if rows.len()>=128||rows.iter().filter(|j|!["completed","failed","cancelled"].contains(&text(j,"state").as_str())).count()>=32{return Err("Job tracking capacity reached; reconcile or export pending jobs before a new submission".into());}}Ok(())}
pub(super) fn resolve(st:&Json,args:&Json)->Result<Json>{
    let id=text(args,"job_ref");if id.is_empty(){return Ok(args.clone());}
    let job=record(st,&id)?;if boolean(st,"agent_pending")&&text(&job,"chat")!=text(st,"active"){return Err("This model cannot access a job from another conversation".into());}
    let action=text(args,"job_action");let key=match action.as_str(){"status"=>"status_route","result"=>"result_route","cancel"=>"cancel_route",_=>return Err("Choose status, result or cancel for a tracked job".into())};
    let rid=text(get(&job,"mapping"),key);if rid.is_empty(){return Err("This job has no configured route for that action".into());}
    if !text(args,"route").is_empty()&&text(args,"route")!=rid{return Err("Job follow-up route does not match the captured job mapping".into());}
    let r=route(st,&rid)?;if origin(st,&r)?!=text(get(&job,"origins"),key){return Err("Job follow-up host changed; the server job identifier was not sent to a new origin".into());}
    let mut out=args.clone();put(&mut out,"route",s(&rid));if let Obj(fields)=&mut out{fields.remove("capability");}
    let mut input=get(args,"input").clone();if input==Null{input=obj(vec![]);}if !matches!(input,Obj(_)){return Err("Job input must be an object".into());}
    if let Obj(mapped)=get(get(&job,"inputs"),&format!("{action}_input")){for(k,v)in mapped{if get(&input,k)!=&Null&&get(&input,k)!=v{return Err("Job input differs from its captured server response mapping".into());}put(&mut input,k,v.clone());}}
    if get(&input,"job")!=&Null&&get(&input,"job")!=get(&job,"server_id"){return Err("Job input identifier differs from the tracked server job".into());}
    put(&mut input,"job",get(&job,"server_id").clone());put(&mut out,"input",input);Ok(out)
}
pub(super) fn adopt(st:&mut Json,expected:&Json,r:&Json,code:u16,document:&Json,value:&Json)->Result<()>{
    let reference=text(expected,"job_ref");
    if !reference.is_empty(){
        let mut job=record(st,&reference)?;
        match text(expected,"job_action").as_str(){
            "status"=>{let state=services::pointer(document,&text(get(&job,"mapping"),"state"))?.clone();if !scalar(&state){return Err("Job state must be a bounded scalar".into());}let mapping=get(&job,"mapping");let next=if array(mapping,"success").contains(&state){"ready"}else if array(mapping,"failure").contains(&state){"failed"}else{"running"};let mut inputs=get(&job,"inputs").clone();if inputs==Null{inputs=obj(vec![]);}let template=get(mapping,"result_input");if template!=&Null&&next=="ready"{let mapped=services::map_request(template,document,0,&mut 32768,&mut |v|Ok(v.clone()))?;put(&mut inputs,"result_input",mapped);}put(&mut job,"inputs",inputs);put(&mut job,"server_state",state);if text(&job,"state")!="completed"{put(&mut job,"state",s(next));}},
            "result"=>{if get(value,"sha256")==&Null{return Err("Job result route did not return a validated local artifact".into());}put(&mut job,"artifact",value.clone());put(&mut job,"state",s("completed"));},
            "cancel"=>put(&mut job,"state",s(if code==202{"cancel_requested"}else{"cancel_response_received"})),
            _=>return Err("Unknown job follow-up".into())}
        put(&mut job,"last_error",Null);save(st,job)?;return Ok(());
    }
    if !acceptance(r,code)||get(r,"job")==&Null{return Ok(());}
    let mapping=get(r,"job");let server=services::pointer(document,&text(mapping,"id"))?;let server=match server{Str(v)|Num(v)if !v.is_empty()&&v.len()<=256=>s(v),_=>return Err("Accepted job response has no bounded scalar job identifier; submission was not replayed".into())};
    let mut origins=obj(vec![]);for key in ["status_route","result_route","cancel_route"]{let id=text(mapping,key);if !id.is_empty(){put(&mut origins,key,s(&origin(st,&route(st,&id)?)?));}}
    let mut inputs=obj(vec![]);for key in ["status_input","cancel_input"]{let template=get(mapping,key);if template!=&Null{put(&mut inputs,key,services::map_request(template,document,0,&mut 32768,&mut |v|Ok(v.clone()))?);}}
    let job=obj(vec![("id",s(&format!("job-{}",now()))),("server_id",server),("root",get(st,"approval_root_identity").clone()),("chat",get(st,"active").clone()),("route",get(r,"id").clone()),("state",s("accepted")),("mapping",mapping.clone()),("inputs",inputs),("origins",origins)]);save(st,job)
}
pub(super) fn failed(st:&mut Json,expected:&Json,error:&str){let id=text(expected,"job_ref");if let Ok(mut job)=record(st,&id){put(&mut job,"last_error",s(&error.chars().take(1024).collect::<String>()));let _=save(st,job);}}
pub(super) fn catalog(st:&Json)->String{let rows=array(st,"service_jobs").into_iter().filter(|j|get(j,"root")==get(st,"approval_root_identity")&&text(j,"chat")==text(st,"active")&&!boolean(j,"archived")).map(|j|obj(vec![("job_ref",get(&j,"id").clone()),("state",get(&j,"state").clone())])).collect();format!("\nTracked jobs in this conversation: {}. To inspect or retrieve one, service args {{job_ref,job_action:\"status\"|\"result\"|\"cancel\",input:{{}}}}. This uses its captured follow-up route; never repeat generation to poll.",Arr(rows).string())}
pub(super) fn handles(a:&str)->bool{a=="jobs_menu"||a.starts_with("job_view:")||["job_status","job_result","job_cancel","job_archive","job_restore","job_forget","job_forget_confirm"].contains(&a)}
pub(super) fn event(st:&mut Json,a:&str)->Result<Json>{
    if boolean(st,"busy")||get(st,"pending")!=&Null{return Err("Finish or cancel the current operation first".into());}
    if a=="jobs_menu"{put(st,"dialog",s("jobs_menu"));return Ok(Null);}
    if let Some(id)=a.strip_prefix("job_view:"){record(st,id)?;put(st,"job_selected",s(id));put(st,"dialog",s("job_view"));return Ok(Null);}
    let mut job=record(st,&text(st,"job_selected"))?;
    if a=="job_forget"{put(st,"dialog",s("job_forget_review"));return Ok(Null);}
    if a=="job_forget_confirm"{if text(st,"dialog")!="job_forget_review"{return Err("Review the selected local job record before removal".into());}let rows=array(st,"service_jobs").into_iter().filter(|row|text(row,"id")!=text(&job,"id")).collect();put(st,"service_jobs",Arr(rows));put(st,"job_selected",Null);put(st,"dialog",s("jobs_menu"));put(st,"notice",s("Selected local job record removed; no server cancellation or file deletion was requested"));return Ok(Null);}
    if ["job_archive","job_restore"].contains(&a){put(&mut job,"archived",Bool(a=="job_archive"));save(st,job)?;return Ok(Null);}
    let action=match a{"job_status"=>"status","job_result"=>"result","job_cancel"=>"cancel",_=>return Err("Unknown job action".into())};
    let args=resolve(st,&obj(vec![("job_ref",get(&job,"id").clone()),("job_action",s(action)),("input",obj(vec![]))]))?;
    put(st,"pending",obj(vec![("tool",s("service")),("args",args)]));put(st,"agent_pending",Bool(false));put(st,"dialog",s("approval"));Ok(Null)
}
pub(super) fn dialog(st:&Json)->Option<Json>{match text(st,"dialog").as_str(){
    "jobs_menu"=>Some(menu("Generation jobs",array(st,"service_jobs").iter().filter(|j|get(*j,"root")==get(st,"approval_root_identity")).map(|j|button(&format!("{} · {}{}",text(j,"id"),text(j,"state"),if boolean(j,"archived"){" · archived"}else{""}),&format!("job_view:{}",text(j,"id")))).collect())),
    "job_forget_review"=>Some(modal("Permanently remove this local job record?",vec![field_ui("job","Selected record",&format!("{}\nThis removes local tracking only. A running server job may continue. Export or record its identifier first; retained files are not removed.",record(st,&text(st,"job_selected")).unwrap_or(Null).string()),"readonly")],vec![button("Remove this local job record","job_forget_confirm"),button("Keep record","jobs_menu")])),
    "job_view"=>{let job=record(st,&text(st,"job_selected")).unwrap_or(Null);let mut buttons=vec![button("Refresh server state","job_status"),button("Retrieve result","job_result"),button("Review local record removal","job_forget"),button(if boolean(&job,"archived"){"Restore in history"}else{"Archive in history"},if boolean(&job,"archived"){"job_restore"}else{"job_archive"})];if !text(get(&job,"mapping"),"cancel_route").is_empty(){buttons.push(button("Review server cancellation","job_cancel"));}Some(modal("Generation job",vec![field_ui("job","Server record",&job.string(),"readonly")],buttons))},_=>None}}
