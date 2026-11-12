//! Non-executing feedback for model responses that violate the proposal envelope.
use super::*;

pub(super) const FEEDBACK: &str = "Proposal format review: if the current task needs a tool, return one bare JSON object containing tool and args. If the previous JSON was only an example, answer in ordinary text and keep examples fenced or quoted. The controller returns the actual results of permitted operations.";
const NOTICE: &str = "Tool-shaped JSON inside text was not registered as a proposal. Tap this message for the raw response. Conversation actions → Request format correction sends one explicit correction request; a corrected local tool proposal still requires approval.";

fn envelope(value: &Json) -> bool {
    matches!(value,Obj(o) if o.len()==2 && matches!(o.get("tool"),Some(Str(name)) if harness::known_tool(name)) && matches!(o.get("args"),Some(Obj(_))))
}
// Scan balanced data as a single unit. Nested config examples cannot become
// proposals. A single candidate is data for an explicit user review action;
// neither this scanner nor reply ingestion dispatches recovered arguments.
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
pub(super) fn proposal_phase(st:&Json)->bool {let run=get(st,"task_run");!boolean(st,"web_answer_only")&&(run==&Null||matches!(text(run,"phase").as_str(),"baseline"|"probe"|"verify"|"delegate"))}
fn request_prefix(text:&str)->bool{
    let Some(fields)=text.strip_prefix('{')else{return false;};
    let fields=fields.trim_start();
    !fields.is_empty()&&["\"tool\"","\"args\""].iter().any(|key|key.starts_with(fields)||fields.strip_prefix(key).is_some_and(|rest|rest.trim_start().starts_with(':')))
}
fn scan(reply:&str,preview:bool)->(Vec<std::ops::Range<usize>>,Option<Json>) {
    if reply.len()>1024*1024 || (!preview&&parse(reply.trim()).is_ok()){return(Vec::new(),None);}
    let mut ranges=Vec::new();let mut single=None;
    let projection=reasoning_text::project(reply,false);if projection.pending{return(Vec::new(),None);}
    let b=reply.as_bytes();let mut i=if projection.hidden{reply.len()-projection.text.len()}else{0};let mut candidates=0;let mut fenced:Option<(u8,usize)>=None;
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
        if matches!(b[i],b'{'|b'['){let end=object_end(b,i);candidates+=1;if candidates>32{return(ranges,None);}if end-i<=65536&&reply.is_char_boundary(end){let item=&reply[i..end];match parse(item){
            Ok(value) if envelope(&value)=>{single=if ranges.is_empty(){Some(value)}else{None};ranges.push(i..end);}
            Err(_) if preview&&end==b.len()&&request_prefix(item)=>ranges.push(i..end),
            _=>{}
        }}i=end;continue;}
        i+=1;
    }
    (ranges,single)
}
pub(super) fn embedded(reply:&str)->bool{!scan(reply,false).0.is_empty()}

