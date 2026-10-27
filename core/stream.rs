use super::*;
use std::sync::{Mutex, OnceLock};

const LIMIT: usize = 2 * 1024 * 1024;
const TEXT_LIMIT: usize = 1024 * 1024;

/// Incremental text is provisional. A completed reply is returned only after
/// the ordinary HTTP decoder validates the complete wire response.
#[derive(Default)]
struct Transfer {
    chat: bool,
    mcp: bool,
    messages: bool,
    content: bool,
    content_parts: usize,
    message: Option<Json>,
    block: Option<(usize, bool)>,
    blocks: usize,
    text_blocks: usize,
    message_delta: bool,
    event_name: String,
    mcp_session: String,
    raw: Vec<u8>,
    offset: usize,
    headers: bool,
    interim: usize,
    expected: Option<usize>,
    chunked: bool,
    remaining: usize,
    terminator: bool,
    trailers: bool,
    json: bool,
    line: Vec<u8>,
    text: String,
    terminal: Option<Json>,
    event: String,
    carriage: bool,
    first_line: bool,
    finish_reason: Option<String>,
    usage: Option<Json>,
}
impl Transfer {
    // Messages events are text-only presentation. Thinking and extension blocks
    // are not transcript text; structured tool execution is not implemented here.
    fn message_event(&mut self, record: &Json, event_name: &str) -> Result<()> {
        let kind=field(record,"type")?;
        if !event_name.is_empty() && event_name!=kind {return Err("Messages event name does not match its data".into());}
        if kind=="error" {return Err(format!("Server error: {}",record.get("error")?.string()));}
        if kind=="ping" {return Ok(());}
        match kind {
            "message_start" => {
                if self.message.is_some() {return Err("Duplicate Messages start".into());}
                let message=record.get("message")?;
                if field(message,"type")? != "message" || field(message,"role")? != "assistant" || !message.get("content")?.array()?.is_empty() {
                    return Err("Invalid Messages start".into());
                }
                let id=field(message,"id")?;let model=field(message,"model")?;
                if id.len()>256 || model.len()>256 {return Err("Messages identity exceeds limit".into());}
                self.message=Some(obj(vec![("id",s(id)),("type",s("message")),("role",s("assistant")),("model",s(model))]));
                if let Ok(usage)=message.get("usage") {self.message_usage(usage)?;}
            },
            "content_block_start" => {
                if self.message.is_none() || self.block.is_some() || self.message_delta || self.blocks>=256 {return Err("Unexpected Messages content block".into());}
                let index=message_index(record)?;
                if index!=self.blocks {return Err("Messages content block index is not sequential".into());}
                let block=record.get("content_block")?;let block_kind=field(block,"type")?;
                if ["tool_use","server_tool_use"].contains(&block_kind) {return Err("Structured Messages tool blocks are not supported; no tool was executed".into());}
                let text=block_kind=="text";
                if text {if self.text_blocks>0 {self.append("\n")?;}self.text_blocks+=1;self.append(field(block,"text")?)?;}
                self.block=Some((index,text));self.blocks+=1;
            },
            "content_block_delta" => {
                let (index,is_text)=self.block.ok_or("Messages delta has no open block")?;
                if index!=message_index(record)? {return Err("Messages delta has the wrong block index".into());}
                let delta=record.get("delta")?;let kind=field(delta,"type")?;
                if kind=="text_delta" {if !is_text {return Err("Text delta does not belong to a text block".into());}self.append(field(delta,"text")?)?;}
                else if is_text && ["thinking_delta","signature_delta","input_json_delta"].contains(&kind) {return Err("Non-text delta does not belong to a text block".into());}
                // Other typed deltas are not promoted to generated text or tools.
            },
            "content_block_stop" => {
                let (index,_)=self.block.ok_or("Messages stop has no open block")?;
                if index!=message_index(record)? {return Err("Messages stop has the wrong block index".into());}
                self.block=None;
            },
            "message_delta" => {
                if self.message.is_none() || self.block.is_some() {return Err("Unexpected Messages message delta".into());}
                self.message_delta=true;
                let delta=record.get("delta")?;
                if let Ok(reason)=delta.get("stop_reason") {if reason!=&Null {let reason=reason.text()?;if reason.is_empty() || reason.len()>128 {return Err("Invalid Messages stop reason".into());}self.finish_reason=Some(reason.to_string());}}
                if let Ok(usage)=record.get("usage") {self.message_usage(usage)?;}
            },
            "message_stop" => {
                if !self.message_delta || self.block.is_some() {return Err("Messages stream stopped before its content completed".into());}
                let reason=self.finish_reason.as_ref().ok_or("Messages stream has no stop reason")?;
                if ["tool_use","pause_turn"].contains(&reason.as_str()) {return Err("Messages reply requires an unsupported structured continuation".into());}
                let mut message=self.message.take().ok_or("Messages stream has no start")?;
                if let Obj(fields)=&mut message {
                    fields.insert("content".into(),Arr(vec![obj(vec![("type",s("text")),("text",s(&self.text))])]));
                    fields.insert("stop_reason".into(),s(reason));
                    if let Some(usage)=&self.usage {fields.insert("usage".into(),usage.clone());}
                }
                self.terminal=Some(message);
            },
            _ => {}, // Forward-compatible event extensions do not change phases.
        }
        Ok(())
    }
    fn message_usage(&mut self, usage:&Json)->Result<()> {
        let Obj(fields)=usage else {return Err("Invalid Messages usage".into());};
        // Counts are cumulative provider reports, never summed or tokenized here.
        let mut saved=match self.usage.take(){Some(Obj(fields))=>fields,_=>BTreeMap::new()};
        for key in ["input_tokens","output_tokens","cache_creation_input_tokens","cache_read_input_tokens"] {
            if let Some(value)=fields.get(key) {
                let Num(n)=value else {return Err("Invalid Messages token count".into());};
                n.parse::<u64>().map_err(|_|"Invalid Messages token count")?;
                saved.insert(key.into(),value.clone());
            }
        }
        self.usage=Some(Obj(saved));Ok(())
    }
    fn content_event(&mut self, record: &Json) -> Result<()> {
        let Obj(_) = record else { return Err("Invalid Content stream record".into()); };
        if let Ok(error)=record.get("error") {return Err(format!("Server error: {}",error.string()));}
        if let Ok(feedback)=record.get("promptFeedback") {
            if let Ok(reason)=feedback.get("blockReason") {if reason!=&Null && !reason.text()?.is_empty() && reason.text()?!="BLOCK_REASON_UNSPECIFIED" {return Err(format!("Content prompt was blocked: {}",reason.text()?));}}
        }
        if let Ok(usage)=record.get("usageMetadata") {
            let Obj(fields)=usage else{return Err("Invalid Content usage".into());};
            let mut saved=match self.usage.take(){Some(Obj(fields))=>fields,_=>BTreeMap::new()};
            for key in ["promptTokenCount","candidatesTokenCount","totalTokenCount","cachedContentTokenCount","thoughtsTokenCount","toolUsePromptTokenCount"] {
                if let Some(value)=fields.get(key) {let Num(n)=value else{return Err("Invalid Content token count".into());};n.parse::<u64>().map_err(|_|"Invalid Content token count")?;saved.insert(key.into(),value.clone());}
            }
            self.usage=Some(Obj(saved));
        }
        let candidates=match record.get("candidates") {Ok(v)=>v.array()?,Err(_)=>return Ok(())};
        if candidates.len()>16 {return Err("Too many Content candidates".into());}
        let mut indices=std::collections::BTreeSet::new();
        for candidate in candidates {
            let index=match candidate.get("index") {Ok(Num(n))=>n.parse::<usize>().map_err(|_|"Invalid Content candidate index")?,Err(_)=>0,_=>return Err("Invalid Content candidate index".into())};
            if index>=16 || !indices.insert(index) {return Err("Invalid or duplicate Content candidate index".into());}
            if index!=0 {continue;}
            if self.finish_reason.is_some() {return Err("Content candidate followed its completion".into());}
            if let Ok(content)=candidate.get("content") {
                if let Ok(role)=content.get("role") {if role.text()?!="model" {return Err("Invalid Content response role".into());}}
                let parts=content.get("parts")?.array()?;
                self.content_parts=self.content_parts.checked_add(parts.len()).ok_or("Content part count overflow")?;
                if self.content_parts>4096 {return Err("Content reply exceeds 4096 parts".into());}
                for part in parts {if let Some(text)=content_part(part)? {self.append(text)?;}}
            }
            if let Ok(reason)=candidate.get("finishReason") {
                if reason!=&Null && !reason.text()?.is_empty() && reason.text()?!="FINISH_REASON_UNSPECIFIED" {
                    let reason=reason.text()?;
                    if !["STOP","MAX_TOKENS"].contains(&reason) {return Err(format!("Content response stopped without a supported text completion: {reason}"));}
                    self.finish_reason=Some(reason.to_string());
                }
            }
        }
        Ok(())
    }
    fn append(&mut self, content: &str) -> Result<()> {
        if self.text.len().saturating_add(content.len()) > TEXT_LIMIT { return Err("Streamed reply exceeds 1 MiB".into()); }
        self.text.push_str(content); Ok(())
    }
    fn event(&mut self) -> Result<()> {
        let data = std::mem::take(&mut self.event);
        let event_name = std::mem::take(&mut self.event_name);
        if data.is_empty() { return Ok(()); }
        let data = data.strip_suffix('\n').unwrap_or(&data);
        if self.mcp {
            if self.terminal.is_some(){return Ok(());}
            let record=parse(data)?;if rpc_result(&record)?{self.terminal=Some(record);}return Ok(());
        }
        if self.content {return self.content_event(&parse(data)?);}
        if self.terminal.is_some() { return Err("Unexpected data after the completed reply".into()); }
        if self.messages {return self.message_event(&parse(data)?, &event_name);}
        if data == "[DONE]" {
            let reason = self.finish_reason.as_ref().ok_or("Stream ended without a completed choice")?;
            let mut fields = vec![("choices",Arr(vec![obj(vec![("index",Num("0".into())),("message",obj(vec![("role",s("assistant")),("content",s(&self.text))])),("finish_reason",s(reason))])]))];
            if let Some(usage) = &self.usage {fields.push(("usage",usage.clone()));}
            self.terminal = Some(obj(fields)); return Ok(());
        }
        let record = parse(data)?;
        if let Ok(error) = record.get("error") {return Err(format!("Server error: {}",error.string()));}
        for choice in record.get("choices")?.array()? {
            if choice.get("index")? != &Num("0".into()) {continue;}
            if self.finish_reason.is_some() {return Err("Choice data followed its completion".into());}
            let delta = choice.get("delta")?;
            if let Ok(content) = delta.get("content") {if content != &Null {self.append(content.text()?)?;}}
            if let Ok(reason) = choice.get("finish_reason") {if reason != &Null {self.finish_reason=Some(reason.text()?.to_string());}}
        }
        if let Ok(usage) = record.get("usage") {if matches!(usage,Obj(_)) {self.usage=Some(usage.clone());}}
        Ok(())
    }
    fn event_line(&mut self) -> Result<()> {
        let bytes = std::mem::take(&mut self.line);
        let mut line = std::str::from_utf8(&bytes).map_err(|_| "Invalid UTF-8 in streamed reply")?;
        if !self.first_line {self.first_line=true;line=line.strip_prefix('\u{feff}').unwrap_or(line);}
        if line.is_empty() {return self.event();}
        let (name,value) = line.split_once(':').unwrap_or((line,""));
        if self.messages && name == "event" {
            if value.len()>256 {return Err("Stream event name exceeds limit".into());}
            self.event_name=value.strip_prefix(' ').unwrap_or(value).to_string();
        }
        if name == "data" {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if self.event.len().saturating_add(value.len()).saturating_add(1)>TEXT_LIMIT {return Err("Stream event exceeds 1 MiB".into());}
            self.event.push_str(value);self.event.push('\n');
        }
        Ok(())
    }
    fn record(&mut self, bytes: &[u8]) -> Result<()> {
        let line = std::str::from_utf8(bytes).map_err(|_| "Invalid UTF-8 in streamed reply")?.trim();
        if line.is_empty() { return Ok(()); }
        if self.terminal.is_some() { return Err("Unexpected data after the completed reply".into()); }
        let record = parse(line)?;
        if let Ok(error) = record.get("error") { return Err(format!("Server error: {}", error.string())); }
        if let Ok(message) = record.get("message") {
            if let Ok(content) = message.get("content") {
                let content = content.text()?;
                self.append(content)?;
            }
        }
        match record.get("done") {
            Ok(Bool(true)) => self.terminal = Some(record),
            Ok(Bool(false)) => {},
            _ => return Err("Streamed response has no valid completion flag".into()),
        }
        Ok(())
    }
    fn body(&mut self, bytes: &[u8]) -> Result<()> {
        if self.json { return Ok(()); }
        if self.chat {
            for byte in bytes {
                if self.carriage {self.carriage=false;if *byte==b'\n' {continue;}}
                if *byte==b'\r' || *byte==b'\n' {self.event_line()?;self.carriage=*byte==b'\r';}
                else {if self.line.len()>=TEXT_LIMIT {return Err("Stream line exceeds 1 MiB".into());}self.line.push(*byte);}
            }
            return Ok(());
        }
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if self.line.len().saturating_add(part.len()) > TEXT_LIMIT { return Err("Stream record exceeds 1 MiB".into()); }
            self.line.extend_from_slice(part);
            if part.last() == Some(&b'\n') { let line = std::mem::take(&mut self.line); self.record(&line)?; }
        }
        Ok(())
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        if self.raw.len().saturating_add(bytes.len()) > LIMIT { return Err("Stream response exceeds 2 MiB".into()); }
        self.raw.extend_from_slice(bytes);
        while !self.headers {
            let Some(relative_end) = self.raw[self.offset..].windows(4).position(|w| w == b"\r\n\r\n") else {
                if self.raw.len() > 65536 { return Err("HTTP headers exceed limit".into()); }
                return Ok(());
            };
            let end=self.offset+relative_end;
            if relative_end > 65536 { return Err("HTTP headers exceed limit".into()); }
            let header = std::str::from_utf8(&self.raw[self.offset..end]).map_err(|_| "Invalid response headers")?;
            let code = header.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|v| v.parse::<u16>().ok()).ok_or("Invalid HTTP status")?;
            // Non-success and informational responses are handled by the complete
            // decoder; they are never rendered as generated text.
            if (100..200).contains(&code) {self.interim+=1;if self.interim>4{return Err("Too many informational HTTP responses".into());}self.offset=end+4;continue;}
            if !(200..300).contains(&code) { return Ok(()); }
            if (self.mcp||self.messages||self.content)&&code!=200{return Err("Streaming result requires HTTP 200 with a response body".into());}
            let mut names=std::collections::BTreeSet::new();
            for line in header.split("\r\n").skip(1) {
                let (name, value) = line.split_once(':').ok_or("Invalid response header")?;
                if self.mcp||self.messages||self.content {
                    let name=name.to_ascii_lowercase();let value=value.trim();
                    if ["content-length","transfer-encoding","content-type","mcp-session-id"].contains(&name.as_str())&&!names.insert(name.clone()){return Err("Duplicate streaming framing header".into());}
                    if name=="transfer-encoding"&&!value.eq_ignore_ascii_case("chunked"){return Err("Unsupported streaming transfer encoding".into());}
                    if name=="content-encoding"&&!value.eq_ignore_ascii_case("identity"){return Err("Unsupported streaming content encoding".into());}
                    if name=="mcp-session-id"{if value.len()>256||value.chars().any(char::is_control){return Err("Invalid MCP session ID".into());}self.mcp_session=value.into();}
                }
                if name.eq_ignore_ascii_case("transfer-encoding") { self.chunked = value.trim().eq_ignore_ascii_case("chunked"); }
                if name.eq_ignore_ascii_case("content-length") { let length=value.trim().parse::<usize>().map_err(|_| "Invalid response length")?;self.expected=Some((end+4).checked_add(length).ok_or("Response length overflow")?); }
                if name.eq_ignore_ascii_case("content-type") { self.json = value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"); }
            }
            if (self.mcp||self.messages||self.content)&&self.chunked&&self.expected.is_some(){return Err("Ambiguous streaming response framing".into());}
            self.headers = true; self.offset = end + 4;
        }
        if (self.mcp||self.messages||self.content)&&self.expected.map(|end|self.raw.len()>end).unwrap_or(false){return Err("Streaming response exceeds Content-Length".into());}
        loop {
            if !self.chunked {
                let data = self.raw[self.offset..].to_vec(); self.offset = self.raw.len(); self.body(&data)?; break;
            }
            if self.trailers { break; }
            if self.terminator {
                if self.raw.len() - self.offset < 2 { break; }
                if &self.raw[self.offset..self.offset+2] != b"\r\n" { return Err("Invalid stream chunk terminator".into()); }
                self.offset += 2; self.terminator = false;
            }
            if self.remaining == 0 {
                let Some(size_end) = self.raw[self.offset..].windows(2).position(|w| w == b"\r\n") else { break; };
                if size_end > 8192 { return Err("Stream chunk header exceeds limit".into()); }
                let line = std::str::from_utf8(&self.raw[self.offset..self.offset+size_end]).map_err(|_| "Invalid stream chunk header")?;
                self.remaining = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(),16).map_err(|_| "Invalid stream chunk size")?;
                if self.remaining > LIMIT { return Err("Stream chunk exceeds response limit".into()); }
                self.offset += size_end + 2;
                if self.remaining == 0 { self.trailers = true; break; }
            }
            let count = self.remaining.min(self.raw.len() - self.offset);
            if count == 0 { break; }
            let data = self.raw[self.offset..self.offset+count].to_vec();
            self.offset += count; self.remaining -= count; self.body(&data)?;
            if self.remaining == 0 { self.terminator = true; }
        }
        Ok(())
    }
    fn framed(&self) -> bool {
        if !self.headers {return false;}
        if self.mcp&&self.terminal.is_some(){return true;}
        if self.chunked {self.trailers && (self.raw[self.offset..]==*b"\r\n" || self.raw[self.offset..].ends_with(b"\r\n\r\n"))}
        else {self.expected.map(|end|self.raw.len()>=end).unwrap_or(false)}
    }
    fn finish(&mut self) -> Result<String> {
        if self.mcp {
            if let Some(result)=self.terminal.take(){return Ok(result.string());}
            let (_,body)=network::decoded(&self.raw)?;let record=parse(&body)?;
            if !rpc_result(&record)?{return Err("MCP response did not match the request ID".into());}return Ok(record.string());
        }
        let (_, body) = network::decoded(&self.raw)?;
        if self.json || !self.headers || (self.chat && self.text.is_empty() && self.terminal.is_none() && body.trim_start().starts_with('{')) {
            // A compatible server may ignore stream=true and return one JSON reply.
            response(&obj(vec![("protocol",s(if self.content {"content"} else if self.messages {"messages"} else if self.chat {"chat"} else {"local"})),("body",s(&body))]))?;
            if parse(&body)?.get("done")==Ok(&Bool(false)){return Err("Connection ended before the model completed its reply".into());}
            return Ok(body);
        }
        // SSE discards an event that lacks a final blank line; NDJSON allows
        // a final complete record without a trailing newline.
        if !self.chat {let line = std::mem::take(&mut self.line); self.record(&line)?;}
        if self.content {
            let reason=self.finish_reason.as_ref().ok_or("Connection ended before the Content candidate completed")?;
            if self.text.trim().is_empty(){return Err("Server returned no text response".into());}
            let mut fields=vec![("candidates",Arr(vec![obj(vec![("index",Num("0".into())),("content",obj(vec![("role",s("model")),("parts",Arr(vec![obj(vec![("text",s(&self.text))])]))])),("finishReason",s(reason))])]))];
            if let Some(usage)=&self.usage {fields.push(("usageMetadata",usage.clone()));}
            return Ok(obj(fields).string());
        }
        let mut complete = self.terminal.take().ok_or("Connection ended before the model completed its reply")?;
        if self.text.trim().is_empty() { return Err("Server returned no text response".into()); }
        if !self.chat {if let Obj(fields) = &mut complete { fields.insert("message".into(),obj(vec![("role",s("assistant")),("content",s(&self.text))])); }}
        Ok(complete.string())
    }
}

