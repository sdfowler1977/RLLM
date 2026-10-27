//! Explicit model advertisements are information, never generation authority.
use super::*;
use std::sync::atomic::{AtomicU64,Ordering};
const PREFIX:&str="model_metadata:";
const BODY_LIMIT:usize=32*1024;
const WIRE_LIMIT:usize=BODY_LIMIT+16*1024;

fn identity(edit:&Json)->Result<Json>{
    // Metadata can help a user correct an unsupported generation setting, so
    // normalize connection identity without validating their generation draft.
    let mut connection=obj(vec![]);
    for key in ["protocol","endpoint","model","name","key","auth_header","auth_prefix","plain"] {
        if get(edit,key)!=&Null {put(&mut connection,key,get(edit,key).clone());}
    }
    super::super::provider::normalized(&connection)
}
fn target(p:&Json)->Result<String>{
    if text(p,"protocol")=="local" {
        let endpoint=text(p,"endpoint");let(path,query)=endpoint.split_once('?').unwrap_or((&endpoint,""));
        let base=path.strip_suffix("/api/chat").ok_or("This custom URL has no unambiguous local model-metadata path. Manual settings remain available.")?;
        let resource=format!("{base}/api/show");
        return Ok(if query.is_empty(){resource}else{format!("{resource}?{query}")});
    }
    if text(p,"protocol")!="content" {return Err("Model metadata is currently available for Content and local chat APIs only. Manual settings remain available.".into());}
    let model=text(p,"model");
    if model.is_empty() || model.bytes().any(|c|!c.is_ascii_alphanumeric()&&!b"-_.".contains(&c)){return Err("This model ID has no supported Content model-metadata resource. Manual settings remain available.".into());}
    let configured=text(p,"endpoint").replace("{model}",&network::encoded(&model));
    let unary=super::super::provider::content_endpoint(&configured,false)?;
    let(path,query)=unary.split_once('?').unwrap_or((&unary,""));
    let suffix=format!("/models/{}:generateContent",network::encoded(&model));
    if !path.ends_with(&suffix){return Err("This custom URL has no unambiguous model-metadata path. Manual settings remain available.".into());}
    let resource=path.strip_suffix(":generateContent").ok_or("Invalid model-metadata path")?;
    Ok(if query.is_empty(){resource.into()}else{format!("{resource}?{query}")})
}
pub(super) fn is_result(inputs:&Json)->bool{text(inputs,"purpose").starts_with(PREFIX)}
pub(super) fn expected(st:&Json,inputs:&Json)->bool{
    let pending=get(st,"metadata_pending");
    boolean(st,"busy") && pending!=&Null && text(pending,"purpose")==text(inputs,"purpose")
        && identity(get(st,"edit")).ok().as_ref()==Some(get(pending,"identity"))
}
fn clear(st:&mut Json){for key in ["metadata_pending","metadata_binding","metadata_record","metadata_error","metadata_version","metadata_version_error"]{put(st,key,Null);}}
pub(super) fn reconcile(st:&mut Json,action:&str)->bool{
    let reset=matches!(action,"cancel"|"resume"|"close"|"back"|"profile_edit"|"profile_new"|"connect_llm"|"home_server"|"profile_save");
    let changed=get(st,"metadata_binding")!=&Null && identity(get(st,"edit")).ok().as_ref()!=Some(get(st,"metadata_binding"));
    if reset||changed {
        let own=get(st,"metadata_pending")!=&Null;
        clear(st);
        if own {put(st,"busy",Bool(false));}
        return own && !matches!(action,"cancel"|"resume");
    }
    false
}
pub(super) fn dispatch(st:&mut Json,action:&str,effect:&Json){
    // Once another runtime operation starts, a retained old purpose cannot own
    // that operation's busy flag. This does not gate the new operation.
    if get(st,"metadata_pending")!=&Null && effect!=&Null && !is_result(effect){put(st,"metadata_pending",Null);}
    if ["model_metadata","model_metadata_version"].contains(&action) && effect==&Null && !boolean(st,"busy") && get(st,"metadata_pending")!=&Null {
        let version=text(get(st,"metadata_pending"),"kind")=="version";
        put(st,"metadata_pending",Null);put(st,if version{"metadata_version_error"}else{"metadata_error"},s(if version{"Server version was not dispatched; model metadata and manual settings remain available."}else{"Model metadata was not dispatched; manual settings remain available."}));
    }
}
pub(super) fn start(st:&mut Json)->Result<Json>{request(st,false)}
pub(super) fn version(st:&mut Json)->Result<Json>{request(st,true)}
fn request(st:&mut Json,version:bool)->Result<Json>{
    if !matches!(text(st,"dialog").as_str(),"generation"|"profile"){return Err("Open the connection's Thinking and context editor first".into());}
    if text(st,"capability_mode")=="local" {return Err("Remote model requests are paused in Resources. Manual connection settings remain available.".into());}
    let p=identity(get(st,"edit"))?;let mut url=target(&p)?;
    if version {
        if text(&p,"protocol")!="local"{return Err("Server version is currently available for the local chat API only".into());}
        let(path,query)=url.split_once('?').unwrap_or((&url,""));let base=path.strip_suffix("/api/show").ok_or("Invalid local version path")?;
        url=if query.is_empty(){format!("{base}/api/version")}else{format!("{base}/api/version?{query}")};
    }
    let auth=super::super::provider::authorization(&p)?;
    let mut headers=vec![("Accept","application/json".into())];
    if let Some((name,value))=&auth {headers.push((name.as_str(),value.clone()));}
    let local=text(&p,"protocol")=="local" && !version;
    let body=if local {obj(vec![("model",get(&p,"model").clone()),("verbose",Bool(false))]).string()}else{String::new()};
    if local {headers.push(("Content-Type","application/json".into()));}
    let mut effect=network::wire(&url,boolean(&p,"plain"),if local{"POST"}else{"GET"},&body,headers)?;
    static SERIAL:AtomicU64=AtomicU64::new(1);
    let purpose=format!("{PREFIX}{}:{}-{}",if version{"version"}else{"model"},now(),SERIAL.fetch_add(1,Ordering::Relaxed));
    put(&mut effect,"purpose",s(&purpose));put(&mut effect,"limit",Num(WIRE_LIMIT.to_string()));
    let mut next=st.clone();
    put(&mut next,"metadata_pending",obj(vec![("purpose",s(&purpose)),("identity",p.clone()),("source",s(&endpoint_summary(&url))),("kind",s(if version{"version"}else{"model"}))]));
    put(&mut next,"metadata_binding",p);
    if version {put(&mut next,"metadata_version",Null);put(&mut next,"metadata_version_error",Null);}else{put(&mut next,"metadata_record",Null);put(&mut next,"metadata_error",Null);}
    put(&mut next,"busy",Bool(true));put(&mut next,"notice",s("Reading model-reported metadata…"));
    if next.string().len()>6*1024*1024-8192{return Err("Encrypted history has insufficient room for a bounded metadata observation; export or split large history first".into());}
    *st=next;
    Ok(effect)
}
fn bounded_text(record:&Json,key:&str,max:usize)->Result<Option<String>>{
    match get(record,key){Null=>Ok(None),Str(value) if !value.is_empty() && value.len()<=max && !value.chars().any(char::is_control)=>Ok(Some(value.clone())),_=>Err(format!("Invalid reported {key}"))}
}
fn token_limit(record:&Json,key:&str)->Result<Json>{
    match get(record,key){Null=>Ok(Null),Num(value)=>{let n=value.parse::<u32>().map_err(|_|format!("Invalid reported {key}"))?;if n==0||n>i32::MAX as u32{return Err(format!("Invalid reported {key}"));}Ok(Num(n.to_string()))},_=>Err(format!("Invalid reported {key}"))}
}
fn summary(body:&str,model:&str)->Result<Json>{
    if body.len()>BODY_LIMIT{return Err("Model metadata exceeds 32 KiB".into());}
    let raw=parse(body)?;if !matches!(raw,Obj(_)){return Err("Expected one model-metadata object".into());}
    let name=bounded_text(&raw,"name",256)?.ok_or("Model metadata is missing its resource name")?;
    if name!=format!("models/{model}"){return Err("Reported model identity does not match this connection".into());}
    let mut result=obj(vec![("schema",Num("1".into())),("name",s(&name)),("inputTokenLimit",token_limit(&raw,"inputTokenLimit")?),("outputTokenLimit",token_limit(&raw,"outputTokenLimit")?)]);
    if let Some(version)=bounded_text(&raw,"version",128)?{put(&mut result,"version",s(&version));}
    match get(&raw,"thinking"){Null=>{},Bool(value)=>put(&mut result,"thinking",Bool(*value)),_=>return Err("Invalid reported thinking flag".into())}
    match get(&raw,"supportedGenerationMethods") {
        Null=>{},Arr(methods)=>{
            if methods.len()>32{return Err("Too many reported methods".into());}
            let mut unique=std::collections::BTreeSet::new();let mut bounded=Vec::new();
            for method in methods {let value=method.text()?;if value.is_empty()||value.len()>64||value.bytes().any(|c|!c.is_ascii_alphanumeric()&&c!=b'_')||!unique.insert(value.to_string()){return Err("Invalid or duplicate reported method".into());}bounded.push(s(value));}
            put(&mut result,"supportedGenerationMethods",Arr(bounded));
        },_=>return Err("Invalid reported methods".into())
    }
    Ok(result)
}
// Local /api/show does not echo an authoritative model resource. Keep the
// requested identity visible without claiming that response identity was proven.
fn local_summary(body:&str,model:&str)->Result<Json>{
    if body.len()>BODY_LIMIT{return Err("Model metadata exceeds 32 KiB".into());}
    let raw=parse(body)?;if !matches!(raw,Obj(_)){return Err("Expected one model-metadata object".into());}
    if get(&raw,"error")!=&Null{return Err("Server reported a model-metadata error".into());}
    let mut out=obj(vec![("schema",Num("1".into())),("format",s("local")),("name",s(model))]);
    if let Some(value)=bounded_text(&raw,"modified_at",128)?{put(&mut out,"modified_at",s(&value));}
    match get(&raw,"capabilities"){
        Null=>{},Arr(items)=>{
            if items.len()>32{return Err("Too many reported capabilities".into());}
            let mut unique=std::collections::BTreeSet::new();let mut values=Vec::new();
            for item in items{let value=item.text()?;if value.is_empty()||value.len()>64||value.bytes().any(|b|!b.is_ascii_alphanumeric()&&!b"_-".contains(&b))||!unique.insert(value.to_string()){return Err("Invalid or duplicate reported capability".into());}values.push(s(value));}
            put(&mut out,"capabilities",Arr(values));
        },_=>return Err("Invalid reported capabilities".into())
    }
    match get(&raw,"details"){
        Null=>{},details@Obj(_)=>{for(from,to)in[("format","model_format"),("parameter_size","parameter_size"),("quantization_level","quantization")]{if let Some(value)=bounded_text(details,from,64)?{put(&mut out,to,s(&value));}}},_=>return Err("Invalid reported details".into())
    }
    match get(&raw,"model_info"){
        Null=>{},info@Obj(_)=>{
            if let Some(arch)=bounded_text(info,"general.architecture",64)?{
                if arch.bytes().any(|b|!b.is_ascii_alphanumeric()&&!b"_-".contains(&b)){return Err("Invalid reported architecture".into());}
                put(&mut out,"architecture",s(&arch));
                put(&mut out,"model_context_length",token_limit(info,&format!("{arch}.context_length"))?);
            }
        },_=>return Err("Invalid reported model info".into())
    }
    match get(&raw,"thinking"){
        Null=>{},thinking@Obj(_)=>{
            let valid=|value:&Json|match value{Bool(_)=>true,Str(v)=>!v.is_empty()&&v.len()<=32&&v.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-'),_=>false};
            match get(thinking,"values"){
                Null=>{if get(thinking,"default")!=&Null{return Err("Reported thinking default has no advertised values".into());}},
                Arr(values)=>{
                    if values.len()>16{return Err("Too many reported thinking values".into());}
                    let mut seen=std::collections::BTreeSet::new();for value in values{if !valid(value)||!seen.insert(value.string()){return Err("Invalid or duplicate reported thinking value".into());}}
                    put(&mut out,"thinking_values",Arr(values.clone()));
                    let default=get(thinking,"default");if default!=&Null{if !valid(default)||!values.contains(default){return Err("Reported thinking default is not advertised".into());}put(&mut out,"thinking_default",default.clone());}
                },_=>return Err("Invalid reported thinking values".into())
            }
        },_=>return Err("Invalid reported thinking controls".into())
    }
    Ok(out)
}
pub(super) fn result(st:&mut Json,action:&str,inputs:&Json)->Json{
    if !expected(st,inputs){return Null;}
    let pending=get(st,"metadata_pending").clone();put(st,"metadata_pending",Null);put(st,"busy",Bool(false));
    let read=(||->Result<Json>{
        if action=="failure"{return Err("The metadata request failed".into());}
        let encoded=text(inputs,"data");if encoded.len()>((WIRE_LIMIT+2)/3)*4{return Err("Model metadata exceeds its wire limit".into());}
        let bytes=network::unbase64(&encoded)?;if bytes.len()>WIRE_LIMIT{return Err("Model metadata exceeds its wire limit".into());}
        let(status,body)=network::decoded(&bytes)?;if status!=200{return Err(format!("Model metadata returned HTTP {status}"));}
        if text(&pending,"kind")=="version" {
            if body.len()>4096{return Err("Server version exceeds 4 KiB".into());}
            let raw=parse(&body)?;if !matches!(raw,Obj(_))||get(&raw,"error")!=&Null{return Err("Invalid server-version object".into());}
            let version=bounded_text(&raw,"version",128)?.ok_or("Server version is missing")?;
            if version.bytes().any(|b|!b.is_ascii_alphanumeric()&&!b"._+-".contains(&b)){return Err("Invalid server version".into());}
            return Ok(obj(vec![("version",s(&version)),("source",get(&pending,"source").clone()),("received_ns",s(&now()))]));
        }
        let p=get(&pending,"identity");
        let mut record=if text(p,"protocol")=="local"{local_summary(&body,&text(p,"model"))?}else{summary(&body,&text(p,"model"))?};
        put(&mut record,"source",get(&pending,"source").clone());put(&mut record,"received_ns",s(&now()));Ok(record)
    })();
    put(st,"notice",Null);
    if text(&pending,"kind")=="version" {
        match read {Ok(record)=>{put(st,"metadata_version",record);put(st,"metadata_version_error",Null);},Err(error)=>{put(st,"metadata_version",Null);put(st,"metadata_version_error",s(&format!("Server version unavailable: {error}. Model metadata and manual settings remain available.")));}}
        return Null;
    }
    match read {Ok(record)=>{put(st,"metadata_record",record);put(st,"metadata_error",Null);},Err(error)=>{put(st,"metadata_record",Null);put(st,"metadata_error",s(&format!("Model metadata unavailable: {error}. Manual settings remain available.")));}}
    Null
}
pub(super) fn display(st:&Json)->String{
    let mut shown=model_display(st);
    if text(get(st,"edit"),"protocol")=="local" {
        let version=get(st,"metadata_version");let error=text(st,"metadata_version_error");
        let line=if !error.is_empty(){error}else if version==&Null{"Server version: Not read (optional separate request).".into()}else{format!("Reported server version: {} — {} (query hidden). Observed in this editor session; refresh explicitly. This is a server version advertisement, not API-version negotiation or capability proof.",text(version,"version"),text(version,"source"))};
        shown=format!("{line}\n\n{shown}");
    }
    shown
}
fn model_display(st:&Json)->String{
    if get(st,"metadata_pending")!=&Null{return if text(get(st,"metadata_pending"),"kind")=="version"{"Reading this connection's server version. No model, conversation or prompt is sent."}else{"Reading metadata from this connection. Only its selected model identity is sent; no conversation or prompt."}.into();}
    let error=text(st,"metadata_error");if !error.is_empty(){return error;}
    let r=get(st,"metadata_record");if r==&Null{return "Optional: read Content or local chat model advertisements. Reported capabilities and limits do not change settings or certify execution. Other API formats and ambiguous custom paths retain manual controls.".into();}
    let value=|key:&str|match get(r,key){Num(n)=>n.clone(),Str(s)=>s.clone(),_=>"Not reported".into()};
    if text(r,"format")=="local" {
        let capabilities=match get(r,"capabilities"){Arr(items) if items.is_empty()=>"None reported".into(),Arr(items)=>items.iter().filter_map(|v|v.text().ok()).collect::<Vec<_>>().join(", "),_=>"Not reported".into()};
        let thinking=match get(r,"thinking_values"){Arr(items) if items.is_empty()=>"None reported".into(),Arr(items)=>items.iter().map(Json::string).collect::<Vec<_>>().join(", "),_=>"Not reported".into()};
        let default=if get(r,"thinking_default")==&Null{"Not reported".into()}else{get(r,"thinking_default").string()};
        return format!("Source: {} (query hidden)\nRequested model: {} (response does not attest identity)\nReported modified time: {}\nReported architecture: {}\nReported model context length: {}\nReported format: {}\nReported parameter size: {}\nReported quantization: {}\nReported capabilities: {capabilities}\nReported thinking values: {thinking}\nReported thinking default: {default}\n\nThese are server advertisements, not measured loaded context, permission or successful capability execution. Model context length is not the active num_ctx or an output limit. Missing thinking controls remain unknown even when a thinking capability is listed. No settings changed; manual/server-default controls and separate context/output/compaction budgets remain authoritative. Closing or restarting discards this observation.",value("source"),value("name"),value("modified_at"),value("architecture"),value("model_context_length"),value("model_format"),value("parameter_size"),value("quantization"));
    }
    let methods=match get(r,"supportedGenerationMethods"){Arr(items) if items.is_empty()=>"None reported".into(),Arr(items)=>items.iter().filter_map(|v|v.text().ok()).collect::<Vec<_>>().join(", "),_=>"Not reported".into()};
    let thinking=match get(r,"thinking"){Bool(true)=>"Reported supported; exact levels/budgets not advertised",Bool(false)=>"Reported unsupported",_=>"Not reported"};
    format!("Source: {} (query hidden)\nResource: {}\nVersion: {}\nReported input token limit: {}\nReported output token limit: {}\nReported thinking: {thinking}\nReported methods: {methods}\n\nThis is server-advertised metadata from this editor session, not measured capacity or permission. No settings changed. Input, output and compaction budgets remain distinct; supported methods do not certify streaming, reasoning levels or tools. Refresh explicitly; closing or restarting discards this observation.",value("source"),value("name"),value("version"),value("inputTokenLimit"),value("outputTokenLimit"))
}

#[cfg(test)] mod checks{
    use super::*;
    fn connection()->Json{obj(vec![("protocol",s("content")),("endpoint",s("https://models.example/proxy/v1beta/models/model-x:streamGenerateContent?route=kept&alt=sse")),("model",s("model-x")),("key",s("private-key"))])}
    #[test]fn metadata_identity_url_and_draft_are_separate(){
        let mut p=connection();put(&mut p,"thinking",s("unsupported-draft"));let p=identity(&p).unwrap();assert_eq!(target(&p).unwrap(),"https://models.example/proxy/v1beta/models/model-x?route=kept");
        let mut proxy=p.clone();put(&mut proxy,"endpoint",s("https://models.example/custom"));assert!(target(&proxy).is_err());put(&mut proxy,"endpoint",s("https://models.example/v1beta/models/other:generateContent"));assert!(target(&proxy).is_err());
        let mut st=blank();put(&mut st,"dialog",s("generation"));put(&mut st,"edit",connection());let e=start(&mut st).unwrap();assert!(text(&e,"wire").starts_with("GET /proxy/v1beta/models/model-x?route=kept HTTP/1.1"));assert!(text(&e,"wire").contains("x-goog-api-key: private-key"));assert!(!text(&e,"wire").contains("contents"));
        let p=identity(&obj(vec![("protocol",s("content")),("endpoint",s("https://models.example/v1beta")),("model",s("model-x"))])).unwrap();assert_eq!(target(&p).unwrap(),"https://models.example/v1beta/models/model-x");
    }
    #[test]fn metadata_bounds_and_unknowns_do_not_infer_support(){
        let r=summary(r#"{"name":"models/model-x","thinking":false,"version":"001","inputTokenLimit":1024,"outputTokenLimit":256,"supportedGenerationMethods":["generateContent"]}"#,"model-x").unwrap();assert_eq!(get(&r,"thinking"),&Bool(false));
        let r=summary(r#"{"name":"models/model-x"}"#,"model-x").unwrap();assert_eq!(get(&r,"thinking"),&Null);
        for body in [r#"{"name":"models/other"}"#,r#"{"name":"models/model-x","thinking":"true"}"#,r#"{"name":"models/model-x","inputTokenLimit":-1}"#,r#"{"name":"models/model-x","supportedGenerationMethods":["x","x"]}"#]{assert!(summary(body,"model-x").is_err());}
    }
    #[test]fn metadata_start_capacity_refuses_transactionally(){
        let mut st=blank();put(&mut st,"dialog",s("generation"));put(&mut st,"edit",connection());put(&mut st,"draft",s(&"x".repeat(6*1024*1024-1024)));let prior=st.clone();assert!(start(&mut st).is_err());assert_eq!(st,prior);
    }
    #[test]fn local_metadata_derivation_pins_query_auth_and_model(){
        let p=identity(&obj(vec![("protocol",s("local")),("endpoint",s("http://models.example/proxy/api/chat?route=kept")),("model",s("team/model:tag")),("key",s("secret")),("thinking",s("bad-draft"))])).unwrap();
        assert_eq!(target(&p).unwrap(),"http://models.example/proxy/api/show?route=kept");
        let mut st=blank();put(&mut st,"dialog",s("generation"));put(&mut st,"edit",p);let e=start(&mut st).unwrap();let wire=text(&e,"wire");
        assert!(wire.starts_with("POST /proxy/api/show?route=kept HTTP/1.1"));assert!(wire.contains("Authorization: Bearer secret"));assert!(wire.ends_with(r#"{"model":"team/model:tag","verbose":false}"#));
        assert!(!wire.contains("messages"));let mut custom=get(&st,"edit").clone();put(&mut custom,"endpoint",s("http://models.example/custom"));assert!(target(&custom).is_err());
    }
    #[test]fn local_metadata_whitelist_and_thinking_types(){
        let r=local_summary(r#"{"capabilities":["completion","thinking"],"thinking":{"values":[false,"low","high"],"default":"low"},"model_info":{"general.architecture":"model_arch","model_arch.context_length":131072,"other.context_length":999},"details":{"format":"gguf","parameter_size":"8B","quantization_level":"Q4"},"template":"UNTRUSTED","parameters":"num_ctx 1"}"#,"team/model:tag").unwrap();
        assert_eq!(get(&r,"model_context_length"),&Num("131072".into()));assert_eq!(get(&r,"thinking_default"),&s("low"));assert_eq!(get(&r,"template"),&Null);assert_eq!(get(&r,"parameters"),&Null);
        let r=local_summary(r#"{"capabilities":["thinking"],"model_info":{"other.context_length":999}}"#,"x").unwrap();assert_eq!(get(&r,"model_context_length"),&Null);assert_eq!(get(&r,"thinking_values"),&Null);
        for body in [r#"{"thinking":{"values":[true,true]}}"#,r#"{"thinking":{"values":[true],"default":false}}"#,r#"{"thinking":{"values":[1]}}"#,r#"{"thinking":{"default":true}}"#,r#"{"capabilities":["x","x"]}"#,r#"{"model_info":{"general.architecture":"arch","arch.context_length":0}}"#,r#"{"error":"NOT_FOUND"}"#]{assert!(local_summary(body,"x").is_err(),"{body}");}
    }
    #[test]fn local_metadata_observation_never_changes_generation(){
        let mut st=blank();put(&mut st,"dialog",s("generation"));put(&mut st,"edit",obj(vec![("protocol",s("local")),("endpoint",s("http://models.example/api/chat")),("model",s("x")),("thinking",s("off")),("context_tokens",s("2048"))]));
        let prior=get(&st,"edit").clone();let e=start(&mut st).unwrap();let body=r#"{"capabilities":["thinking"],"thinking":{"values":[true,false],"default":true}}"#;
        let wire=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",body.len());let input=obj(vec![("purpose",get(&e,"purpose").clone()),("data",s(&network::b64(wire.as_bytes())))]);result(&mut st,"network_result",&input);
        assert_eq!(get(&st,"edit"),&prior);assert!(!boolean(&st,"busy"));assert!(display(&st).contains("does not attest identity"));assert!(display(&st).contains("not the active num_ctx"));let snapshot=st.clone();assert_eq!(result(&mut st,"network_result",&input),Null);assert_eq!(st,snapshot);
    }

    #[test]fn local_server_version_is_separate_information_and_bound_to_purpose(){
        let mut st=blank();put(&mut st,"dialog",s("generation"));put(&mut st,"edit",obj(vec![("protocol",s("local")),("endpoint",s("http://server.example/proxy/api/chat?route=kept")),("model",s("x")),("key",s("secret"))]));
        put(&mut st,"metadata_record",local_summary(r#"{"capabilities":["thinking"]}"#,"x").unwrap());
        let e=version(&mut st).unwrap();assert!(text(&e,"purpose").starts_with("model_metadata:version:"));let wire=text(&e,"wire");assert!(wire.starts_with("GET /proxy/api/version?route=kept HTTP/1.1"));assert!(wire.ends_with("\r\n\r\n"));assert!(wire.contains("Authorization: Bearer secret"));
        let body=r#"{"version":"0.17.7-custom+build","unknown":"IGNORED"}"#;let wire=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",body.len());
        let input=obj(vec![("purpose",get(&e,"purpose").clone()),("data",s(&network::b64(wire.as_bytes())))]);result(&mut st,"network_result",&input);
        assert_eq!(text(get(&st,"metadata_version"),"version"),"0.17.7-custom+build");assert_eq!(get(get(&st,"metadata_record"),"capabilities"),&Arr(vec![s("thinking")]));assert!(display(&st).contains("not API-version negotiation"));
        let model=start(&mut st).unwrap();assert!(text(&model,"purpose").starts_with("model_metadata:model:"));let prior=st.clone();result(&mut st,"network_result",&input);assert_eq!(st,prior);assert!(get(&st,"metadata_version")!=&Null);
        reconcile(&mut st,"close");assert_eq!(get(&st,"metadata_version"),&Null);
    }

}