fn without_requests(raw:&str,ranges:Vec<std::ops::Range<usize>>)->String{
    let mut visible=raw.to_owned();
    for range in ranges.into_iter().rev(){visible.replace_range(range,"");}
    visible
}
pub(super) fn display(message:&Json)->String {
    if boolean(message,"embedded_tool_json") {
        let raw=text(message,"content");
        let visible=without_requests(&raw,scan(&raw,false).0);
        let prose=assistant_display(&visible,false);
        let status=if boolean(message,"recovered_search"){"Search request recovered. Tap for details."}else if boolean(message,"recovered_page"){"Page request recovered. Tap for details."}else if boolean(message,"format_retry_used"){"The model used an unsupported request format. Tap for response details."}else{"The model used an unsupported request format. Tap to review the request or ask for a correction."};
        if prose.trim().is_empty(){status.into()}else{format!("{}\n\n{status}",prose.trim())}
    }else{"The model returned unsupported tool-call markup. This markup does not execute a tool. Tap this message to inspect the raw response. Use Conversation actions → Regenerate reply in new branch to retry.".into()}
}
pub(super) fn preview(message:&Json)->String{
    let raw=text(message,"content");
    let streaming=boolean(message,"streaming");
    if streaming||boolean(message,"interrupted"){
        let ranges=scan(&raw,true).0;
        if !ranges.is_empty(){
            let prose=without_requests(&raw,ranges);
            let visible=assistant_display(&prose,streaming);
            if !visible.trim().is_empty(){return visible.trim().into();}
            return if streaming{"Preparing a tool request…"}else{"Tool request interrupted. Tap for details."}.into();
        }
    }
    let visible=assistant_display(&raw,streaming);
    if visible.trim_start().starts_with('{')&&parse(visible.trim()).is_ok_and(|value|envelope(&value)){
        return "Tool request. Tap for details.".into();
    }
    visible
}
pub(super) fn request_context(st:&mut Json,profile:&Json,agent:&Json){
    put(st,"format_request_profile",profile.clone());put(st,"format_request_agent",agent.clone());
    // Optional correction bookkeeping must never prevent an ordinary send.
    // Reserve space after all ordinary request metadata has been assembled.
    if super::super::mcp_schema::state_bytes(st)>6*1024*1024-16384 {if let Obj(fields)=st {fields.remove("format_request_profile");fields.remove("format_request_agent");}}
}
pub(super) fn clear(st:&mut Json){put(st,"format_pending",Null);put(st,"format_retry_request",Bool(false));put(st,"format_correction_prepare",Null);}
fn recoverable(st:&Json)->bool {
    let p=get(st,"format_pending");let messages=array(&chat(st),"messages");
    p!=&Null&&!boolean(st,"busy")&&text(p,"chat")==text(st,"active")&&number(p,"messages")==messages.len()&&messages.last().map(|m|boolean(m,"embedded_tool_json")).unwrap_or(false)
}
pub(super) fn available(st:&Json)->bool {
    recoverable(st)&&array(&chat(st),"messages").last().is_some_and(|m|!boolean(m,"format_retry_used"))
}
fn origin(st:&Json)->Result<Json>{
    if !recoverable(st){return Err("No current reply is eligible for request recovery".into());}
    let p=get(st,"format_pending").clone();let index=number(&p,"index");
    let agent=if get(st,"task_run")==&Null{harness::active_agent(st)}else{get(get(st,"task_run"),"agent").clone()};
    if array(st,"profiles").get(index)!=Some(get(&p,"profile"))||get(st,"task_run")!=get(&p,"task")||agent!=*get(&p,"agent"){return Err("The original connection or task changed; no request was recovered".into());}
    Ok(p)
}
fn recoverable_reply(st:&Json)->Result<String>{
    origin(st)?;
    if get(st,"pending")!=&Null||!proposal_phase(st)||!harness::has_operation_room(st){return Err("Finish the current review or task before recovering another request".into());}
    let messages=array(&chat(st),"messages");let message=messages.last().ok_or("No response is available")?;
    Ok(text(message,"content"))
}
fn candidate(st:&Json,automatic:bool)->Result<Json>{
    let reply=recoverable_reply(st)?;
    let (ranges,single)=scan(&reply,false);
    let p=single.ok_or("The reply does not contain exactly one unquoted, unfenced request; ask for a format correction")?;
    if automatic&&!ranges.first().is_some_and(|span|web_request_prose(&reply,span.start,text(&p,"tool")=="web_read")){return Err("The wrapped request needs a clear action or a format correction".into());}
    harness::allowed_tool(st,&p)?;validate_tool(&p)?;Ok(p)
}
fn leading_prose(reply:&str,start:usize)->String{
    let before=reply[..start].trim_end();
    before.rsplit(['\n','.','!','?']).find(|part|part.chars().any(char::is_alphabetic)).unwrap_or("").to_ascii_lowercase()
}
fn prose_words(label:&str)->Vec<&str>{
    label.split(|c:char|!c.is_alphanumeric()).filter(|word|!word.is_empty()).collect()
}
fn non_request_prose(words:&[&str])->bool{
    matches!(words,
        ["example"|"sample"|"schema"|"template"|"illustration"|"illustrative",..]
        |["for","example"|"illustration"|"documentation",..]
        |["documentation","payload"|"example"|"sample",..]
        |["do","not","execute"|"run"|"send"|"open"|"read"|"visit"|"fetch",..]
        |["don","t","execute"|"run"|"send"|"open"|"read"|"visit"|"fetch",..]
        |["never","execute"|"run"|"send"|"open"|"read"|"visit"|"fetch",..]
        |["i","will"|"am"|"ll"|"m","not"|"never",..]|["i","won","t",..])
        || matches!(words,["here"|"this","is"|"s",rest @ ..] if rest.iter().take(6).any(|word|matches!(*word,"example"|"sample"|"schema"|"template"|"illustration"|"demonstration")))
        || matches!(words,["i","will"|"ll","show"|"give"|"provide"|"present"|"explain"|"write",rest @ ..] if rest.iter().take(6).any(|word|matches!(*word,"example"|"sample"|"schema"|"template"|"illustration"|"demonstration")))
        || matches!(words,["looking","at","this"|"the","example"|"sample"|"schema"|"template",..])
        || (words.contains(&"how")&&words.windows(2).any(|pair|matches!(pair,["would"|"should","look"])))
}
fn web_request_prose(reply:&str,start:usize,page:bool)->bool{
    let label=leading_prose(reply,start);
    let words=prose_words(&label);
    if non_request_prose(&words){return false;}
    if page{
        return match words.as_slice(){
            ["page"|"web","request",..]|["opening"|"reading"|"visiting"|"fetching",..]=>true,
            ["i","ll"|"will"|"am"|"m",rest @ ..]|["let","me",rest @ ..]=>
                rest.iter().copied().find(|word|!matches!(*word,"now"|"first"|"directly"|"try"|"going"|"to"))
                    .is_some_and(|word|matches!(word,"open"|"opening"|"read"|"reading"|"visit"|"visiting"|"fetch"|"fetching")),
            _=>false,
        };
    }
    if words.windows(5).any(|w|matches!(w,["so","let","me","try","the"|"another"]))
        && !words.iter().any(|w|matches!(*w,"example"|"sample"|"schema"|"template")){return true;}
    match words.as_slice(){
        ["proposal",..]|["visible","proposal",..]|["tool"|"search","request",..]
            |["here","is","the","tool"|"search","request",..]
            |["searching"]|["searching","for"|"the"|"now",..]|["looking","for"|"up",..]=>true,
        ["i","ll"|"will"|"am"|"m",rest @ ..]|["let","me",rest @ ..]|["need","to",rest @ ..]=>
            rest.iter().copied().skip_while(|word|matches!(*word,"now"|"first"|"quickly"|"just"|"start"|"begin"|"by"|"going"|"to"))
                .next().is_some_and(|word|matches!(word,"search"|"searching"|"look"|"looking"|"find"|"finding"|"check"|"checking"|"browse"|"browsing"|"query"|"querying"|"call"|"calling")),
        _=>false,
    }
}
pub(super) fn review_available(st:&Json)->bool{candidate(st,false).is_ok()}
pub(super) fn review(st:&mut Json)->Result<()>{
    let request=candidate(st,false)?;
    stage_review(st,request)
}
fn stage_review(st:&mut Json,mut p:Json)->Result<()>{
    let mut next=st.clone();
    if get(&next,"task_run")!=&Null{
        if harness::model_reply(&mut next,&p.string())?!=Some(Null){return Err("The current task could not prepare a request review".into());}
    }else{
        if text(&p,"tool")=="delegate"{delegation::unreviewed(&mut p);}
        if web_operation::is_web(&p){web_operation::own_model(&next,&mut p);}
        claim_records::own(&next,&mut p)?;experiment_records::own(&next,&mut p)?;
        put(&mut next,"pending",p);put(&mut next,"agent_pending",Bool(true));put(&mut next,"dialog",s("approval"));
    }
    let mut pending=get(&next,"pending").clone();put(&mut pending,"format_review",Bool(true));put(&mut next,"pending",pending);
    put(&mut next,"format_pending",Null);put(&mut next,"branch_source",Null);put(&mut next,"branch_index",Null);
    put(&mut next,"notice",s("Review the recovered request and its exact arguments. Nothing has been submitted; this request requires explicit approval."));
    if super::super::mcp_schema::state_bytes(&next)>6*1024*1024-8192{return Err("The recovered review does not fit current saved state; raw reply and current work retained".into());}
    *st=next;Ok(())
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
    let p=if !context_available {Null}else{obj(vec![("chat",s(&text(st,"active"))),("messages",Num(array(&chat(st),"messages").len().to_string())),("index",get(st,"request_index").clone()),("profile",get(st,"format_request_profile").clone()),("agent",get(st,"format_request_agent").clone()),("task",get(st,"task_run").clone())])};
    put(st,"format_pending",p);put(st,"format_retry_request",Bool(false));put(st,"busy",Bool(false));
    if super::super::mcp_schema::state_bytes(st)>6*1024*1024-8192 {put(st,"format_pending",Null);}
    let unavailable=!retried&&get(st,"format_pending")==&Null;
    put(st,"notice",s(if unavailable{"Tool-shaped JSON inside text was not registered as a proposal. The raw reply is retained, but the original correction context is unavailable within saved-state limits. No local proposal was executed."}else if retried{"The model still used an unsupported request format. Tap the message to review a recoverable request or inspect the response. No operation ran."}else{NOTICE}));
    put(st,"notice_error",Bool(true));Ok(())
}
pub(super) fn recover_web(st:&mut Json)->bool{
    let Ok(request)=candidate(st,true)else{return false;};
    let page=text(&request,"tool")=="web_read";
    if !(search::standing(st,&request)||web_operation::standing_read(st,&request))||stage_review(st,request).is_err(){return false;}
    let mut pending=get(st,"pending").clone();
    if let Obj(fields)=&mut pending{fields.remove("format_review");}
    put(st,"pending",pending);
    let mut current=chat(st);let mut messages=array(&current,"messages");
    if let Some(last)=messages.last_mut(){put(last,if page{"recovered_page"}else{"recovered_search"},Bool(true));}
    put(&mut current,"messages",Arr(messages));replace_chat(st,current);
    put(st,"notice",s(if page{"Recovered the requested public page read."}else{"Recovered the requested web search."}));
    put(st,"notice_error",Bool(false));
    true
}
pub(super) fn correct_web(st:&mut Json)->Option<Json>{
    if !available(st){return None;}
    let reply=recoverable_reply(st).ok()?;
    let (ranges,single)=scan(&reply,false);
    let search_only=!ranges.is_empty()&&ranges.iter().all(|span|!non_request_prose(&prose_words(&leading_prose(&reply,span.start)))&&parse(&reply[span.clone()]).is_ok_and(|request|
        search::standing(st,&request)&&harness::allowed_tool(st,&request).is_ok()&&validate_tool(&request).is_ok()));
    let read_correction=single.is_some_and(|request|
        ranges.first().is_some_and(|span|!non_request_prose(&prose_words(&leading_prose(&reply,span.start))))
        && web_operation::standing_read(st,&request)&&harness::allowed_tool(st,&request).is_ok()&&validate_tool(&request).is_ok());
    if !search_only&&!read_correction{return None;}
    let effect=correction(st).ok()?;
    put(st,"notice",s("Asking the model to correct its web request format…"));
    put(st,"notice_error",Bool(false));
    Some(effect)
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
 #[test]fn single_review_candidate_preserves_data_boundaries(){
  let j=r#"{"tool":"web_read","args":{"url":"https://example.test"}}"#;
  assert!(scan(&format!("Observation retained.\n{j}"),false).1.is_some());
  assert!(scan(&format!("First:\n{j}\nSecond:\n{j}"),false).1.is_none());
  assert!(!embedded(&format!("<think>Consider {j}</think>Ordinary answer.")));
  assert!(scan(&format!("<think>Consider {j}</think>Proposal:\n{j}"),false).1.is_some());
  assert!(!embedded(&format!("<think>Unfinished {j}")));
 }
}