fn content_part(part:&Json)->Result<Option<&str>> {
    let Obj(_) = part else{return Err("Invalid Content part".into());};
    for key in ["functionCall","functionResponse","executableCode","codeExecutionResult","inlineData","fileData"] {
        if part.get(key).is_ok(){return Err("Structured Content tools and media are not supported here; no tool was executed".into());}
    }
    let thought=match part.get("thought") {Ok(Bool(v))=>*v,Err(_)=>false,_=>return Err("Invalid Content thought marker".into())};
    if thought {return Ok(None);}
    match part.get("text") {Ok(t)=>Ok(Some(t.text()?)),Err(_)=>Ok(None)}
}
pub(super) fn content_text(body:&Json)->Result<String> {
    let mut transfer=Transfer{content:true,..Transfer::default()};
    transfer.content_event(body)?;
    // Preserve complete-JSON part separators from the existing text adapter.
    // Streamed replies are normalized to one aggregate part at completion.
    for candidate in body.get("candidates")?.array()? {
        if candidate.get("index").unwrap_or(&Num("0".into()))!=&Num("0".into()) {continue;}
        let mut parts=Vec::new();
        for part in candidate.get("content")?.get("parts")?.array()? {if let Some(text)=content_part(part)? {parts.push(text);}}
        return Ok(parts.join("\n"));
    }
    Err("No response candidate zero; content may have been blocked".into())
}

