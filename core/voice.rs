//! Explicit, expiring foreground voice turns; text remains the conversation.
use super::*;
use std::sync::{Mutex,OnceLock};
use std::time::{Instant,Duration};
static CONSENTS:OnceLock<Mutex<BTreeMap<String,(String,Instant)>>>=OnceLock::new();
fn consents()->&'static Mutex<BTreeMap<String,(String,Instant)>>{CONSENTS.get_or_init(||Mutex::new(BTreeMap::new()))}
fn binding(st:&Json)->String{harness_rules::digest(&obj(vec![("root",get(st,"approval_root_identity").clone()),("chat",get(st,"active").clone()),("config",get(st,"service_connections").clone()),("profiles",get(st,"profiles").clone()),("selected",get(st,"selected").clone()),("routing",get(st,"routing").clone()),("agent",harness::active_agent(st)),("policy",harness_rules::binding(st))]))}
pub(super) fn authorized(st:&Json)->bool{let ticket=text(st,"voice_consent");consents().lock().ok().is_some_and(|held|held.get(&ticket).is_some_and(|(b,t)|*b==binding(st)&&t.elapsed()<Duration::from_secs(600)))}
pub(super) fn phase(st:&Json,p:&Json)->String{
    let phase=text(p,"voice_phase");
    if boolean(st,"agent_pending")||phase!=text(st,"voice_phase"){return String::new();}
    if matches!(phase.as_str(),"transcript"|"manual_synthesis")||authorized(st){phase}else{String::new()}
}
pub(super) fn interrupt(st:&mut Json){if let Ok(mut held)=consents().lock(){held.remove(&text(st,"voice_consent"));}put(st,"voice_consent",Null);put(st,"voice_turn",Bool(false));put(st,"voice_phase",Null);}
fn destinations(st:&Json,capability:&str)->Result<String>{
    let args=obj(vec![("capability",s(capability)),("input",obj(vec![]))]);let mut p=obj(vec![("tool",s("service")),("args",args)]);services::prepare(&mut st.clone(),&mut p)?;Ok(text(&p,"destination"))
}
fn ready(st:&Json)->Result<String>{
    profile(st)?;let cfg=get(st,"service_connections");
    let routes=services::capability_routes(st,"speech_synthesis")?;
    for id in routes{let r=array(cfg,"routes").into_iter().find(|r|text(r,"id")==id).ok_or("Speech output route is absent")?;if !["binary","base64","artifact_url"].contains(&text(&r,"response").as_str()){return Err("Voice synthesis routes must return binary or base64 audio".into());}}
    Ok(format!("Recognition and configured fallbacks:\n{}\n\nSpeech and configured fallbacks:\n{}\n\nConversation model:\n{}\n\nEach deliberate Record / Finish uploads that recording, sends its transcript as your next message and plays the returned spoken reply. Consent expires after ten minutes, process restart, connection/policy changes or Stop voice. Ordinary tool reviews remain in force. Capture is foreground only; no open microphone is enabled.",destinations(st,"speech_recognition")?,destinations(st,"speech_synthesis")?,endpoint_summary(&text(&profile(st)?,"endpoint"))))
}
fn request(st:&mut Json,capability:&str,file:bool,input:Json,phase:&str,automatic:bool)->Result<Json>{
    if automatic&&!authorized(st){return Err("Voice consent expired or changed; text and files remain available".into());}
    let mut p=obj(vec![("tool",s("service")),("args",obj(vec![("capability",s(capability)),("file",Bool(file)),("input",input)])),("voice_phase",s(phase))]);
    services::prepare(st,&mut p)?;put(st,"voice_phase",s(phase));put(st,"agent_pending",Bool(false));put(st,"agent_running",Bool(false));
    if !automatic||services::network_review(st,&p)||harness_rules::review_required(st,&p){put(st,"pending",p);put(st,"dialog",s("approval"));return Ok(Null);}
    let effect=services::start(st,&p)?;put(st,"pending",Null);put(st,"dialog",Null);Ok(effect)
}
pub(super) fn recorded(st:&mut Json)->Result<Json>{if authorized(st){put(st,"voice_turn",Bool(true));request(st,"speech_recognition",true,obj(vec![]),"recognition",true)}else{Ok(Null)}}
pub(super) fn service_result(st:&mut Json,expected:&Json,result:&Json)->Result<Json>{
    let phase=text(expected,"voice_phase");if phase.is_empty(){return Ok(Null);}
    put(st,"voice_phase",Null);
    if text(result,"status")!="response_received"{put(st,"voice_turn",Bool(false));put(st,"notice",s("Voice service did not return a usable result; text and local files remain available"));return Ok(Null);}
    if matches!(phase.as_str(),"recognition"|"transcript"){
        let transcript=get(result,"value").text()?.trim().to_string();if transcript.is_empty()||transcript.len()>32768{return Err("Transcript is empty or exceeds 32 KiB; no message was sent".into());}
        let previous=text(st,"draft");if !previous.trim().is_empty(){put(st,"draft",s(&format!("{previous}\n{transcript}")));put(st,"voice_turn",Bool(false));put(st,"dialog",Null);put(st,"notice",s("Transcript appended for review; nothing was sent"));return Ok(Null);}
        put(st,"draft",s(&transcript));put(st,"dialog",Null);
        if phase=="transcript"||!authorized(st){put(st,"voice_turn",Bool(false));put(st,"notice",s("Transcript is in the composer for review; it was not sent"));return Ok(Null);}
        let next=event_inner(&obj(vec![("state",st.clone()),("action",s("send")),("inputs",obj(vec![("draft",s(&transcript)),("voice_transcript",Bool(true))])),("root",get(st,"approval_root").clone())]))?;
        *st=get(&next,"state").clone();Ok(get(&next,"effect").clone())
    }else if phase=="synthesis"{
        if !authorized(st){put(st,"notice",s("Voice consent expired; returned audio is available for manual preview"));return Ok(Null);}
        if text(get(result,"value"),"kind")!="audio"{return Err("Synthesis returned a non-audio artifact; the text reply remains available".into());}
        media::event(st,"media_preview",&obj(vec![]))
    }else if phase=="manual_synthesis"{if text(get(result,"value"),"kind")!="audio"{return Err("Speech service returned a non-audio file; the text remains available".into());}media::event(st,"media_preview",&obj(vec![]))}else{Err("Unknown voice phase".into())}
}
pub(super) fn answer(st:&mut Json)->Result<Json>{
    if !boolean(st,"voice_turn"){return Ok(Null);}put(st,"voice_turn",Bool(false));
    if !authorized(st){put(st,"notice",s("Reply retained as text; voice consent expired or changed"));return Ok(Null);}
    let messages=array(&chat(st),"messages");let last=messages.last().ok_or("No assistant answer is available")?;
    if text(last,"role")!="assistant"{return Ok(Null);}let answer=text(last,"content");if answer.len()>32768{return Err("Reply exceeds the configured voice text bound; the full text remains available".into());}
    request(st,"speech_synthesis",false,obj(vec![("text",s(&answer))]),"synthesis",true)
}
pub(super) fn handles(a:&str)->bool{["voice_review","voice_consent","voice_stop","voice_record","voice_transcribe","voice_speak"].contains(&a)}
pub(super) fn event(st:&mut Json,a:&str)->Result<Json>{
    if a=="voice_stop"{
        let service=!text(get(st,"service_expected"),"voice_phase").is_empty();let capture=get(st,"media_expected")!=&Null;
        if !text(get(st,"pending"),"voice_phase").is_empty(){put(st,"pending",Null);put(st,"dialog",Null);put(st,"agent_pending",Bool(false));}
        interrupt(st);if service{services::interrupt(st);put(st,"busy",Bool(false));}if capture{media::interrupt(st);}
        return Ok(obj(vec![("kind",s(if service||capture{"cancel"}else{"media_playback_stop"}))]));
    }
    if boolean(st,"busy")||get(st,"pending")!=&Null||get(st,"task_run")!=&Null{return Err("Finish or cancel the current task before starting voice".into());}
    match a{
        "voice_review"=>{if chat(st)==Null{new_chat(st)?;}ready(st)?;put(st,"dialog",s("voice_review"));},
        "voice_consent"=>{
            if text(st,"dialog")!="voice_review"{return Err("Review voice destinations before enabling conversation".into());}ready(st)?;
            let ticket=format!("voice:{}",now());let mut held=consents().lock().map_err(|_|"Voice consent unavailable")?;held.retain(|_,(_,t)|t.elapsed()<Duration::from_secs(600));held.remove(&text(st,"voice_consent"));if held.len()>=4{return Err("Too many active voice sessions".into());}held.insert(ticket.clone(),(binding(st),Instant::now()));put(st,"voice_consent",s(&ticket));put(st,"dialog",s("media_menu"));put(st,"notice",s("Voice conversation enabled for ten minutes. Choose Record audio for each turn."));
        },
        "voice_record"=>return media::event(st,"media_record",&obj(vec![])),
        "voice_transcribe"=>return request(st,"speech_recognition",true,obj(vec![]),"transcript",false),
        "voice_speak"=>{let answer=array(&chat(st),"messages").iter().rev().find(|m|text(m,"role")=="assistant").map(|m|text(m,"content")).ok_or("No assistant reply is available")?;return request(st,"speech_synthesis",false,obj(vec![("text",s(&answer))]),"manual_synthesis",false);},
        _=>return Err("Unknown voice action".into())}
    Ok(Null)
}
pub(super) fn dialog(st:&Json)->Option<Json>{if text(st,"dialog")!="voice_review"{return None;}Some(modal("Voice conversation",vec![field_ui("voice_destinations","Review destinations and behavior",&ready(st).unwrap_or_else(|e|e),"readonly")],vec![button("Enable voice conversation","voice_consent"),button("Cancel","close")]))}
