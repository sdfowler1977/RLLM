//! Non-executing feedback for model responses that violate the proposal envelope.
use super::*;

pub(super) const FEEDBACK: &str = "Tool-shaped JSON was found inside text and was not registered as a proposal. No local proposal was executed. If you intended a tool request, return the entire response as one bare JSON object with tool and args, without commentary or fences. Otherwise answer in ordinary text and put illustrative JSON in a fenced or quoted example. A valid proposal still requires user approval. Do not claim a tool ran without an actual tool result.";
const NOTICE: &str = "Tool-shaped JSON inside text was not registered as a proposal. Tap this message for the raw response. Conversation actions → Request format correction sends one explicit correction request; a corrected local tool proposal still requires approval.";

fn envelope(value: &Json) -> bool {
    matches!(value,Obj(o) if o.len()==2 && matches!(o.get("tool"),Some(Str(name)) if harness::known_tool(name)) && matches!(o.get("args"),Some(Obj(_))))
}
// Scan balanced data as a single unit. Nested config examples cannot become
// proposals. This scanner only identifies an error; it never returns arguments.
fn object_end(b:&[u8], start:usize)->usize {
    let mut stack=Vec::new();let mut quoted=false;let mut escaped=false;
    for i in start..b.len().min(start.saturating_add(65536)) {
        let c=b[i];
        if quoted {if escaped {escaped=false;}else if c==b'\\'{escaped=true;}else if c==b'"'{quoted=false;}continue;}
        match c {
            b'"'=>quoted=true,
            b'{'|b'['=>{if stack.len()==64{return b.len();}stack.push(c);},
            b'}'|b']'=>{let expected=if c==b'}'{b'{'}else{b'['};if stack.pop()!=Some(expected){return i+1;}if stack.is_empty(){return i+1;}},
            _=>{}
        }
    }
    b.len()
}
pub(super) fn proposal_phase(st:&Json)->bool {let run=get(st,"task_run");run==&Null||matches!(text(run,"phase").as_str(),"baseline"|"probe"|"delegate")}
pub(super) fn embedded(reply:&str)->bool {
    if reply.len()>1024*1024 || parse(reply.trim()).is_ok(){return false;}
    let b=reply.as_bytes();let mut i=0;let mut candidates=0;let mut fenced:Option<(u8,usize)>=None;
    while i<b.len(){
        if i==0||b[i-1]==b'\n' {
            let end=b[i..].iter().position(|c|*c==b'\n').map(|v|i+v).unwrap_or(b.len());
            let mut p=i;while p<end&&matches!(b[p],b' '|b'\t'){p+=1;}
            let marker=if p<end{b[p]}else{0};let mut n=0;while p+n<end&&b[p+n]==marker{n+=1;}
            if let Some((ch,count))=fenced {if marker==ch&&n>=count&&b[p+n..end].iter().all(u8::is_ascii_whitespace){fenced=None;}i=end.saturating_add(1);continue;}
            if matches!(marker,b'`'|b'~')&&n>=3 {fenced=Some((marker,n));i=end.saturating_add(1);continue;}
            if marker==b'>'||p-i>=4||b.get(i)==Some(&b'\t'){i=end.saturating_add(1);continue;}
        }
        if b[i]==b'`' {let start=i;while i<b.len()&&b[i]==b'`'{i+=1;}let count=i-start;let mut p=i;let mut found=None;while p<b.len(){if b[p]==b'`'{let q=p;while p<b.len()&&b[p]==b'`'{p+=1;}if p-q==count{found=Some(p);break;}}else{p+=1;}}i=found.unwrap_or(b.len());continue;}
        if b[i]==b'"'||(b[i]==b'\''&&(i==0||!b[i-1].is_ascii_alphanumeric())) {
            let quote=b[i];i+=1;let mut escaped=false;while i<b.len(){let c=b[i];i+=1;if escaped{escaped=false;}else if c==b'\\'{escaped=true;}else if c==quote{break;}}continue;
        }
        if reply.is_char_boundary(i)&&(reply[i..].starts_with('“')||reply[i..].starts_with('‘')){
            let end_quote=if reply[i..].starts_with('“'){'”'}else{'’'};i+=3;
            i=reply[i..].find(end_quote).map(|p|i+p+3).unwrap_or(b.len());continue;
        }
        if matches!(b[i],b'{'|b'['){let end=object_end(b,i);candidates+=1;if candidates>32{return false;}if end-i<=65536&&reply.is_char_boundary(end){if let Ok(value)=parse(&reply[i..end]){if envelope(&value){return true;}}}i=end;continue;}
        i+=1;
    }
    false
}