fn message_index(record:&Json)->Result<usize>{
    let Num(index)=record.get("index")? else {return Err("Invalid Messages block index".into());};
    index.parse::<usize>().map_err(|_|"Invalid Messages block index".into())
}
fn rpc_result(record:&Json)->Result<bool>{
    if record.get("jsonrpc")!=Ok(&s("2.0")){return Err("Invalid JSON-RPC version".into());}
    if record.get("id")!=Ok(&Num("1".into())){return Ok(false);}
    let result=record.get("result").is_ok();let error=record.get("error").is_ok();
    if result==error{return Err("MCP reply requires exactly one result or error".into());}Ok(true)
}
struct Sessions { next: u64, entries: BTreeMap<u64, Transfer> }

#[cfg(test)] mod mcp_checks {
 use super::*;
 #[test]fn persistent_sse_waits_for_matching_result_and_preserves_session(){
  let events="data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{}}\n\ndata: {\"jsonrpc\":\"2.0\",\n data: ignored\n";
  let good="data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1,\"result\":{\"tools\":[]}}\n\n";
  let prefix=events.split("data: {\"jsonrpc\":\"2.0\",\n").next().unwrap();
  for chunks in [false,true]{let header=if chunks{"Transfer-Encoding: chunked\r\n"}else{""};let head=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nMcp-Session-Id: session-123\r\n{header}\r\n");let mut t=Transfer{chat:true,mcp:true,..Transfer::default()};t.feed(head.as_bytes()).unwrap();
   let encode=|body:&str|if chunks{format!("{:x}\r\n{body}\r\n",body.len())}else{body.into()};
   t.feed(encode(prefix).as_bytes()).unwrap();assert!(!t.framed());
   for b in encode(good).as_bytes(){t.feed(&[*b]).unwrap();}
   assert!(t.framed());assert_eq!(t.mcp_session,"session-123");assert_eq!(parse(&t.finish().unwrap()).unwrap().get("id"),Ok(&Num("1".into())));
  }
 }
 #[test]fn wrong_id_and_ambiguous_framing_are_not_success(){
  for body in ["{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}","{\"jsonrpc\":\"2.0\",\"id\":1}","{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{},\"error\":{}}"]{let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len());let mut t=Transfer{chat:true,mcp:true,..Transfer::default()};t.feed(wire.as_bytes()).unwrap();assert!(t.finish().is_err());}
  for status in ["200 OK","204 No Content"]{let mut t=Transfer{chat:true,mcp:true,..Transfer::default()};let wire=format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\ndata: {{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}\n\n");assert!(t.feed(wire.as_bytes()).is_err());}
  let mut t=Transfer{chat:true,mcp:true,..Transfer::default()};assert!(t.feed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n").is_err());
 }
}
static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
pub fn operation(v: &Json) -> Result<Json> {
    let mut sessions = SESSIONS.get_or_init(||Mutex::new(Sessions{next:1,entries:BTreeMap::new()})).lock().map_err(|_| "Stream state unavailable")?;
    let action = field(v,"op")?;
    if action == "stream_open" {
        if sessions.entries.len() >= 4 { return Err("Too many active reply streams".into()); }
        let id = sessions.next; sessions.next = id.checked_add(1).ok_or("Stream identifier exhausted")?;
        let protocol=v.get("protocol").and_then(Json::text).unwrap_or("local");
        if !["local","chat","mcp","messages","content"].contains(&protocol) {return Err("Unsupported incremental API format".into());}
        sessions.entries.insert(id,Transfer{chat:protocol!="local",mcp:protocol=="mcp",messages:protocol=="messages",content:protocol=="content",..Transfer::default()}); return Ok(Num(id.to_string()));
    }
    let id = match v.get("id")? { Num(n)=>n.parse::<u64>().map_err(|_| "Invalid stream identifier")?, _=>return Err("Invalid stream identifier".into()) };
    if action == "stream_close" { sessions.entries.remove(&id); return Ok(Null); }
    let result = (|| {
        let transfer = sessions.entries.get_mut(&id).ok_or("Reply stream is no longer active")?;
        if action == "stream_finish" {
            let body = transfer.finish()?;
            let session=if transfer.mcp_session.is_empty(){String::new()}else{format!("Mcp-Session-Id: {}\r\n",transfer.mcp_session)};
            let wire = format!("HTTP/1.1 200 OK\r\n{session}Content-Length: {}\r\n\r\n{}",body.len(),body);
            return Ok(obj(vec![("data",s(&network::b64(wire.as_bytes())))]));
        }
        let previous = transfer.text.len();
        let bytes=network::unbase64(field(v,"data")?)?;
        if let Err(error)=transfer.feed(&bytes) {
            // Preserve text already decoded in this same read before an error.
            // The native consumer publishes it as interrupted, never completed.
            if transfer.messages||transfer.content {return Ok(obj(vec![("delta",s(&transfer.text[previous..])),("complete",Bool(false)),("error",s(&error))]));}
            return Err(error);
        }
        Ok(obj(vec![("delta",s(&transfer.text[previous..])),("complete",Bool(transfer.framed()))]))
    })();
    if result.is_err() || result.as_ref().map(|v|v.get("error").is_ok()).unwrap_or(false) || action == "stream_finish" { sessions.entries.remove(&id); }
    result
}

#[cfg(test)]
mod checks {
    use super::*;
    fn chat_transfer()->Transfer {Transfer{chat:true,..Transfer::default()}}
    fn event_body()->String {
        concat!("\u{feff}: keepalive\r\nevent: message\r\n",
          "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null},\"finish_reason\":null}]}\r\n\r\n",
          "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"café 中文\"},\"finish_reason\":null}]}\r\r",
          "data: {\"choices\":[\n",
          "data: {\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
          "data: {\"choices\":[],\"usage\":{\"completion_tokens\":4}}\n\n",
          "data: [DONE]\n\n").into()
    }
    #[test] fn sse_fragmentation_bom_line_endings_multiline_and_usage(){
        for chunked in [false,true] {let bytes=wire(&event_body(),chunked);for size in 1..=bytes.len(){let mut t=chat_transfer();for part in bytes.chunks(size){t.feed(part).unwrap();}assert_eq!(t.text,"café 中文");let body=t.finish().unwrap();assert_eq!(response(&obj(vec![("protocol",s("chat")),("body",s(&body))])).unwrap(),s("café 中文"));assert_eq!(parse(&body).unwrap().get("usage").unwrap().get("completion_tokens"),Ok(&Num("4".into())));}}
    }
    #[test] fn sse_completion_requires_terminated_done_and_complete_choice(){
        for body in [event_body().replace("data: [DONE]\n\n",""),event_body().trim_end().to_string(),"data: [DONE]\n\n".into()] {let mut t=chat_transfer();let result=t.feed(&wire(&body,false));assert!(result.is_err()||t.finish().is_err());}
        let mut t=chat_transfer();assert!(t.feed(&wire("data: {\"error\":{\"message\":\"denied\"}}\n\n",true)).is_err());
        let mut t=chat_transfer();assert!(t.feed(&wire(&(event_body()+"data: {}\n\n"),true)).is_err());
    }
    #[test] fn sse_ignores_other_choices_and_does_not_commit_tool_only_output(){
        let body="data: {\"choices\":[{\"index\":1,\"delta\":{\"content\":\"other\"},\"finish_reason\":\"stop\"},{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
        let mut t=chat_transfer();t.feed(&wire(body,false)).unwrap();assert!(t.text.is_empty());assert!(t.finish().is_err());
        let mut t=chat_transfer();let body="{\"choices\":[{\"message\":{\"content\":\"fallback\"}}]}";let bytes=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len());t.feed(bytes.as_bytes()).unwrap();assert_eq!(t.finish().unwrap(),body);
    }
    fn wire(body:&str,chunked:bool)->Vec<u8>{
        if !chunked {return format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",body.len()).into_bytes();}
        let mut out=b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for chunk in body.as_bytes().chunks(7){out.extend_from_slice(format!("{:x}\r\n",chunk.len()).as_bytes());out.extend_from_slice(chunk);out.extend_from_slice(b"\r\n");}out.extend_from_slice(b"0\r\n\r\n");out
    }
    #[test] fn session_cleanup_is_scoped_and_reusable(){
        fn opened()->Json{operation(&obj(vec![("op",s("stream_open"))])).unwrap()}
        let retained=opened();
        for _ in 0..8 {let id=opened();assert!(operation(&obj(vec![("op",s("stream_feed")),("id",id.clone()),("data",s("invalid!"))])).is_err());assert!(operation(&obj(vec![("op",s("stream_finish")),("id",id)])).is_err());}
        let bytes=wire("{\"message\":{\"content\":\"retained\"},\"done\":true}\n",false);
        operation(&obj(vec![("op",s("stream_feed")),("id",retained.clone()),("data",s(&network::b64(&bytes)))])).unwrap();
        assert!(operation(&obj(vec![("op",s("stream_finish")),("id",retained.clone())])).is_ok());
        assert!(operation(&obj(vec![("op",s("stream_close")),("id",retained)])).is_ok());
        for _ in 0..8 {let id=opened();operation(&obj(vec![("op",s("stream_close")),("id",id)])).unwrap();}
    }
    #[test] fn fragmented_unicode_records_and_http_chunks(){
        let body="{\"message\":{\"content\":\"café \"},\"done\":false}\n{\"message\":{\"content\":\"中文\"},\"done\":false}\n{\"message\":{\"content\":\"\"},\"done\":true,\"eval_count\":4}\n";
        for chunked in [false,true] {let bytes=wire(body,chunked);for size in 1..=bytes.len(){let mut t=Transfer::default();for part in bytes.chunks(size){t.feed(part).unwrap();}assert_eq!(t.text,"café 中文");let done=parse(&t.finish().unwrap()).unwrap();assert_eq!(field(done.get("message").unwrap(),"content"),Ok("café 中文"));assert_eq!(done.get("eval_count"),Ok(&Num("4".into())));}}
    }
    #[test] fn truncated_error_and_oversized_streams_are_not_completed(){
        let body="{\"message\":{\"content\":\"partial\"},\"done\":false}\n";
        let mut t=Transfer::default();t.feed(&wire(body,false)).unwrap();assert_eq!(t.text,"partial");assert!(t.finish().is_err());
        let mut t=Transfer::default();assert!(t.feed(&wire("{\"error\":\"model absent\"}\n",true)).is_err());
        let mut t=Transfer::default();t.feed(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n").unwrap();assert!(t.text.is_empty());assert!(t.finish().is_err());
        let mut t=Transfer::default();assert!(t.feed(&vec![b'x';LIMIT+1]).is_err());
        let mut t=Transfer::default();let mut bytes=wire("{\"message\":{\"content\":\"ok\"},\"done\":true}\n",true);bytes.pop();t.feed(&bytes).unwrap();assert!(t.finish().is_err());
    }
    #[test] fn informational_headers_and_complete_framing(){
        let body="{\"message\":{\"content\":\"ready\"},\"done\":true}\n";
        for chunked in [false,true]{let mut bytes=b"HTTP/1.1 100 Continue\r\n\r\n".to_vec();bytes.extend_from_slice(&wire(body,chunked));let mut t=Transfer::default();for (i,b) in bytes.iter().enumerate(){t.feed(&[*b]).unwrap();assert_eq!(t.framed(),i+1==bytes.len());}assert_eq!(t.text,"ready");assert!(t.finish().unwrap().contains("ready"));}
    }
    #[test] fn single_json_fallback_and_unterminated_final_record(){
        let body="{\"message\":{\"content\":\"complete\"},\"done\":true}";
        let mut t=Transfer::default();t.feed(&wire(body,false)).unwrap();assert!(t.text.is_empty());assert!(t.finish().unwrap().contains("complete"));
        let mut t=Transfer::default();let bytes=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len());t.feed(bytes.as_bytes()).unwrap();assert_eq!(t.finish().unwrap(),body);
    }
}

