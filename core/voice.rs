//! Explicit, expiring foreground voice turns; text remains the conversation.
use super::*;
use std::sync::{Mutex,OnceLock};
use std::time::{Instant,Duration};
static CONSENTS:OnceLock<Mutex<BTreeMap<String,(String,Instant)>>>=OnceLock::new();
static PLAYBACKS:OnceLock<Mutex<BTreeMap<String,(String,Instant)>>>=OnceLock::new();
fn playbacks()->&'static Mutex<BTreeMap<String,(String,Instant)>>{PLAYBACKS.get_or_init(||Mutex::new(BTreeMap::new()))}
pub(super) fn token_live(ticket:&str)->bool{consents().lock().ok().is_some_and(|held|held.get(ticket).is_some_and(|(_,t)|t.elapsed()<Duration::from_secs(600)))}
fn settings(st:&Json)->Json{if get(st,"voice_settings")!=&Null{return get(st,"voice_settings").clone();}obj(vec![("silence_ms",Num("800".into())),("threshold",Num("500".into())),("max_seconds",Num("20".into()))])}
fn set_phase(st:&mut Json,value:&str){put(st,"voice_status",s(value));}
pub(super) fn open(st:&Json)->bool{boolean(st,"voice_open")&&authorized(st)}
fn pause(st:&mut Json,reason:&str){let session=text(st,"voice_consent");voice_audio::cancel_session(&session);if let Ok(mut held)=playbacks().lock(){held.retain(|_,(s,_)|*s!=session);}put(st,"voice_audio_expected",Null);put(st,"voice_playback_expected",Null);put(st,"voice_turn",Bool(false));if boolean(st,"voice_open"){set_phase(st,"paused");}put(st,"notice",s(reason));}
fn listen(st:&mut Json)->Result<Json>{if !open(st){return Err("Review and enable the foreground microphone first".into());}if boolean(st,"busy")||get(st,"pending")!=&Null{return Err("Finish the current operation before resuming the microphone".into());}if !text(st,"draft").trim().is_empty(){return Err("Send or clear the existing draft before starting the microphone".into());}let session=text(st,"voice_consent");voice_audio::cancel_session(&session);let ticket=voice_audio::open(&session,&settings(st))?;put(st,"voice_audio_expected",obj(vec![("ticket",s(&ticket)),("session",s(&session))]));set_phase(st,"microphone_pending");put(st,"page",s("chat"));put(st,"dialog",Null);Ok(obj(vec![("kind",s("voice_listen")),("voice_session",s(&session)),("voice_audio_ticket",s(&ticket)),("sample_rate",Num("16000".into())),("frame_bytes",Num("640".into()))]))}
pub(super) fn callback(a:&str)->bool{matches!(a,"voice_audio_start"|"voice_audio_result"|"voice_audio_failure"|"voice_playback_start"|"voice_playback_result")}
pub(super) fn consume(st:&Json,a:&str,input:&Json)->bool{let session=text(input,"voice_session");if session.is_empty()||session!=text(st,"voice_consent")||!consents().lock().ok().is_some_and(|held|held.get(&session).is_some_and(|(b,_)|*b==binding(st))){return false;}if a.starts_with("voice_audio_"){let ticket=text(input,"voice_audio_ticket");let expected=get(st,"voice_audio_expected");return text(expected,"ticket")==ticket&&text(expected,"session")==session&&(a!="voice_audio_start"||text(st,"voice_status")=="microphone_pending")&&if a=="voice_audio_result"{voice_audio::receipt_owned(&ticket,&session,&text(input,"receipt"))}else{voice_audio::owns(&ticket,&session)};}
 let ticket=text(input,"voice_playback_ticket");if text(get(st,"voice_playback_expected"),"ticket")!=ticket{return false;}let Ok(mut held)=playbacks().lock()else{return false;};if !held.get(&ticket).is_some_and(|(s,t)|*s==session&&t.elapsed()<Duration::from_secs(600)){return false;}if a=="voice_playback_start"{return text(st,"voice_status")=="playback_pending";}held.remove(&ticket);true}