pub(super) fn display(message:&Json)->String {
    if boolean(message,"embedded_tool_json") {"Tool-shaped JSON inside text was not registered as a proposal. No local proposal was executed. Tap this message to inspect the raw response. Any corrected proposal still requires approval.".into()}else{"The model returned unsupported tool-call markup. This markup does not execute a tool. Tap this message to inspect the raw response. Use Conversation actions → Regenerate reply in new branch to retry.".into()}
}
pub(super) fn request_context(st:&mut Json,profile:&Json,agent:&Json){
    put(st,"format_request_profile",profile.clone());put(st,"format_request_agent",agent.clone());
    // Optional correction bookkeeping must never prevent an ordinary send.
    // Reserve space after all ordinary request metadata has been assembled.
    if super::super::mcp_schema::state_bytes(st)>6*1024*1024-16384 {if let Obj(fields)=st {fields.remove("format_request_profile");fields.remove("format_request_agent");}}
}
pub(super) fn clear(st:&mut Json){put(st,"format_pending",Null);put(st,"format_retry_request",Bool(false));put(st,"format_correction_prepare",Null);}
pub(super) fn available(st:&Json)->bool {
    let p=get(st,"format_pending");let messages=array(&chat(st),"messages");
    p!=&Null&&!boolean(st,"busy")&&text(p,"chat")==text(st,"active")&&number(p,"messages")==messages.len()&&messages.last().map(|m|boolean(m,"embedded_tool_json")&&!boolean(m,"format_retry_used")).unwrap_or(false)
}
pub(super) fn record(st:&mut Json,reply:&str,inputs:&Json)->Result<()> {
    let retried=boolean(st,"format_retry_request");
    let mut next=st.clone();put(&mut next,"stream_text",Null);
    let captured=append(&mut next,"assistant",reply).is_ok();
    if captured {
        let mut c=chat(&next);let mut m=array(&c,"messages");if let Some(last)=m.last_mut(){put(last,"embedded_tool_json",Bool(true));put(last,"tool_format_error",Bool(true));put(last,"format_retry_used",Bool(retried));}
        put(&mut c,"messages",Arr(m));replace_chat(&mut next,c);
        harness::finish_attempt(&mut next,inputs,"error");
    }
    if !captured || super::super::mcp_schema::state_bytes(&next)>6*1024*1024-8192 {
        put(st,"stream_text",Null);harness::finish_attempt(st,inputs,"error");clear(st);put(st,"busy",Bool(false));
        put(st,"notice",s("Tool-shaped JSON inside text was not registered as a proposal. History capacity prevented capturing the raw reply or offering correction; existing messages and task are retained. No local proposal was executed."));put(st,"notice_error",Bool(true));return Ok(());
    }
    *st=next;
    let context_available=get(st,"format_request_profile")!=&Null&&get(st,"format_request_agent")!=&Null;
    let p=if retried||!context_available {Null}else{obj(vec![("chat",s(&text(st,"active"))),("messages",Num(array(&chat(st),"messages").len().to_string())),("index",get(st,"request_index").clone()),("profile",get(st,"format_request_profile").clone()),("agent",get(st,"format_request_agent").clone()),("task",get(st,"task_run").clone())])};
    put(st,"format_pending",p);put(st,"format_retry_request",Bool(false));put(st,"busy",Bool(false));
    if super::super::mcp_schema::state_bytes(st)>6*1024*1024-8192 {put(st,"format_pending",Null);}
    let unavailable=!retried&&get(st,"format_pending")==&Null;
    put(st,"notice",s(if unavailable{"Tool-shaped JSON inside text was not registered as a proposal. The raw reply is retained, but the original correction context is unavailable within saved-state limits. No local proposal was executed."}else if retried{"The correction still contained tool-shaped JSON in text. No proposal was registered and no local proposal was executed. The one correction attempt is exhausted; raw replies are retained."}else{NOTICE}));
    put(st,"notice_error",Bool(true));Ok(())
}
pub(super) fn correction(st:&mut Json)->Result<Json>{
    if !available(st){return Err("No current reply is eligible for a format correction".into());}
    let p=get(st,"format_pending").clone();let index=number(&p,"index");
    let agent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};
    if array(st,"profiles").get(index)!=Some(get(&p,"profile"))||get(st,"task_run")!=get(&p,"task")||agent!=*get(&p,"agent") {return Err("The original connection or task changed; no correction was sent".into());}
    let mut next=st.clone();put(&mut next,"format_correction_prepare",Bool(true));put(&mut next,"format_pending",Null);
    let mut c=chat(&next);let mut messages=array(&c,"messages");if let Some(last)=messages.last_mut(){put(last,"format_retry_used",Bool(true));}put(&mut c,"messages",Arr(messages));replace_chat(&mut next,c);
    put(&mut next,"dialog",Null);
    let effect=model_effect_at(&mut next,index)?;
    let bytes=super::super::mcp_schema::state_bytes(&next);let policy=super::super::resources::policy(get(&next,"device"),get(&next,"resources"),bytes);
    if bytes>6*1024*1024-8192||number(&policy,"response_bytes")<65536{return Err("Current saved-state or memory budget cannot admit a format correction; the raw response and correction option are retained".into());}
    *st=next;Ok(effect)
}
pub(super) fn messages(st:&Json,messages:&mut Vec<Json>){
    if !boolean(st,"format_correction_prepare"){return;}
    if get(st,"task_run")!=&Null{if let Some(last)=array(&chat(st),"messages").last(){messages.push(obj(vec![("role",s("assistant")),("content",get(last,"content").clone())]));}}
    messages.push(obj(vec![("role",s("user")),("content",s(FEEDBACK))]));
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn strict_envelope_and_data_examples(){
  let j=r#"{"tool":"web_read","args":{"url":"https://example.test"}}"#;
  assert!(embedded(&format!("I will read this.\n{j}")));
  for t in [j.to_string(),format!("Example: `{j}`"),format!("Example:\n```json\n{j}\n```"),format!("Example:\n~~~json\n{j}\n~~~"),format!("Quoted: '{j}'"),format!("Quoted: “{j}”"),format!("> {j}"),format!("    {j}"),format!("Config: {{\"example\":{j}}}"),format!("Data: [{j}]"),r#"data {"tool":"other","args":{}}"#.into(),r#"data {"tool":"web_read","args":[]}"#.into(),r#"schema {"tool":"web_read","args":{},"description":"example"}"#.into()]{assert!(!embedded(&t),"{t}");}
  assert!(embedded(&format!("That's the issue. Re-proposing:\n{j}")));
 }
}