#[cfg(test)]
mod messages_checks {
    use super::*;
    fn event(value:Json)->String {format!("event: {}\ndata: {}\n\n",field(&value,"type").unwrap(),value.string())}
    fn fixture()->Vec<Json> {
        vec![
            parse(r#"{"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","model":"assigned-model","content":[],"usage":{"input_tokens":10,"output_tokens":1}}}"#).unwrap(),
            parse(r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"hidden"}}"#).unwrap(),
            parse(r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private reasoning"}}"#).unwrap(),
            parse(r#"{"type":"content_block_stop","index":0}"#).unwrap(),
            parse(r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":"café "}}"#).unwrap(),
            parse(r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"中文 🦀"}}"#).unwrap(),
            parse(r#"{"type":"content_block_stop","index":1}"#).unwrap(),
            parse(r#"{"type":"content_block_start","index":2,"content_block":{"type":"text","text":"second"}}"#).unwrap(),
            parse(r#"{"type":"content_block_stop","index":2}"#).unwrap(),
            parse(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#).unwrap(),
            parse(r#"{"type":"message_delta","delta":{},"usage":{"output_tokens":13}}"#).unwrap(),
            parse(r#"{"type":"message_stop"}"#).unwrap(),
        ]
    }
    fn wire(body:&str,chunked:bool)->Vec<u8> {
        if !chunked{return format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",body.len()).into_bytes();}
        let mut out=b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for chunk in body.as_bytes().chunks(17){out.extend_from_slice(format!("{:x}\r\n",chunk.len()).as_bytes());out.extend_from_slice(chunk);out.extend_from_slice(b"\r\n");}out.extend_from_slice(b"0\r\n\r\n");out
    }
    fn transfer()->Transfer {Transfer{chat:true,messages:true,..Transfer::default()}}
    #[test] fn messages_error_keeps_same_read_partial_and_poisoned_session() {
        let mut records=fixture();records.truncate(6);records.push(parse(r#"{"type":"error","error":{"message":"busy"}}"#).unwrap());
        let body=records.into_iter().map(event).collect::<String>();
        let id=operation(&obj(vec![("op",s("stream_open")),("protocol",s("messages"))])).unwrap();
        let result=operation(&obj(vec![("op",s("stream_feed")),("id",id.clone()),("data",s(&network::b64(&wire(&body,false))))])).unwrap();
        assert_eq!(field(&result,"delta"),Ok("café 中文 🦀"));assert!(result.get("error").is_ok());assert_eq!(result.get("complete"),Ok(&Bool(false)));
        assert!(operation(&obj(vec![("op",s("stream_finish")),("id",id)])).is_err());
    }
    #[test] fn messages_stream_fragmented_text_hidden_blocks_usage_and_terminal() {
        let body=fixture().into_iter().map(event).collect::<String>();
        for chunked in [false,true] {let bytes=wire(&body,chunked);for size in [1,2,3,7,64,1024,65536] {let mut t=transfer();for part in bytes.chunks(size){t.feed(part).unwrap();}assert!(t.framed());assert_eq!(t.text,"café 中文 🦀\nsecond");let result=t.finish().unwrap();assert!(!result.contains("hidden"));assert!(!result.contains("private reasoning"));let output=parse(&result).unwrap();assert_eq!(output.get("usage").unwrap().get("input_tokens"),Ok(&Num("10".into())));assert_eq!(output.get("usage").unwrap().get("output_tokens"),Ok(&Num("13".into())));assert_eq!(response(&obj(vec![("protocol",s("messages")),("body",s(&result))])).unwrap(),s("café 中文 🦀\nsecond"));}}
    }
    #[test] fn messages_stream_scoped_errors_missing_terminal_and_http_integrity() {
        let good=fixture();
        for bad in [r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"orphan"}}"#,r#"{"type":"message_stop"}"#,r#"{"type":"error","error":{"message":"unavailable"}}"#] {let mut t=transfer();assert!(t.feed(&wire(&event(parse(bad).unwrap()),false)).is_err());}
        for cut in 0..good.len() {let mut t=transfer();t.feed(&wire(&good[..cut].iter().cloned().map(event).collect::<String>(),false)).unwrap();assert!(t.finish().is_err());}
        let body=good.into_iter().map(event).collect::<String>();let mut bytes=wire(&body,true);bytes.pop();let mut t=transfer();t.feed(&bytes).unwrap();assert!(t.finish().is_err());
        let mut t=transfer();assert!(t.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
        let mut t=transfer();assert!(t.feed(&wire(&(body+"data: {}\n\n"),false)).is_err());
    }
    #[test] fn messages_stream_extensions_fallback_and_wrong_block_identity() {
        let initial=event(fixture()[0].clone());
        for extra in [r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":"wrong"}}"#,r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use"}}"#] {let mut t=transfer();assert!(t.feed(&wire(&(initial.clone()+&event(parse(extra).unwrap())),false)).is_err());}
        let body="\u{feff}: comment\r\nevent: ping\rdata: {\"type\":\"ping\"}\r\revent: future_notice\ndata: {\"type\":\"future_notice\"}\n\n".to_string()+&fixture().into_iter().map(event).collect::<String>();let mut t=transfer();t.feed(&wire(&body,false)).unwrap();assert!(t.finish().is_ok());
        let mut t=transfer();assert!(t.feed(&wire("event: message_stop\ndata: {\"type\":\"ping\"}\n\n",false)).is_err());
        let body=r#"{"content":[{"type":"text","text":"JSON compatibility"}]}"#;let bytes=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len());let mut t=transfer();t.feed(bytes.as_bytes()).unwrap();assert_eq!(t.finish().unwrap(),body);
    }
}

#[cfg(test)]
mod content_checks {
    use super::*;
    fn transfer()->Transfer {Transfer{chat:true,content:true,..Transfer::default()}}
    fn wire(events:&[&str],chunked:bool)->Vec<u8>{
        let body=events.iter().map(|v|format!("data: {v}\n\n")).collect::<String>();
        if !chunked {return format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",body.len()).into_bytes();}
        let mut result=b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for part in body.as_bytes().chunks(11){result.extend(format!("{:x}\r\n",part.len()).as_bytes());result.extend(part);result.extend(b"\r\n");}result.extend(b"0\r\n\r\n");result
    }
    const FIRST:&str=r#"{"candidates":[{"index":1,"content":{"parts":[{"text":"other"}]}},{"index":0,"content":{"role":"model","parts":[{"text":"hidden","thought":true},{"thoughtSignature":"private"},{"text":"café "}]}}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":1,"thoughtsTokenCount":90}}"#;
    const LAST:&str=r#"{"candidates":[{"index":0,"content":{"parts":[{"text":"中文 🦀"}]},"finishReason":"STOP"}],"usageMetadata":{"candidatesTokenCount":777,"totalTokenCount":874}}"#;
    #[test] fn content_sse_split_utf8_usage_and_hidden_parts(){
        for chunked in [false,true]{let bytes=wire(&[FIRST,LAST,r#"{"usageMetadata":{"candidatesTokenCount":778}}"#],chunked);for size in [1,2,3,7,37,bytes.len()]{let mut t=transfer();for part in bytes.chunks(size){t.feed(part).unwrap();}assert!(t.framed());assert_eq!(t.text,"café 中文 🦀");let body=t.finish().unwrap();assert_eq!(response(&obj(vec![("protocol",s("content")),("body",s(&body))])).unwrap(),s("café 中文 🦀"));let b=parse(&body).unwrap();assert_eq!(b.get("usageMetadata").unwrap().get("candidatesTokenCount"),Ok(&Num("778".into())));assert!(!body.contains("hidden")&&!body.contains("private")&&!body.contains("other"));}}
    }
    #[test] fn content_errors_are_scoped_and_no_false_completion(){
        for last in [r#"{"error":{"message":"busy"}}"#,r#"{"candidates":[{"index":0,"finishReason":"SAFETY"}]}"#,r#"{"candidates":[{"index":0,"finishReason":"CONTINUATION"}]}"#,r#"{"candidates":[{"index":0,"content":{"parts":[{"functionCall":{"name":"ignored"}}]}}]}"#,r#"{"candidates":[{"index":0},{"index":0}]}"#]{let mut t=transfer();assert!(t.feed(&wire(&[FIRST,last],false)).is_err());assert_eq!(t.text,"café ");}
        let mut t=transfer();t.feed(&wire(&[FIRST],false)).unwrap();assert!(t.finish().is_err());
        let mut t=transfer();let mut w=wire(&[FIRST,LAST],true);w.pop();t.feed(&w).unwrap();assert!(t.finish().is_err());
        let mut t=transfer();assert!(t.feed(&wire(&[FIRST,LAST,FIRST],false)).is_err());
        let id=operation(&obj(vec![("op",s("stream_open")),("protocol",s("content"))])).unwrap();
        let result=operation(&obj(vec![("op",s("stream_feed")),("id",id.clone()),("data",s(&network::b64(&wire(&[FIRST,r#"{"error":{"message":"failed"}}"#],false))))])).unwrap();assert_eq!(field(&result,"delta"),Ok("café "));assert!(result.get("error").is_ok());assert!(operation(&obj(vec![("op",s("stream_finish")),("id",id)])).is_err());
    }
    #[test] fn content_json_fallback_and_bounds(){
        let body=r#"{"candidates":[{"content":{"parts":[{"thought":true,"text":"hidden"},{"text":"visible"}]},"finishReason":"STOP"}]}"#;
        let mut t=transfer();t.feed(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len()).as_bytes()).unwrap();assert_eq!(content_text(&parse(&t.finish().unwrap()).unwrap()).unwrap(),"visible");
        assert!(content_text(&parse(r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#).unwrap()).is_err());
        assert!(content_text(&parse(r#"{"candidates":[{"content":{"parts":[{"thought":"true","text":"hidden"}]}}]}"#).unwrap()).is_err());
        let mut t=transfer();let event=obj(vec![("candidates",Arr(vec![obj(vec![("index",Num("0".into())),("content",obj(vec![("parts",Arr(vec![obj(vec![("text",s("x"))]);4097]))]))])]))]);assert!(t.content_event(&event).is_err());
        let mut t=transfer();assert!(t.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
    }
}