pub(super) fn reconcile(st:&mut Json,action:&str,input:&Json){if boolean(st,"voice_open")&&!authorized(st){interrupt(st);return;}if !open(st){return;}if text(st,"page")!="chat"||["voice_review","voice_open_review","service_connections","connections","profile_save","profile_new","routing_save","service_save","workspace_select","new_chat","phone_file","media_pick","media_record"].contains(&action)||(action=="send"&&!boolean(input,"voice_transcript")){interrupt(st);}else if boolean(st,"notice_error"){pause(st,"Voice conversation paused after an operation error; text and saved files remain available");}else if matches!(text(st,"voice_status").as_str(),"listening"|"microphone_pending")&&!text(st,"draft").trim().is_empty(){pause(st,"Microphone paused while you edit the text draft");}}
fn consents()->&'static Mutex<BTreeMap<String,(String,Instant)>>{CONSENTS.get_or_init(||Mutex::new(BTreeMap::new()))}
fn binding(st:&Json)->String{harness_rules::digest(&obj(vec![("root",get(st,"approval_root_identity").clone()),("chat",get(st,"active").clone()),("config",get(st,"service_connections").clone()),("profiles",get(st,"profiles").clone()),("selected",get(st,"selected").clone()),("routing",get(st,"routing").clone()),("agent",harness::active_agent(st)),("voice_settings",get(st,"voice_settings").clone()),("policy",harness_rules::binding(st))]))}
pub(super) fn authorized(st:&Json)->bool{let ticket=text(st,"voice_consent");consents().lock().ok().is_some_and(|held|held.get(&ticket).is_some_and(|(b,t)|*b==binding(st)&&t.elapsed()<Duration::from_secs(600)))}
pub(super) fn phase(st:&Json,p:&Json)->String{
    let phase=text(p,"voice_phase");
    if boolean(st,"agent_pending")||phase!=text(st,"voice_phase"){return String::new();}
    if matches!(phase.as_str(),"transcript"|"manual_synthesis")||authorized(st){phase}else{String::new()}
}
pub(super) fn interrupt(st:&mut Json){let session=text(st,"voice_consent");voice_audio::cancel_session(&session);if let Ok(mut held)=playbacks().lock(){held.retain(|_,(s,_)|*s!=session);}if let Ok(mut held)=consents().lock(){held.remove(&session);}put(st,"voice_consent",Null);put(st,"voice_open",Bool(false));put(st,"voice_audio_expected",Null);put(st,"voice_playback_expected",Null);set_phase(st,"off");put(st,"voice_turn",Bool(false));put(st,"voice_phase",Null);}
fn destinations(st:&Json,capability:&str)->Result<String>{
    let args=obj(vec![("capability",s(capability)),("input",obj(vec![]))]);let mut p=obj(vec![("tool",s("service")),("args",args)]);services::prepare(&mut st.clone(),&mut p)?;Ok(text(&p,"destination"))
}
fn audio_mapping(v:&Json,depth:usize)->bool{if depth>24{return false;}match v{Obj(m)if m.len()==1&&m.contains_key("$file")=>["base64","data_url"].contains(&text(v,"$file").as_str()),Obj(m)=>m.values().any(|v|audio_mapping(v,depth+1)),Arr(a)=>a.iter().any(|v|audio_mapping(v,depth+1)),_=>false}}
fn ready(st:&Json)->Result<String>{
    profile(st).map_err(|_|"Choose a conversation model in LLM connections first")?;let cfg=get(st,"service_connections");
    let recognition=destinations(st,"speech_recognition").map_err(|e|format!("Speech recognition connection is not ready: {e}. Configure the speech_recognition default in Service connections."))?;
    for id in services::capability_routes(st,"speech_recognition")?{let r=array(cfg,"routes").into_iter().find(|r|text(r,"id")==id).ok_or("Recognition route is absent")?;if !["json","text"].contains(&text(&r,"response").as_str()){return Err("Recognition routes must return text or a JSON pointer selecting text".into());}if text(&r,"method")=="GET"||(!["binary","multipart"].contains(&text(&r,"request_format").as_str())&&!audio_mapping(get(&r,"request"),0)){return Err("Recognition routes must send the actual recording using binary, multipart or a base64/data_url file mapping".into());}}
    let routes=services::capability_routes(st,"speech_synthesis").map_err(|e|format!("Speech output connection is not ready: {e}. Configure the speech_synthesis default in Service connections."))?;
    for id in routes{let r=array(cfg,"routes").into_iter().find(|r|text(r,"id")==id).ok_or("Speech output route is absent")?;if !["binary","base64","artifact_url"].contains(&text(&r,"response").as_str()){return Err("Voice synthesis routes must return binary or base64 audio".into());}}
    Ok(format!("Recognition and configured fallbacks:\n{}\n\nSpeech and configured fallbacks:\n{}\n\nConversation model:\n{}\n\nManual mode uploads each deliberate recording. Optional open microphone mode detects utterances, uploads them for recognition, sends recognized words to the conversation model, plays speech and listens again after completed playback. The microphone pauses during recognition, model work and playback to prevent speaker feedback. Stop voice, leaving the foreground, process restart, changed settings or ten minutes end the session. Ordinary operation reviews remain in force. A speech output key alone does not provide speech recognition.",recognition,destinations(st,"speech_synthesis")?,endpoint_summary(&text(&profile(st)?,"endpoint"))))
}
fn request(st:&mut Json,capability:&str,file:bool,input:Json,phase:&str,automatic:bool)->Result<Json>{
    if automatic&&!authorized(st){return Err("Voice consent expired or changed; text and files remain available".into());}
    let mut p=obj(vec![("tool",s("service")),("args",obj(vec![("capability",s(capability)),("file",Bool(file)),("input",input)])),("voice_phase",s(phase))]);
    services::prepare(st,&mut p)?;if open(st){voice_audio::cancel_session(&text(st,"voice_consent"));put(st,"voice_audio_expected",Null);}put(st,"voice_phase",s(phase));if open(st){set_phase(st,if phase=="recognition"{"recognizing"}else{"synthesizing"});}put(st,"agent_pending",Bool(false));put(st,"agent_running",Bool(false));
    if !automatic||services::network_review(st,&p)||harness_rules::review_required(st,&p){put(st,"pending",p);put(st,"dialog",s("approval"));return Ok(Null);}
    let effect=services::start(st,&p)?;put(st,"pending",Null);put(st,"dialog",Null);Ok(effect)
}
pub(super) fn recorded(st:&mut Json)->Result<Json>{if authorized(st){put(st,"voice_turn",Bool(true));request(st,"speech_recognition",true,obj(vec![]),"recognition",true)}else{Ok(Null)}}
pub(super) fn service_result(st:&mut Json,expected:&Json,result:&Json)->Result<Json>{
    let phase=text(expected,"voice_phase");if phase.is_empty(){return Ok(Null);}
    let paused=boolean(st,"voice_open")&&text(st,"voice_status")=="paused";put(st,"voice_phase",Null);
    if text(result,"status")!="response_received"{pause(st,"Voice service did not return a usable result; microphone paused and text remains available");return Ok(Null);}
    if matches!(phase.as_str(),"recognition"|"transcript"){
        let transcript=get(result,"value").text()?.trim().to_string();if transcript.is_empty()||transcript.len()>32768{return Err("Transcript is empty or exceeds 32 KiB; no message was sent".into());}
        let previous=text(st,"draft");if !previous.trim().is_empty(){put(st,"draft",s(&format!("{previous}\n{transcript}")));pause(st,"Transcript appended for review; nothing was sent");put(st,"dialog",Null);return Ok(Null);}
        put(st,"draft",s(&transcript));put(st,"dialog",Null);
        if phase=="transcript"||!authorized(st)||paused{pause(st,"Transcript is in the composer for review; it was not sent");return Ok(Null);}
        if open(st){set_phase(st,"thinking");}let next=event_inner(&obj(vec![("state",st.clone()),("action",s("send")),("inputs",obj(vec![("draft",s(&transcript)),("voice_transcript",Bool(true))])),("root",get(st,"approval_root").clone())]))?;
        *st=get(&next,"state").clone();Ok(get(&next,"effect").clone())
    }else if phase=="synthesis"{
        if !authorized(st)||paused{put(st,"notice",s("Voice paused or consent expired; returned audio is available for manual preview"));return Ok(Null);}
        if text(get(result,"value"),"kind")!="audio"{return Err("Synthesis returned a non-audio artifact; the text reply remains available".into());}
        let mut effect=media::event(st,"media_preview",&obj(vec![]))?;if open(st){let ticket=format!("voice-playback:{}",now());let session=text(st,"voice_consent");let mut held=playbacks().lock().map_err(|_|"Playback ownership unavailable")?;held.retain(|_,(_,t)|t.elapsed()<Duration::from_secs(600));if held.len()>=16{return Err("Voice playback slots occupied".into());}held.insert(ticket.clone(),(session.clone(),Instant::now()));put(st,"voice_playback_expected",obj(vec![("ticket",s(&ticket))]));set_phase(st,"playback_pending");put(&mut effect,"voice_playback_ticket",s(&ticket));put(&mut effect,"voice_session",s(&session));}Ok(effect)
    }else if phase=="manual_synthesis"{if text(get(result,"value"),"kind")!="audio"{return Err("Speech service returned a non-audio file; the text remains available".into());}media::event(st,"media_preview",&obj(vec![]))}else{Err("Unknown voice phase".into())}
}
pub(super) fn answer(st:&mut Json)->Result<Json>{
    if !boolean(st,"voice_turn"){return Ok(Null);}put(st,"voice_turn",Bool(false));
    if !authorized(st){put(st,"notice",s("Reply retained as text; voice consent expired or changed"));return Ok(Null);}
    let messages=array(&chat(st),"messages");let last=messages.last().ok_or("No assistant answer is available")?;
    if text(last,"role")!="assistant"{return Ok(Null);}let answer=text(last,"content");if answer.len()>32768{return Err("Reply exceeds the configured voice text bound; the full text remains available".into());}
    request(st,"speech_synthesis",false,obj(vec![("text",s(&answer))]),"synthesis",true)
}
pub(super) fn handles(a:&str)->bool{callback(a)||["voice_review","voice_open_review","voice_consent","voice_stop","voice_suspend","voice_listen","voice_record","voice_transcribe","voice_speak"].contains(&a)}
pub(super) fn event(st:&mut Json,a:&str,input:&Json)->Result<Json>{
    if a=="voice_stop"||a=="voice_suspend"{
        let service=!text(get(st,"service_expected"),"voice_phase").is_empty();let capture=get(st,"media_expected")!=&Null;
        if !text(get(st,"pending"),"voice_phase").is_empty(){put(st,"pending",Null);put(st,"dialog",Null);put(st,"agent_pending",Bool(false));}
        interrupt(st);if service{services::interrupt(st);put(st,"busy",Bool(false));}if capture{media::interrupt(st);}
        return Ok(obj(vec![("kind",s(if service||capture{"cancel"}else{"media_playback_stop"}))]));
    }
    if callback(a){if !authorized(st){interrupt(st);return Ok(obj(vec![("kind",s("media_playback_stop"))]));}match a{
      "voice_audio_start"=>{set_phase(st,"listening");return Ok(Null);},
      "voice_audio_result"=>{let bytes=voice_audio::take(&text(input,"voice_audio_ticket"),&text(input,"voice_session"),&text(input,"receipt"))?;put(st,"voice_audio_expected",Null);let record=media::store(st,&bytes,"recording.wav","audio","input")?;put(st,"media_input",record);return recorded(st);},
      "voice_audio_failure"=>{pause(st,"Microphone unavailable or stopped; text conversation remains available");return Ok(Null);},
      "voice_playback_start"=>{set_phase(st,"speaking");return Ok(Null);},
      "voice_playback_result"=>{put(st,"voice_playback_expected",Null);if boolean(input,"completed")&&open(st){return listen(st);}pause(st,"Speech playback stopped or failed; microphone paused");return Ok(Null);},_=>{}}}
    if boolean(st,"busy")||get(st,"pending")!=&Null||get(st,"task_run")!=&Null{return Err("Finish or cancel the current task before starting voice".into());}
    match a{
        "voice_review"|"voice_open_review"=>{if chat(st)==Null{new_chat(st)?;}put(st,"voice_review_open",Bool(a=="voice_open_review"));put(st,"edit",obj(vec![]));put(st,"dialog",s("voice_review"));},
        "voice_consent"=>{
            if text(st,"dialog")!="voice_review"{return Err("Review voice destinations before enabling conversation".into());}ready(st)?;
            let mut config=settings(st);for key in ["silence_ms","threshold","max_seconds"]{let value=text(get(st,"edit"),&format!("voice_{key}"));if !value.is_empty(){let n=value.parse::<usize>().map_err(|_|"Voice settings require whole numbers")?;put(&mut config,key,Num(n.to_string()));}}voice_audio::validate(&config)?;let microphone=if get(get(st,"edit"),"voice_open")==&Null{boolean(st,"voice_review_open")}else{boolean(get(st,"edit"),"voice_open")};if microphone&&!text(st,"draft").trim().is_empty(){return Err("Send or clear the draft before starting open microphone conversation".into());}interrupt(st);put(st,"voice_settings",config);
            let ticket=format!("voice:{}",now());let mut held=consents().lock().map_err(|_|"Voice consent unavailable")?;held.retain(|_,(_,t)|t.elapsed()<Duration::from_secs(600));if held.len()>=4{return Err("Too many active voice sessions".into());}held.insert(ticket.clone(),(binding(st),Instant::now()));drop(held);put(st,"voice_consent",s(&ticket));put(st,"voice_open",Bool(microphone));put(st,"dialog",s("media_menu"));put(st,"notice",s("Voice conversation enabled for ten minutes"));if microphone{return listen(st);}
        },
        "voice_listen"=>return listen(st),
        "voice_record"=>{if open(st){voice_audio::cancel_session(&text(st,"voice_consent"));put(st,"voice_audio_expected",Null);put(st,"voice_open",Bool(false));set_phase(st,"off");}return media::event(st,"media_record",&obj(vec![]));},
        "voice_transcribe"=>return request(st,"speech_recognition",true,obj(vec![]),"transcript",false),
        "voice_speak"=>{let answer=array(&chat(st),"messages").iter().rev().find(|m|text(m,"role")=="assistant").map(|m|text(m,"content")).ok_or("No assistant reply is available")?;return request(st,"speech_synthesis",false,obj(vec![("text",s(&answer))]),if open(st){"synthesis"}else{"manual_synthesis"},false);},
        _=>return Err("Unknown voice action".into())}
    Ok(Null)
}
pub(super) fn status(st:&Json)->Json{if !boolean(st,"voice_open"){return Null;}obj(vec![("phase",get(st,"voice_status").clone()),("label",s(match text(st,"voice_status").as_str(){"microphone_pending"=>"Starting microphone","listening"=>"Listening","recognizing"=>"Transcribing","thinking"=>"Preparing reply","synthesizing"=>"Preparing speech","playback_pending"=>"Starting playback","speaking"=>"Speaking",_=>"Paused"})),("active",Bool(open(st)))] )}
pub(super) fn dialog(st:&Json)->Option<Json>{if text(st,"dialog")!="voice_review"{return None;}let cfg=settings(st);Some(modal("Voice conversation",vec![field_ui("voice_destinations","Review destinations and behavior",&ready(st).unwrap_or_else(|e|e),"readonly"),field_ui("voice_open","Keep listening between replies while this app is open",if boolean(st,"voice_review_open"){"true"}else{"false"},"check"),field_ui("voice_silence_ms","Pause before sending speech (300–3000 milliseconds)",&number(&cfg,"silence_ms").to_string(),"number"),field_ui("voice_threshold","Microphone activity threshold (100–5000; lower is more sensitive)",&number(&cfg,"threshold").to_string(),"number"),field_ui("voice_max_seconds","Maximum utterance length (5–30 seconds)",&number(&cfg,"max_seconds").to_string(),"number")],vec![button("Enable voice conversation","voice_consent"),button("Configure speech connections","service_connections"),button("Cancel","close")]))}
