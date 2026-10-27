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
    server_stream: bool,
    resumable: bool,
    cursor: String,
    event_id: Option<String>,
    pending_cursor: Option<String>,
    resume_until: Option<std::time::Instant>,
    resumes: usize,
    server_notifications: usize,
    server_messages: usize,
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
    poisoned: bool,
    observed_eof: bool,
    control: Option<Json>,
    controls: Vec<Json>,
    control_pending: BTreeMap<u64,Vec<String>>,
    control_count: usize,
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
    fn request_id(&self)->Result<Json>{super::mcp_identity::captured(self.control.as_ref().ok_or("MCP request binding unavailable")?)?.ok_or("MCP GET has no client request ID".into())}
    fn server_observation(&self)->Json{if !self.server_stream{return Null;}obj(vec![("notifications",Num(self.server_notifications.to_string())),("requests",Num(self.control_count.to_string())),("messages",Num(self.server_messages.to_string()))])}
    fn lifecycle_session(&self)->Result<&str>{
        let context=self.control.as_ref().ok_or("No MCP lifecycle binding")?;
        let configured=field(context,"session")?;
        if !configured.is_empty(){return Ok(configured);}
        if self.headers&&context.get("request").and_then(Json::text)==Ok("initialize"){return Ok(&self.mcp_session);}
        Ok("")
    }
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
    fn control_reply(record:&Json,version:&str)->Result<Option<(String,Json)>> {
        if record.get("jsonrpc")!=Ok(&s("2.0"))||record.get("result").is_ok()||record.get("error").is_ok(){return Err("Invalid MCP server request envelope".into());}
        let method=record.get("method")?.text()?;
        if method.is_empty()||method.len()>128||method.chars().any(char::is_control){return Err("Invalid MCP server method".into());}
        let Ok(id)=record.get("id") else{super::mcp_schema::notification(version,record)?;return Ok(None);};
        match id {Str(value) if value.len()<=256=>{},Num(value) if value.len()<=64=>{},_=>return Err("Invalid or oversized MCP server request ID".into())};
        if record.string().len()>4096{return Err("MCP control request exceeds 4 KiB".into());}
        let params_valid=record.get("params").map(|v|matches!(v,Obj(_))).unwrap_or(true);
        let mut reply=obj(vec![("jsonrpc",s("2.0")),("id",id.clone())]);
        if method=="ping"&&params_valid {if let Obj(fields)=&mut reply{fields.insert("result".into(),obj(vec![]));}}
        else{let(code,message)=if method=="ping"{(-32602,"Invalid params")}else{(-32601,"Method not found")};if let Obj(fields)=&mut reply{fields.insert("error".into(),obj(vec![("code",Num(code.to_string())),("message",s(message))]));}}
        Ok(Some((control_key(id),reply)))
    }
    fn control_effect(&self,reply:&Json)->Result<Json>{
        let context=self.control.as_ref().ok_or("MCP control transport context is unavailable")?;
        let endpoint=field(context,"endpoint")?;let version=field(context,"version")?;
        let configured=context.get("session").and_then(Json::text).unwrap_or("");
        if !configured.is_empty()&&!self.mcp_session.is_empty()&&configured!=self.mcp_session{return Err("MCP response changed its session".into());}
        if configured.is_empty()&&!self.mcp_session.is_empty()&&context.get("request").is_ok()&&context.get("request").and_then(Json::text)!=Ok("initialize"){return Err("MCP session was assigned outside initialization".into());}
        let session=if self.mcp_session.is_empty(){configured}else{&self.mcp_session};
        let mut headers=vec![("Content-Type","application/json".into()),("Accept","application/json, text/event-stream".into()),("MCP-Protocol-Version",version.into())];
        let key=context.get("key").and_then(Json::text).unwrap_or("");if !key.is_empty(){headers.push(("Authorization",format!("Bearer {key}")));}
        if !session.is_empty(){headers.push(("Mcp-Session-Id",session.into()));}
        let mut effect=network::wire(endpoint,endpoint.starts_with("http://"),"POST",&reply.string(),headers)?;
        let ticket=control_ticket()?;
        if let Obj(fields)=&mut effect{fields.insert("ticket".into(),Num(ticket.to_string()));fields.insert("owner".into(),context.get("owner")?.clone());}
        Ok(effect)
    }
    fn pending_count(&self)->usize{self.control_pending.values().map(Vec::len).sum()}
    fn control_request(&mut self,record:&Json)->Result<()> {
        let Some((key,reply))=Self::control_reply(record,field(self.control.as_ref().ok_or("MCP notification binding unavailable")?,"version")?)? else{return Ok(());};
        if self.control_count>=32||self.pending_count()>=8{return Err("MCP control request limit exceeded".into());}
        if self.control_pending.values().any(|ids|ids.contains(&key)){return Err("Duplicate pending MCP server request ID".into());}
        let effect=self.control_effect(&reply)?;
        let Num(ticket)=effect.get("ticket")? else{return Err("Invalid MCP control ticket".into());};let ticket=ticket.parse::<u64>().map_err(|_|"Invalid MCP control ticket")?;
        self.control_count+=1;self.control_pending.insert(ticket,vec![key]);self.controls.push(effect);Ok(())
    }
    fn batch(&mut self,records:Vec<Json>)->Result<()> {
        let context=self.control.as_ref().ok_or("MCP batch transport binding unavailable")?;
        if field(context,"version")?!="2025-03-26"||!["tools/list","tools/call","GET"].contains(&field(context,"request")?){return Err("MCP SSE batches require an already-negotiated March tools or GET stream".into());}
        if records.is_empty()||records.len()>32{return Err("MCP batch requires one to 32 messages".into());}
        let requests=records[0].get("method").is_ok();
        if records.iter().any(|record|!matches!(record,Obj(_))||record.get("method").is_ok()!=requests){return Err("MCP batch directions must be homogeneous objects".into());}
        if requests {
            if self.server_stream&&self.server_messages+records.len()>256{return Err("MCP server stream exceeds 256 messages".into());}
            let mut ids=Vec::new();let mut replies=Vec::new();let mut notifications=0;
            // Stage only bounded IDs and canonical replies. No raw/text buffers are cloned.
            for record in &records {
                if let Ok(id)=record.get("id"){batch_id(id)?;}else if record.get("params").map(|v|!matches!(v,Obj(_))).unwrap_or(false){return Err("Invalid MCP batch notification parameters".into());}
                if let Some((key,reply))=Self::control_reply(record,field(self.control.as_ref().ok_or("MCP notification binding unavailable")?,"version")?)? {
                    if self.terminal.is_some(){return Err("MCP request followed the completed response".into());}
                    if ids.contains(&key)||self.control_pending.values().any(|pending|pending.contains(&key)){return Err("Duplicate pending MCP server request ID".into());}
                    ids.push(key);replies.push(reply);
                }else{notifications+=1;}
            }
            if self.control_count+ids.len()>32||self.pending_count()+ids.len()>8{return Err("MCP control request limit exceeded".into());}
            let effect=if replies.is_empty(){None}else{
                let reply=Arr(replies);if reply.string().len()>16384{return Err("MCP aggregate reply body exceeds 16 KiB".into());}
                let effect=self.control_effect(&reply)?;
                if field(&effect,"wire")?.len()>24576||effect.string().len()>32768{return Err("MCP aggregate reply exceeds wire or serialized transport limit".into());}
                Some(effect)
            };
            // The entire batch and wire have validated before any observable state commits.
            if let Some(effect)=effect {
                let Num(ticket)=effect.get("ticket")? else{return Err("Invalid MCP batch ticket".into());};let ticket=ticket.parse::<u64>().map_err(|_|"Invalid MCP batch ticket")?;
                self.control_count+=ids.len();self.control_pending.insert(ticket,ids);self.controls.push(effect);
            }
            if self.server_stream{self.server_messages+=records.len();self.server_notifications+=notifications;}
        }else{
            if self.server_stream{return Err("MCP GET-origin streams cannot receive response batches".into());}
            if let Some(terminal)=batch_result_for(records,&self.request_id()?)?{if self.terminal.is_some(){return Err("Duplicate correlated MCP terminal response".into());}self.terminal=Some(terminal);}
        }
        self.cursor_event();Ok(())
    }
    fn append(&mut self, content: &str) -> Result<()> {
        if self.text.len().saturating_add(content.len()) > TEXT_LIMIT { return Err("Streamed reply exceeds 1 MiB".into()); }
        self.text.push_str(content); Ok(())
    }
    fn event(&mut self) -> Result<()> {
        let data = std::mem::take(&mut self.event);
        let event_name = std::mem::take(&mut self.event_name);
        if data.is_empty() { self.cursor_event(); return Ok(()); }
        let data = data.strip_suffix('\n').unwrap_or(&data);
        if self.mcp {
            if self.server_stream&&data.len()>4096{return Err("MCP server message exceeds 4 KiB".into());}
            let record=parse(data)?;
            if let Arr(records)=record{return self.batch(records);}
            if self.server_stream {
                if self.server_messages>=256{return Err("MCP server stream exceeds 256 messages".into());}
                if record.get("method").is_err(){return Err("Unsolicited MCP responses are not allowed on a non-resumed server stream".into());}
                if record.get("id").is_err()&&record.get("params").map(|v|!matches!(v,Obj(_))).unwrap_or(false){return Err("Invalid MCP server notification parameters".into());}
                self.control_request(&record)?;
                if record.get("id").is_err(){self.server_notifications+=1;}
                self.server_messages+=1;self.cursor_event();return Ok(());
            }
            if record.get("method").is_ok(){
                if self.terminal.is_some()&&record.get("id").is_ok(){return Err("MCP request followed the completed response".into());}
                self.control_request(&record)?;
            }else if self.terminal.is_none() && rpc_result_for(&record,&self.request_id()?)?{self.terminal=Some(record);}
            return Ok(());
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
    fn cursor_event(&mut self){
        if let Some(id)=self.event_id.take(){self.pending_cursor=Some(id);}
        self.cursor_commit();
    }
    fn cursor_commit(&mut self){if self.control_pending.is_empty(){if let Some(id)=self.pending_cursor.take(){self.cursor=id;}}}
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
        if self.resumable && name=="id" {
            let value=value.strip_prefix(' ').unwrap_or(value);
            if value.len()>256||value.trim()!=value||!value.bytes().all(|b|(0x20..=0x7e).contains(&b)){return Err("MCP resume cursor requires at most 256 printable ASCII bytes without edge whitespace".into());}
            self.event_id=Some(value.into());
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
            if self.mcp {let mut parts=header.lines().next().unwrap_or("").split_whitespace();let version=parts.next().unwrap_or("");let status=parts.next().unwrap_or("");if !["HTTP/1.0","HTTP/1.1"].contains(&version)||status.len()!=3||!status.bytes().all(|b|b.is_ascii_digit()){return Err("Invalid MCP HTTP response status line".into());}}
            let code = header.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|v| v.parse::<u16>().ok()).ok_or("Invalid HTTP status")?;
            // Non-success and informational responses are handled by the complete
            // decoder; they are never rendered as generated text.
            if (100..200).contains(&code) {self.interim+=1;if self.interim>4{return Err("Too many informational HTTP responses".into());}self.offset=end+4;continue;}
            if self.server_stream&&code==405{return Err("This MCP endpoint does not offer an optional server GET stream (HTTP 405); no retry was sent".into());}
            if !(200..300).contains(&code) { return Ok(()); }
            if (self.mcp||self.messages||self.content)&&code!=200{return Err("Streaming result requires HTTP 200 with a response body".into());}
            let mut names=std::collections::BTreeSet::new();
            let mut server_sse=false;
            for line in header.split("\r\n").skip(1) {
                let (name, value) = line.split_once(':').ok_or("Invalid response header")?;
                if self.mcp||self.messages||self.content {
                    let name=name.to_ascii_lowercase();let value=value.trim();
                    if ["content-length","transfer-encoding","content-type","mcp-session-id"].contains(&name.as_str())&&!names.insert(name.clone()){return Err("Duplicate streaming framing header".into());}
                    if name=="transfer-encoding"&&!value.eq_ignore_ascii_case("chunked"){return Err("Unsupported streaming transfer encoding".into());}
                    if name=="content-encoding"&&!value.eq_ignore_ascii_case("identity"){return Err("Unsupported streaming content encoding".into());}
                    if name=="content-type"&&value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/event-stream"){server_sse=true;}
                    if name=="mcp-session-id"{if value.len()>256||value.chars().any(char::is_control)||(self.mcp&&(value.is_empty()||!value.bytes().all(|b|(0x21..=0x7e).contains(&b)))){return Err("Invalid MCP session ID".into());}self.mcp_session=value.into();}
                }
                if name.eq_ignore_ascii_case("transfer-encoding") { self.chunked = value.trim().eq_ignore_ascii_case("chunked"); }
                if name.eq_ignore_ascii_case("content-length") { let length=value.trim().parse::<usize>().map_err(|_| "Invalid response length")?;self.expected=Some((end+4).checked_add(length).ok_or("Response length overflow")?); }
                if name.eq_ignore_ascii_case("content-type") { self.json = value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"); }
            }
            if (self.mcp||self.messages||self.content)&&self.chunked&&self.expected.is_some(){return Err("Ambiguous streaming response framing".into());}
            if self.server_stream {
                if !server_sse{return Err("MCP server GET requires text/event-stream".into());}
                let configured=self.control.as_ref().ok_or("No MCP server stream binding")?.get("session")?.text()?;
                if !self.mcp_session.is_empty()&&self.mcp_session!=configured{return Err("MCP server GET changed its session".into());}
            }
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
        if self.server_stream&&!self.control_pending.is_empty(){return false;}
        if self.mcp&&self.terminal.is_some(){return self.control_pending.is_empty();}
        if self.chunked {self.trailers && (self.raw[self.offset..]==*b"\r\n" || self.raw[self.offset..].ends_with(b"\r\n\r\n"))}
        else {self.expected.map(|end|self.raw.len()>=end).unwrap_or(false)}
    }
    fn json_terminal(&self)->Result<Json>{
        let (_,body)=network::decoded(&self.raw)?;let mut record=parse(&body)?;
        if let Arr(records)=record {
            let context=self.control.as_ref().ok_or("MCP JSON batch binding unavailable")?;
            if field(context,"version")?!="2025-03-26"||!["tools/list","tools/call"].contains(&field(context,"request")?){return Err("MCP JSON response batches require already-negotiated March tools".into());}
            if !self.framed()&&!self.observed_eof{return Err("Close-delimited MCP JSON batch requires observed EOF".into());}
            json_batch_media(&self.raw,body.len())?;
            record=batch_result_for(records,&self.request_id()?)?.ok_or("MCP JSON batch has no matching response ID")?;
        }
        if !rpc_result_for(&record,&self.request_id()?)?{return Err("MCP response did not match the request ID".into());}Ok(record)
    }
    fn finish(&mut self) -> Result<String> {
        if self.mcp {
            if self.server_stream {
                if !self.control_pending.is_empty(){return Err("MCP server control response is not acknowledged".into());}
                let(status,_)=network::decoded(&self.raw)?;
                if status!=200||!self.headers||self.json{return Err("MCP server GET requires a complete HTTP 200 SSE response".into());}
                return Ok(obj(vec![("stream_schema",Num("1".into())),("notifications",Num(self.server_notifications.to_string())),("requests",Num(self.control_count.to_string())),("messages",Num(self.server_messages.to_string()))]).string());
            }
            if !self.control_pending.is_empty(){return Err("MCP control response is not acknowledged".into());}
            if let Some(result)=self.terminal.take(){return Ok(result.string());}
            return Ok(self.json_terminal()?.string());
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
// Exact decimal comparison keys do not expand powers or use floating point.
// Accepted numeric ID spellings remain echoed verbatim on the response wire.
fn decimal_exponent(value:&str,offset:i32)->Option<String>{
    let (negative,digits)=if let Some(v)=value.strip_prefix('-'){(true,v)}else{(false,value.strip_prefix('+').unwrap_or(value))};
    if digits.is_empty()||!digits.bytes().all(|b|b.is_ascii_digit()){return None;}
    let digits=digits.trim_start_matches('0');let digits=if digits.is_empty(){"0"}else{digits};
    if digits.len()<=3{let n=digits.parse::<i32>().ok()?;return Some((if negative{-n}else{n}+offset).to_string());}
    // With at least four digits and |offset|<=64 the magnitude cannot change sign.
    let mut out=digits.as_bytes().to_vec();let delta=if negative{-offset}else{offset};
    if delta>=0{let mut carry=delta as u32;for b in out.iter_mut().rev(){let n=(*b-b'0')as u32+carry;*b=b'0'+(n%10)as u8;carry=n/10;if carry==0{break;}}while carry!=0{out.insert(0,b'0'+(carry%10)as u8);carry/=10;}}
    else{let mut borrow=(-delta)as i32;for b in out.iter_mut().rev(){let mut n=(*b-b'0')as i32-borrow%10;borrow/=10;if n<0{n+=10;borrow+=1;}*b=b'0'+n as u8;if borrow==0{break;}}if borrow!=0{return None;}}
    let magnitude=std::str::from_utf8(&out).ok()?.trim_start_matches('0');let magnitude=if magnitude.is_empty(){"0"}else{magnitude};
    Some(format!("{}{}",if negative&&magnitude!="0"{"-"}else{""},magnitude))
}
fn numeric_key(value:&str)->Option<String>{
    let (mantissa,exponent)=if let Some(at)=value.find(['e','E']){(&value[..at],&value[at+1..])}else{(value,"0")};
    let negative=mantissa.starts_with('-');let mantissa=mantissa.strip_prefix('-').unwrap_or(mantissa);
    let fraction=mantissa.split_once('.').map(|(_,f)|f.len()).unwrap_or(0);if mantissa.matches('.').count()>1{return None;}
    let digits=mantissa.bytes().filter(|b|*b!=b'.').collect::<Vec<_>>();if digits.is_empty()||!digits.iter().all(|b|b.is_ascii_digit()){return None;}
    decimal_exponent(exponent,0)?;
    let digits=std::str::from_utf8(&digits).ok()?.trim_start_matches('0');if digits.is_empty(){return Some("n:0".into());}
    let coefficient=digits.trim_end_matches('0');let trailing=digits.len()-coefficient.len();
    let exponent=decimal_exponent(exponent,trailing as i32-fraction as i32)?;
    Some(format!("n:{}{}e{}",if negative{"-"}else{""},coefficient,exponent))
}
fn control_key(id:&Json)->String{if let Num(value)=id{numeric_key(value).unwrap_or_else(||id.string())}else{id.string()}}

#[cfg(test)]
fn integer_id(value:&str)->bool{let digits=value.strip_prefix('-').unwrap_or(value);(digits=="0"||digits.as_bytes().first().map(|b|(b'1'..=b'9').contains(b)).unwrap_or(false))&&digits.bytes().all(|b|b.is_ascii_digit())}
fn batch_id(id:&Json)->Result<String>{
    match id{Str(value) if value.len()<=256=>Ok(id.string()),Num(value) if value.len()<=64&&super::mcp_schema::integer_number(value)=>Ok(control_key(id)),_=>Err("MCP batch IDs require bounded strings or integer-valued numbers".into())}
}
fn batch_response(record:&Json)->Result<String>{
    super::mcp_schema::response(record)?;
    batch_id(record.get("id")?)
}
fn control_ticket()->Result<u64>{
    static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
    NEXT.try_update(std::sync::atomic::Ordering::Relaxed,std::sync::atomic::Ordering::Relaxed,|n|n.checked_add(1)).map_err(|_|"MCP control ticket exhausted".into())
}
fn rpc_result_for(record:&Json,expected:&Json)->Result<bool>{
    if record.get("jsonrpc")!=Ok(&s("2.0")){return Err("Invalid JSON-RPC version".into());}
    if !record.get("id").map(|id|super::mcp_identity::matches(id,expected)).unwrap_or(false){return Ok(false);}
    super::mcp_schema::response(record)?;Ok(true)
}
pub(super) fn batch_result_for(records:Vec<Json>,expected:&Json)->Result<Option<Json>>{
    if records.is_empty()||records.len()>32{return Err("MCP response batch requires one to 32 members".into());}
    let mut terminal=None;let mut ids=Vec::new();
    for record in records {
        if !matches!(record,Obj(_)){return Err("MCP response batch members must be objects".into());}
        let key=batch_response(&record)?;if ids.contains(&key){return Err("Duplicate MCP batch response ID".into());}ids.push(key);
        if record.get("id").map(|id|super::mcp_identity::matches(id,expected)).unwrap_or(false){if terminal.is_some(){return Err("Duplicate correlated MCP terminal response".into());}terminal=Some(record);}
    }Ok(terminal)
}
#[cfg(test)]
fn rpc_result(record:&Json)->Result<bool>{rpc_result_for(record,&Num("1".into()))}
#[cfg(test)]
fn batch_result(records:Vec<Json>)->Result<Option<Json>>{batch_result_for(records,&Num("1".into()))}
pub(super) fn json_batch_media(mut raw:&[u8],body_len:usize)->Result<()>{
    if raw.len()>LIMIT||body_len>TEXT_LIMIT{return Err("MCP JSON batch exceeds bounded response bytes".into());}
    for _ in 0..=4 {
        let end=raw.windows(4).position(|v|v==b"\r\n\r\n").ok_or("Incomplete MCP JSON batch headers")?;if end>65536{return Err("MCP JSON batch headers exceed limit".into());}
        let header=std::str::from_utf8(&raw[..end]).map_err(|_|"Invalid MCP JSON batch header text")?;let mut lines=header.split("\r\n");let mut status=lines.next().unwrap_or("").split_whitespace();let version=status.next().unwrap_or("");let code=status.next().unwrap_or("");
        if !["HTTP/1.0","HTTP/1.1"].contains(&version)||code.len()!=3||!code.bytes().all(|b|b.is_ascii_digit()){return Err("Invalid MCP JSON batch status".into());}
        let code=code.parse::<u16>().map_err(|_|"Invalid MCP JSON batch status")?;if (100..200).contains(&code){raw=&raw[end+4..];continue;}if code!=200{return Err("MCP JSON batch requires HTTP 200".into());}
        let mut media=None;for line in lines{let(key,value)=line.split_once(':').ok_or("Invalid MCP JSON batch header")?;if key.eq_ignore_ascii_case("content-type"){if media.is_some(){return Err("Duplicate MCP JSON batch media type".into());}media=Some(value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"));}}
        return if media==Some(true){Ok(())}else{Err("MCP JSON response batch requires application/json".into())};
    }Err("Too many informational MCP JSON batch responses".into())
}
fn control_framed(bytes:&[u8])->bool {
    let Some(end)=bytes.windows(4).position(|v|v==b"\r\n\r\n") else{return false;};
    let Ok(header)=std::str::from_utf8(&bytes[..end]) else{return false;};
    header.lines().skip(1).any(|line|line.split_once(':').map(|(key,_)|key.eq_ignore_ascii_case("content-length")||key.eq_ignore_ascii_case("transfer-encoding")).unwrap_or(false))
}
struct Lease { token:u64, context:Json, cursor:String, until:std::time::Instant, resumes:usize, reserved:Option<String> }
struct Sessions { next: u64, entries: BTreeMap<u64, Transfer>, openings:BTreeMap<u64,Json>, lease:Option<Lease> }
fn summary_wire(t:&Transfer,token:Option<u64>,paused:bool)->Json{
 let mut summary=obj(vec![("stream_schema",Num("1".into())),("notifications",Num(t.server_notifications.to_string())),("requests",Num(t.control_count.to_string())),("messages",Num(t.server_messages.to_string())),("paused",Bool(paused))]);
 if let Some(token)=token{if let Obj(f)=&mut summary{f.insert("resume_token".into(),Num(token.to_string()));}}
 let body=summary.string();let wire=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",body.len(),body);obj(vec![("data",s(&network::b64(wire.as_bytes())))])
}
fn suspend(sessions:&mut Sessions,id:u64,owner:&Json,paused:bool)->Result<Json>{
 let t=sessions.entries.get(&id).ok_or("MCP stream is no longer active")?;
 if !t.resumable||t.poisoned||!t.headers||!t.server_stream||t.control.as_ref().and_then(|v|v.get("owner").ok())!=Some(owner)||!t.control_pending.is_empty(){return Err("MCP stream cannot be paused with unacknowledged or invalid messages".into());}
 let until=t.resume_until.ok_or("MCP resume lifetime unavailable")?;
 if t.cursor.is_empty()||std::time::Instant::now()>=until||t.resumes>=4{return Err("No usable MCP cursor or resume lifetime remains; close and listen anew".into());}
 if sessions.lease.is_some(){return Err("A paused MCP stream already owns the resume slot".into());}
 let token=sessions.next;sessions.next=token.checked_add(1).ok_or("MCP resume ticket exhausted")?;
 let t=sessions.entries.remove(&id).unwrap();sessions.lease=Some(Lease{token,context:t.control.clone().unwrap(),cursor:t.cursor.clone(),until,resumes:t.resumes,reserved:None});Ok(summary_wire(&t,Some(token),paused))
}

#[cfg(test)] mod mcp_checks {
 use super::*;
 #[test]fn persistent_sse_waits_for_matching_result_and_preserves_session(){
  let events="data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/custom\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{}}\n\ndata: {\"jsonrpc\":\"2.0\",\n data: ignored\n";
  let good="data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1,\"result\":{\"tools\":[]}}\n\n";
  let prefix=events.split("data: {\"jsonrpc\":\"2.0\",\n").next().unwrap();
  for chunks in [false,true]{let header=if chunks{"Transfer-Encoding: chunked\r\n"}else{""};let head=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nMcp-Session-Id: session-123\r\n{header}\r\n");let mut t=Transfer{chat:true,mcp:true,control:Some(obj(vec![("request_binding",Num("1".into())),("version",s("2025-06-18"))])),..Transfer::default()};t.feed(head.as_bytes()).unwrap();
   let encode=|body:&str|if chunks{format!("{:x}\r\n{body}\r\n",body.len())}else{body.into()};
   t.feed(encode(prefix).as_bytes()).unwrap();assert!(!t.framed());
   for b in encode(good).as_bytes(){t.feed(&[*b]).unwrap();}
   assert!(t.framed());assert_eq!(t.mcp_session,"session-123");assert_eq!(parse(&t.finish().unwrap()).unwrap().get("id"),Ok(&Num("1".into())));
  }
 }
 #[test]fn wrong_id_and_ambiguous_framing_are_not_success(){
  for body in ["{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}","{\"jsonrpc\":\"2.0\",\"id\":1}","{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{},\"error\":{}}"]{let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len());let mut t=Transfer{chat:true,mcp:true,control:Some(obj(vec![("request_binding",Num("1".into())),("version",s("2025-06-18"))])),..Transfer::default()};t.feed(wire.as_bytes()).unwrap();assert!(t.finish().is_err());}
  for status in ["200 OK","204 No Content"]{let mut t=Transfer{chat:true,mcp:true,control:Some(obj(vec![("request_binding",Num("1".into())),("version",s("2025-06-18"))])),..Transfer::default()};let wire=format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\ndata: {{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}\n\n");assert!(t.feed(wire.as_bytes()).is_err());}
  let mut t=Transfer{chat:true,mcp:true,control:Some(obj(vec![("request_binding",Num("1".into())),("version",s("2025-06-18"))])),..Transfer::default()};assert!(t.feed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n").is_err());
 }
}
static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
pub fn operation(v: &Json) -> Result<Json> {
    let mut sessions = SESSIONS.get_or_init(||Mutex::new(Sessions{next:1,entries:BTreeMap::new(),openings:BTreeMap::new(),lease:None})).lock().map_err(|_| "Stream state unavailable")?;
    let action = field(v,"op")?;
    if action=="stream_prepare" {
        let context=v.get("context")?;let owner=field(context,"owner")?;let endpoint=field(context,"endpoint")?;let key=field(context,"key")?;let session=field(context,"session")?;
        network::url(endpoint,endpoint.starts_with("http://"))?;
        if endpoint.len()>8192||key.len()>8192||owner.is_empty()||owner.len()>256||session.len()>256||!session.bytes().all(|b|(0x21..=0x7e).contains(&b))||!["2025-03-26","2025-06-18"].contains(&field(context,"version")?)||field(context,"request")?!="GET"||context.get("resumable")!=Ok(&Bool(true)){return Err("Invalid resumable MCP GET admission".into());}
        super::mcp_identity::captured(context)?;
        if sessions.openings.len()>=4||sessions.entries.len()+sessions.openings.len()>=4{return Err("MCP GET opening capacity is full".into());}
        if sessions.openings.values().any(|c|c.get("owner")==context.get("owner"))||sessions.entries.values().any(|t|t.control.as_ref().and_then(|c|c.get("owner").ok())==context.get("owner").ok()){return Err("MCP GET owner already has an opening".into());}
        let token=sessions.next;sessions.next=token.checked_add(1).ok_or("MCP admission ticket exhausted")?;
        let context=obj(vec![("endpoint",s(endpoint)),("key",s(key)),("owner",s(owner)),("version",s(field(context,"version")?)),("session",s(session)),("request",s("GET")),("resumable",Bool(true)),("admission_token",Num(token.to_string())),("request_binding",context.get("request_binding")?.clone())]);sessions.openings.insert(token,context.clone());return Ok(context);
    }
    if action=="stream_open_cleanup" {
        let context=v.get("context")?;
        if let Ok(token)=context.get("resume_token"){
            if sessions.lease.as_ref().map(|l|token==&Num(l.token.to_string())&&l.reserved.as_deref()==context.get("owner").and_then(Json::text).ok()).unwrap_or(false){let lease=sessions.lease.take().unwrap();return super::mcp_lifecycle::plan(&lease.context,field(&lease.context,"session")?,false);}return Ok(Arr(vec![]));
        }
        let token=match context.get("admission_token")?{Num(n)=>n.parse::<u64>().map_err(|_|"Invalid MCP opening ticket")?,_=>return Err("Invalid MCP opening ticket".into())};
        if sessions.openings.get(&token).map(|c|c!=context).unwrap_or(false){return Err("MCP opening cleanup owner mismatch".into());}
        return if let Some(context)=sessions.openings.remove(&token){super::mcp_lifecycle::plan(&context,field(&context,"session")?,false)}else{Ok(Arr(vec![]))};
    }
    if action=="stream_owner_cancel" {
        let owner=field(v,"owner")?;if owner.is_empty()||owner.len()>256{return Err("Invalid MCP cancellation owner".into());}
        for t in sessions.entries.values_mut(){if t.server_stream&&t.control.as_ref().and_then(|c|c.get("owner").ok()).and_then(|v|v.text().ok())==Some(owner){t.resumable=false;t.poisoned=true;}}
        let tokens=sessions.openings.iter().filter(|(_,c)|c.get("owner").and_then(Json::text)==Ok(owner)).map(|(k,_)|*k).collect::<Vec<_>>();let mut plans=Vec::new();
        for token in tokens{let context=sessions.openings.remove(&token).unwrap();plans.extend(super::mcp_lifecycle::plan(&context,field(&context,"session")?,false)?.array()?.clone());}
        if sessions.lease.as_ref().map(|l|l.reserved.as_deref().or_else(||l.context.get("owner").and_then(Json::text).ok())==Some(owner)).unwrap_or(false){let lease=sessions.lease.take().unwrap();plans.extend(super::mcp_lifecycle::plan(&lease.context,field(&lease.context,"session")?,false)?.array()?.clone());}
        return Ok(Arr(plans));
    }
    if action=="stream_resume_close" {
        let token=v.get("token").ok();
        if let Some(lease)=sessions.lease.as_ref(){let owned=if let Some(token)=token{token==&Num(lease.token.to_string())}else{v.get("owner").and_then(Json::text).ok().filter(|owner|!owner.is_empty())==lease.reserved.as_deref().or_else(||lease.context.get("owner").and_then(Json::text).ok())};if !owned{return Err("MCP resume ticket is not current".into());}}
        return if let Some(lease)=sessions.lease.take(){super::mcp_lifecycle::plan(&lease.context,field(&lease.context,"session")?,false)}else{Ok(Arr(vec![]))};
    }
    if action=="stream_resume"||action=="stream_resume_probe"||action=="stream_resume_summary" {
        let lease=sessions.lease.as_ref().ok_or("Paused MCP stream is unavailable after restart or closure")?;
        if v.get("token")!=Ok(&Num(lease.token.to_string())){return Err("MCP resume ticket is not current".into());}
        let config=v.get("config")?;
        if config.get("endpoint")!=lease.context.get("endpoint")||config.get("key")!=lease.context.get("key"){return Err("MCP resume connection changed; close the paused stream".into());}
        if std::time::Instant::now()>=lease.until{return Err("MCP resume lifetime expired; close the paused stream".into());}
        if action=="stream_resume_summary"{if lease.reserved.is_some(){return Err("MCP cursor summary cannot adopt a reserved resume opening".into());}for key in ["owner","version","session"]{if v.get(key)!=lease.context.get(key){return Err("MCP cursor summary belongs to a different stream lifecycle".into());}}return Ok(Null);}
        if action=="stream_resume"&&lease.reserved.is_some(){return Err("MCP resume ticket was already reserved".into());}
        let reservation_token=if action=="stream_resume"{sessions.next}else{lease.token};
        let mut context=lease.context.clone();if let Obj(f)=&mut context{f.insert("owner".into(),v.get("owner")?.clone());f.insert("resumable".into(),Bool(true));f.insert("resume_token".into(),Num(reservation_token.to_string()));}
        let endpoint=field(config,"endpoint")?;let key=field(config,"key")?;
        let mut headers=vec![("Accept","text/event-stream".into()),("MCP-Protocol-Version",field(&context,"version")?.into()),("Last-Event-ID",lease.cursor.clone())];
        if !key.is_empty(){headers.push(("Authorization",format!("Bearer {key}")));}let session=field(&context,"session")?;if !session.is_empty(){headers.push(("Mcp-Session-Id",session.into()));}
        let mut e=network::wire(endpoint,endpoint.starts_with("http://"),"GET","",headers)?;
        if let Obj(f)=&mut e{f.insert("mcp_control".into(),context);f.insert("stream".into(),Bool(true));f.insert("stream_protocol".into(),s("mcp_server"));f.insert("purpose".into(),v.get("owner")?.clone());}
        if action=="stream_resume"{sessions.next=reservation_token.checked_add(1).ok_or("MCP reservation ticket exhausted")?;let lease=sessions.lease.as_mut().unwrap();lease.token=reservation_token;lease.reserved=Some(field(v,"owner")?.into());}return Ok(e);
    }
    if action == "stream_open" {
        if sessions.entries.len() >= 4 { return Err("Too many active reply streams".into()); }
        let id = sessions.next; sessions.next = id.checked_add(1).ok_or("Stream identifier exhausted")?;
        let protocol=v.get("protocol").and_then(Json::text).unwrap_or("local");
        if !["local","chat","mcp","mcp_server","messages","content"].contains(&protocol) {return Err("Unsupported incremental API format".into());}
        let control=if protocol=="mcp"||protocol=="mcp_server" {match v.get("mcp_control") {Ok(context)=>{
            let endpoint=field(context,"endpoint")?;network::url(endpoint,endpoint.starts_with("http://"))?;
            if endpoint.len()>8192||field(context,"key")?.len()>8192||field(context,"owner")?.is_empty()||field(context,"owner")?.len()>256||!["2025-03-26","2025-06-18"].contains(&field(context,"version")?){return Err("Invalid MCP control binding".into());}
            if let Ok(request)=context.get("request"){if !["initialize","tools/list","tools/call","GET"].contains(&request.text()?){return Err("Invalid MCP lifecycle request binding".into());}}
            let session=field(context,"session")?;if session.len()>256||!session.bytes().all(|b|(0x21..=0x7e).contains(&b)){return Err("Invalid MCP control session".into());}
            let expected=super::mcp_identity::captured(context)?;
            let mut bound=obj(vec![("endpoint",s(endpoint)),("key",s(field(context,"key")?)),("owner",s(field(context,"owner")?)),("version",s(field(context,"version")?)),("session",s(session)),("request",s(context.get("request").and_then(Json::text).unwrap_or(""))),("resumable",Bool(context.get("resumable")==Ok(&Bool(true)))),("request_binding",context.get("request_binding")?.clone())]);
            if context.get("request_binding")==Ok(&Num("2".into())){if let Some(id)=expected{if let Obj(fields)=&mut bound{fields.insert("request_id".into(),id);}}}
            Some(bound)},Err(_)=>return Err("MCP stream requires a request identity binding".into())}}else{None};
        if protocol=="mcp"&&control.as_ref().and_then(|c|c.get("request").ok()).and_then(|m|m.text().ok())==Some("GET"){return Err("MCP GET requires the standalone server stream mode".into());}
        if protocol=="mcp_server"&&control.as_ref().and_then(|c|c.get("request").ok()).and_then(|m|m.text().ok())!=Some("GET"){return Err("MCP server stream requires a GET lifecycle binding".into());}
        let resumable=protocol=="mcp_server"&&v.get("mcp_control").and_then(|c|c.get("resumable"))==Ok(&Bool(true));
        let mut cursor=String::new();let mut resume_until=if resumable{Some(std::time::Instant::now()+std::time::Duration::from_secs(600))}else{None};let mut resumes=0;
        if let Ok(token)=v.get("mcp_control").and_then(|c|c.get("resume_token")){
            let lease=sessions.lease.as_ref().ok_or("MCP resume lease was consumed")?;
            if token!=&Num(lease.token.to_string())||std::time::Instant::now()>=lease.until{return Err("MCP resume lease is no longer current".into());}
            if lease.reserved.as_deref()!=control.as_ref().and_then(|c|c.get("owner").ok()).and_then(|v|v.text().ok()){return Err("MCP resume owner is not reserved".into());}
            for key in ["endpoint","key","version","session"]{if control.as_ref().and_then(|c|c.get(key).ok())!=lease.context.get(key).ok(){return Err("MCP resume binding changed".into());}}
            let lease=sessions.lease.take().unwrap();cursor=lease.cursor;resume_until=Some(lease.until);resumes=lease.resumes+1;
        }
        if resumable&&v.get("mcp_control").and_then(|c|c.get("resume_token")).is_err(){
            let context=v.get("mcp_control")?;let token=match context.get("admission_token")?{Num(n)=>n.parse::<u64>().map_err(|_|"Invalid MCP opening ticket")?,_=>return Err("Invalid MCP opening ticket".into())};
            if sessions.openings.get(&token)!=Some(context){return Err("MCP GET opening was cancelled, consumed or changed".into());}sessions.openings.remove(&token);
        }
        sessions.entries.insert(id,Transfer{resumable,cursor,resume_until,resumes,control,server_stream:protocol=="mcp_server",chat:protocol!="local",mcp:protocol=="mcp"||protocol=="mcp_server",messages:protocol=="messages",content:protocol=="content",..Transfer::default()}); return Ok(Num(id.to_string()));
    }
    let id = match v.get("id")? { Num(n)=>n.parse::<u64>().map_err(|_| "Invalid stream identifier")?, _=>return Err("Invalid stream identifier".into()) };
    if action=="stream_suspend" {return suspend(&mut sessions,id,v.get("owner")?,true);}
    if action == "stream_abandon" {
        let transfer=sessions.entries.get(&id).ok_or("Reply stream is no longer active")?;
        let context=transfer.control.as_ref().ok_or("No MCP lifecycle binding")?;
        if context.get("owner").ok()!=v.get("owner").ok(){return Err("MCP abandon owner is not current".into());}
        let session=transfer.lifecycle_session()?;
        let completed=transfer.terminal.is_some()||transfer.json_terminal().is_ok();
        let plan=super::mcp_lifecycle::plan(context,session,v.get("cancel")==Ok(&Bool(true))&&!completed)?;
        sessions.entries.remove(&id);return Ok(plan);
    }
    if action == "stream_close" { sessions.entries.remove(&id); return Ok(Null); }
    if action=="stream_control_ack" || action=="stream_control_probe" {
        let transfer=sessions.entries.get(&id).ok_or("Reply stream is no longer active")?;
        let ticket=match v.get("ticket")?{Num(n)=>n.parse::<u64>().map_err(|_|"Invalid MCP control ticket")?,_=>return Err("Invalid MCP control ticket".into())};
        if transfer.control.as_ref().and_then(|c|c.get("owner").ok())!=v.get("owner").ok()||!transfer.control_pending.contains_key(&ticket){return Err("MCP control acknowledgment is not current".into());}
    }
    let result = (|| {
        let transfer = sessions.entries.get_mut(&id).ok_or("Reply stream is no longer active")?;
        if transfer.poisoned{return Err("MCP stream failed; only abandonment or close is allowed".into());}
        if action=="stream_control_ack" || action=="stream_control_probe" {
            let ticket=match v.get("ticket")?{Num(n)=>n.parse::<u64>().map_err(|_|"Invalid MCP control ticket")?,_=>return Err("Invalid MCP control ticket".into())};
            if transfer.control.as_ref().and_then(|c|c.get("owner").ok())!=v.get("owner").ok()||!transfer.control_pending.contains_key(&ticket){return Err("MCP control acknowledgment is not current".into());}
            let encoded=field(v,"data")?;if encoded.len()>((16384+2)/3)*4{return Err("MCP control acknowledgment exceeds encoded limit".into());}
            let bytes=network::unbase64(encoded)?;if bytes.len()>16384{return Err("MCP control acknowledgment exceeds 16 KiB".into());}
            if action=="stream_control_probe"{return Ok(obj(vec![("ready",Bool(control_framed(&bytes)&&network::decoded(&bytes).is_ok()))]));}
            if !control_framed(&bytes)&&v.get("eof")!=Ok(&Bool(true)){return Err("Close-delimited MCP acknowledgment requires observed EOF".into());}
            let(status,body)=network::decoded(&bytes)?;if status!=202||!body.is_empty(){return Err("MCP control response was not accepted with empty HTTP 202".into());}
            transfer.control_pending.remove(&ticket);transfer.cursor_commit();return Ok(obj(vec![("complete",Bool(transfer.framed()))]));
        }
        if action == "stream_finish" {
            transfer.observed_eof=v.get("eof")==Ok(&Bool(true));
            if transfer.server_stream&&!transfer.framed()&&v.get("eof")!=Ok(&Bool(true)){return Err("Close-delimited MCP server stream requires observed EOF".into());}
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
            if transfer.mcp {
                let cleanup=transfer.control.as_ref().and_then(|context|{super::mcp_lifecycle::plan(context,transfer.lifecycle_session().ok()?,false).ok()}).unwrap_or(Arr(vec![]));
                return Ok(obj(vec![("delta",s("")),("complete",Bool(false)),("error",s(&error)),("cleanup",cleanup),("observation",transfer.server_observation())]));
            }
            if transfer.messages||transfer.content {return Ok(obj(vec![("delta",s(&transfer.text[previous..])),("complete",Bool(false)),("error",s(&error))]));}
            return Err(error);
        }
        let controls=std::mem::take(&mut transfer.controls);
        Ok(obj(vec![("delta",s(&transfer.text[previous..])),("complete",Bool(transfer.framed())),("controls",Arr(controls)),("observation",transfer.server_observation())]))
    })();
    if action=="stream_finish"&&result.is_ok()&&sessions.entries.get(&id).map(|t|t.resumable&&!t.cursor.is_empty()&&t.resumes<4&&t.resume_until.map(|u|std::time::Instant::now()<u).unwrap_or(false)).unwrap_or(false){
        let owner=sessions.entries[&id].control.as_ref().unwrap().get("owner")?.clone();return suspend(&mut sessions,id,&owner,false);
    }
    let retain_failed_cleanup=result.is_err()&&sessions.entries.get(&id).map(|t|t.mcp&&(action=="stream_finish"||t.control.as_ref().and_then(|c|c.get("request").ok()).and_then(|m|m.text().ok())==Some("initialize"))).unwrap_or(false);
    if retain_failed_cleanup{if let Some(t)=sessions.entries.get_mut(&id){t.poisoned=true;}}
    else if result.is_err() || result.as_ref().map(|v|v.get("error").is_ok()).unwrap_or(false) || action == "stream_finish" { sessions.entries.remove(&id); }
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

#[cfg(test)] mod control_checks {
 use super::*;
 fn transfer()->Transfer{Transfer{mcp:true,control:Some(obj(vec![("request_binding",Num("1".into())),("endpoint",s("https://server.example/proxy/mcp?q=x")),("key",s("REMOTE")),("version",s("2025-06-18")),("session",s("session")),("owner",s("phase"))])),..Transfer::default()}}
 #[test]fn server_id_direction_and_bounded_untrusted_requests(){
  let mut t=transfer();t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num("1".into())),("method",s("ping"))])).unwrap();
  assert!(field(&t.controls[0],"wire").unwrap().contains(r#""result":{}"#));assert_eq!(t.control_pending.len(),1);assert!(t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num("1".into())),("method",s("ping"))])).is_err());
  t.control_pending.clear();t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num("1".into())),("method",s("sampling/createMessage")),("params",obj(vec![("prompt",s("UNTRUSTED"))]))])).unwrap();let w=field(t.controls.last().unwrap(),"wire").unwrap();assert!(w.contains("-32601"));assert!(!w.contains("UNTRUSTED"));
 }
 #[test]fn notification_envelope_validation_preserves_request_error_replies(){
  for params in [Null,Arr(vec![]),Bool(false),obj(vec![("_meta",Null)])] {
   let record=obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/custom")),("params",params)]);assert!(Transfer::control_reply(&record,"2025-06-18").is_err());
  }
  let record=obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/custom")),("params",obj(vec![("_meta",obj(vec![("progressToken",Null)]))]))]);assert!(Transfer::control_reply(&record,"2025-06-18").unwrap().is_none());
  for (method,code) in [("ping","-32602"),("unknown","-32601")] {
   let record=obj(vec![("jsonrpc",s("2.0")),("method",s(method)),("id",Num("1".into())),("params",Null)]);let (_,reply)=Transfer::control_reply(&record,"2025-06-18").unwrap().unwrap();assert_eq!(reply.get("error").unwrap().get("code"),Ok(&Num(code.into())));
  }
 }
 #[test]fn control_limits_and_invalid_envelopes_refuse_without_authority(){
  let mut t=transfer();for i in 0..8{t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num(i.to_string())),("method",s("ping"))])).unwrap();}
  assert!(t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",s("overflow")),("method",s("ping"))])).is_err());
  let mut t=transfer();for id in [Null,Bool(true),obj(vec![])]{assert!(t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",id),("method",s("ping"))])).is_err());}
  t.control_request(&obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/custom"))])).unwrap();assert!(t.controls.is_empty());
 }
 #[test]fn current_session_is_bound_and_completion_waits_for_ack(){
  let mut t=transfer();t.mcp_session="different".into();assert!(t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",s("x")),("method",s("ping"))])).is_err());
  t.mcp_session="session".into();t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",s("x")),("method",s("ping"))])).unwrap();t.headers=true;t.terminal=Some(obj(vec![("jsonrpc",s("2.0")),("id",Num("1".into())),("result",obj(vec![]))]));assert!(!t.framed());assert!(t.finish().is_err());t.control_pending.clear();assert!(t.framed());assert!(t.finish().is_ok());
 }
}

#[cfg(test)] mod server_stream_checks {
 use super::*;
 fn transfer()->Transfer {Transfer{chat:true,mcp:true,server_stream:true,control:Some(obj(vec![("request_binding",Num("1".into())),("endpoint",s("http://127.0.0.1:9999/mcp?q=old")),("key",s("OLD")),("version",s("2025-06-18")),("session",s("session")),("owner",s("server-phase")),("request",s("GET"))])),..Transfer::default()}}
 fn event(j:Json)->String {format!("data: {}\n\n",j.string())}
 fn header()->&'static str {"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nMcp-Session-Id: session\r\n\r\n"}
 #[test]fn standalone_server_stream_counts_only_messages_and_waits_ack(){
  let mut t=transfer();t.feed(header().as_bytes()).unwrap();
  t.feed(event(obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/tools/list_changed")),("params",obj(vec![("untrusted",s("never instructions"))]))])).as_bytes()).unwrap();
  t.feed(event(obj(vec![("jsonrpc",s("2.0")),("id",Num("1".into())),("method",s("ping"))])).as_bytes()).unwrap();assert!(!t.framed());assert!(t.finish().is_err());assert!(t.text.is_empty()&&t.terminal.is_none());
  t.control_pending.clear();let summary=parse(&t.finish().unwrap()).unwrap();assert_eq!(summary.get("requests"),Ok(&Num("1".into())));assert_eq!(summary.get("notifications"),Ok(&Num("1".into())));assert!(!summary.string().contains("untrusted"));
 }
 #[test]fn standalone_server_stream_rejects_responses_changed_sessions_and_non_sse(){
  for j in [obj(vec![("jsonrpc",s("2.0")),("id",Num("1".into())),("result",obj(vec![]))]),obj(vec![("jsonrpc",s("2.0")),("id",Num("99".into())),("error",Null)]),obj(vec![("jsonrpc",s("2.0")),("method",s("ping")),("id",Num("1".into())),("result",Null)])]{let mut t=transfer();t.feed(header().as_bytes()).unwrap();assert!(t.feed(event(j).as_bytes()).is_err());}
  for h in [header().replace("HTTP/1.1","NOTHTTP"),header().replace("200 OK","0200 OK"),header().replace("session","different"),header().replace("text/event-stream","application/json"),header().replace("Content-Type: text/event-stream\r\n","")]{assert!(transfer().feed(h.as_bytes()).is_err());}
  let mut t=transfer();t.feed(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 5\r\n\r\nx").unwrap();assert!(t.finish().is_err());
 }
 #[test]fn standalone_server_stream_notifications_and_message_bytes_are_bounded(){
  let n=event(obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/tools/list_changed"))]));let mut t=transfer();t.feed(header().as_bytes()).unwrap();for _ in 0..256{t.feed(n.as_bytes()).unwrap();}assert!(t.feed(n.as_bytes()).is_err());
  let mut t=transfer();t.feed(header().as_bytes()).unwrap();assert!(t.feed(event(obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/logging")),("params",obj(vec![("data",s(&"x".repeat(4096)))]))])).as_bytes()).is_err());
 }
}

#[cfg(test)] mod resume_checks{
 use super::*;
 fn transfer()->Transfer{Transfer{mcp:true,chat:true,server_stream:true,resumable:true,control:Some(obj(vec![("request_binding",Num("1".into())),("endpoint",s("http://example.test/mcp?q=old")),("key",s("old")),("version",s("2025-06-18")),("session",s("known")),("owner",s("one")),("request",s("GET"))])),resume_until:Some(std::time::Instant::now()+std::time::Duration::from_secs(600)),..Transfer::default()}}
 fn header()->&'static[u8]{b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n"}
 #[test]fn cursor_waits_for_controls_and_empty_id_clears(){
  let mut t=transfer();t.feed(header()).unwrap();
  t.feed(b"id: before\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/custom\"}\n\nid: ping\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\nid: after\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/custom\"}\n\n").unwrap();
  assert_eq!(t.cursor,"before");assert_eq!(t.pending_cursor.as_deref(),Some("after"));
  t.control_pending.clear();t.cursor_commit();assert_eq!(t.cursor,"after");
  t.feed(b"id:\n\n").unwrap();assert!(t.cursor.is_empty());
  for part in b"id: split\n\n".chunks(1){t.feed(part).unwrap();}assert_eq!(t.cursor,"split");
  t.feed(b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/custom\"}\n\n").unwrap();assert_eq!(t.cursor,"split");
 }
 #[test]fn cursor_bounds_and_get_direction_are_unchanged(){
  for bad in ["\0".into(),"é".into(),"x".repeat(257),"\t".into()," leading".into(),"trailing ".into()]{let mut t=transfer();t.feed(header()).unwrap();assert!(t.feed(format!("id: {bad}\n\n").as_bytes()).is_err());}
  let mut t=transfer();t.feed(header()).unwrap();t.cursor="old".into();assert!(t.feed(b"id: new\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n").is_err());assert_eq!(t.cursor,"old");
 }
 #[test]fn suspend_requires_owner_ack_cursor_and_original_lifetime(){
  let mut t=transfer();t.feed(header()).unwrap();t.cursor="cursor".into();
  let mut sessions=Sessions{next:20,entries:BTreeMap::from([(1,t)]),openings:BTreeMap::new(),lease:None};
  assert!(suspend(&mut sessions,1,&s("other"),true).is_err());
  sessions.entries.get_mut(&1).unwrap().control_pending.insert(1,vec!["pending".into()]);assert!(suspend(&mut sessions,1,&s("one"),true).is_err());sessions.entries.get_mut(&1).unwrap().control_pending.clear();
  let summary=suspend(&mut sessions,1,&s("one"),true).unwrap();assert!(summary.get("data").is_ok());assert!(sessions.entries.is_empty());let lease=sessions.lease.unwrap();assert_eq!(lease.token,20);assert_eq!(lease.cursor,"cursor");assert_eq!(field(&lease.context,"key").unwrap(),"old");
  let mut expired=transfer();expired.feed(header()).unwrap();expired.cursor="cursor".into();expired.resume_until=Some(std::time::Instant::now());let mut sessions=Sessions{next:1,entries:BTreeMap::from([(1,expired)]),openings:BTreeMap::new(),lease:None};assert!(suspend(&mut sessions,1,&s("one"),true).is_err());
 }
}

#[cfg(test)] mod batch_checks {
 #[test] fn integer_numeric_batch_ids_and_correlated_response_spellings_preserve_identity(){
  use super::*;
  for id in ["1.0","1e0","10e-1","0.01e2"] {
   let record=crate::parse(&format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#)).unwrap();assert!(rpc_result(&record).unwrap());assert!(batch_result(vec![record]).unwrap().is_some());
   let records=crate::parse(&format!(r#"[{{"jsonrpc":"2.0","id":1,"result":{{}}}},{{"jsonrpc":"2.0","id":{id},"result":{{}}}}]"#)).unwrap();let Arr(records)=records else{panic!()};assert!(batch_result(records).is_err());
  }
  for id in ["12.30e1","1e999999999999999999999999","-0e-99999999999999999999999"] {assert!(batch_id(&Num(id.into())).is_ok());}
  for id in ["1.25","1e-99999999999999999999999","1.000000000000000000001"] {assert!(batch_id(&Num(id.into())).is_err());}
  assert_ne!(batch_id(&s("1")).unwrap(),batch_id(&Num("1.0".into())).unwrap());
  assert!(batch_id(&Num(format!("1e{}","0".repeat(62)))).is_ok());assert!(batch_id(&Num(format!("1e{}","0".repeat(63)))).is_err());
  // Existing single server-control fractional-ID compatibility remains intact.
  let request=crate::parse(r#"{"jsonrpc":"2.0","id":1.25,"method":"ping"}"#).unwrap();let (_,reply)=Transfer::control_reply(&request,"2025-06-18").unwrap().unwrap();assert_eq!(reply.get("id"),Ok(&Num("1.25".into())));
 }

 use super::*;
 fn transfer(request:&str,version:&str)->Transfer{Transfer{mcp:true,server_stream:request=="GET",control:Some(obj(vec![("request_binding",Num("1".into())),("endpoint",s("http://127.0.0.1:32123/proxy?old=yes")),("key",s("old")),("version",s(version)),("session",s("session")),("owner",s("batch-owner")),("request",s(request))])),..Transfer::default()}}
 fn records(value:&str)->Vec<Json>{parse(value).unwrap().array().unwrap().clone()}
 #[test]fn numeric_pending_comparison_is_exact_and_input_spelling_is_preserved(){
  for spelling in ["1.0","1e0","1.000e+000","10e-1"]{let mut t=transfer("GET","2025-03-26");t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num(spelling.into())),("method",s("ping"))])).unwrap();let wire=field(&t.controls[0],"wire").unwrap();let reply=parse(wire.split("\r\n\r\n").nth(1).unwrap()).unwrap();assert_eq!(reply.get("id"),Ok(&Num(spelling.into())));assert!(t.batch(records(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)).is_err());}
  for (a,b) in [("-0","0"),("0e99999999999999999999999999999999999999999999999999","0"),("9007199254740993.0","9007199254740993"),("1.50","15e-1")]{assert_eq!(control_key(&Num(a.into())),control_key(&Num(b.into())));}
  assert_ne!(control_key(&Num("9007199254740993.0".into())),control_key(&Num("9007199254740992".into())));assert_ne!(control_key(&s("1")),control_key(&Num("1".into())));
  assert_eq!(decimal_exponent("999999999999999999999999999999999999",1),Some("1000000000000000000000000000000000000".into()));assert_eq!(decimal_exponent("-1000000000000000000000000000000000000",1),Some("-999999999999999999999999999999999999".into()));
  let mut t=transfer("GET","2025-03-26");t.batch(records(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)).unwrap();assert!(t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num("1e0".into())),("method",s("ping"))])).is_err());
 }
 #[test]fn json_response_batches_require_media_framing_eof_and_correlation(){
  let body=r#"[{"jsonrpc":"2.0","id":99,"result":{}},{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}]"#;
  let wire=|headers:&str,body:&str|format!("HTTP/1.1 200 OK\r\n{headers}\r\n{body}");
  let mut t=transfer("tools/list","2025-03-26");t.feed(wire(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n",body.len()),body).as_bytes()).unwrap();assert!(t.framed());assert!(t.json_terminal().is_ok());assert!(t.terminal.is_none());assert_eq!(parse(&t.finish().unwrap()).unwrap().get("id"),Ok(&Num("1".into())));
  let mut t=transfer("tools/list","2025-03-26");t.feed(wire("Content-Type: application/json\r\n",body).as_bytes()).unwrap();assert!(t.finish().is_err());assert!(t.json_terminal().is_err());t.observed_eof=true;assert!(t.json_terminal().is_ok());assert!(t.finish().is_ok());
  for headers in [format!("Content-Type: text/plain\r\nContent-Length: {}\r\n",body.len()),format!("Content-Type: text/event-stream\r\nContent-Length: {}\r\n",body.len()),format!("Content-Type: application/json\r\nContent-Length: {}\r\n",body.len()+1),format!("Content-Type: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",body.len())]{let mut t=transfer("tools/list","2025-03-26");if t.feed(wire(&headers,body).as_bytes()).is_ok(){t.observed_eof=true;assert!(t.finish().is_err());}}
  for(request,version)in[("tools/list","2025-06-18"),("initialize","2025-03-26")]{let mut t=transfer(request,version);t.feed(wire(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n",body.len()),body).as_bytes()).unwrap();assert!(t.finish().is_err());}
  let chunk=format!("{:x}\r\n{}\r\n0\r\n\r\n",body.len(),body);let mut t=transfer("tools/list","2025-03-26");t.feed(wire("Content-Type: application/json\r\nTransfer-Encoding: chunked\r\n",&chunk).as_bytes()).unwrap();assert!(t.finish().is_ok());
  let mut t=transfer("tools/list","2025-03-26");t.feed(wire("Content-Type: application/json\r\nTransfer-Encoding: chunked\r\n",chunk.trim_end()).as_bytes()).unwrap();t.observed_eof=true;assert!(t.finish().is_err());
  for bad in [r#"[{"jsonrpc":"2.0","id":99,"result":{}}]"#,r#"[{"jsonrpc":"2.0","id":1,"result":{}},{"jsonrpc":"2.0","id":99,"error":null}]"#,r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#]{let mut t=transfer("tools/list","2025-03-26");t.feed(wire(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n",bad.len()),bad).as_bytes()).unwrap();assert!(t.finish().is_err());assert!(t.controls.is_empty());}
  assert!(!integer_id("01")&&!integer_id("-01")&&!integer_id("+1")&&integer_id("-0"));
 }
 #[test]fn request_batch_one_reply_group_and_cursor_ack_barrier(){
  let mut t=transfer("GET","2025-03-26");t.cursor="before".into();t.event_id=Some("after".into());
  t.batch(records(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/custom","params":{"secret":"never"}},{"jsonrpc":"2.0","id":"x","method":"sampling/createMessage","params":{"prompt":"never echo"}},{"jsonrpc":"2.0","id":2,"method":"ping","params":[]}]"#)).unwrap();
  assert_eq!(t.controls.len(),1);assert_eq!(t.pending_count(),3);assert_eq!(t.control_count,3);assert_eq!(t.server_messages,4);assert_eq!(t.server_notifications,1);assert_eq!(t.cursor,"before");
  let wire=field(&t.controls[0],"wire").unwrap();assert!(wire.starts_with("POST /proxy?old=yes "));assert!(wire.contains("Mcp-Session-Id: session\r\n"));assert!(!wire.contains("never"));
  let replies=parse(wire.split("\r\n\r\n").nth(1).unwrap()).unwrap();let replies=replies.array().unwrap();assert_eq!(replies.len(),3);assert_eq!(replies[0].get("result"),Ok(&obj(vec![])));assert_eq!(replies[1].get("error").unwrap().get("code"),Ok(&Num("-32601".into())));assert_eq!(replies[2].get("error").unwrap().get("code"),Ok(&Num("-32602".into())));
  t.control_pending.clear();t.cursor_commit();assert_eq!(t.cursor,"after");
  let mut t=transfer("GET","2025-03-26");t.batch(records(r#"[{"jsonrpc":"2.0","method":"notifications/custom"}]"#)).unwrap();assert!(t.controls.is_empty());assert_eq!(t.server_messages,1);
 }
 #[test]fn whole_event_atomicity_direction_and_all_response_validation(){
  for bad in [r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/custom","params":[]}]"#,r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","id":1,"result":{}}]"#,r#"[{"jsonrpc":"2.0","id":0,"method":"ping"},{"jsonrpc":"2.0","id":-0,"method":"ping"}]"#,r#"[{"jsonrpc":"2.0","id":1.25,"method":"ping"}]"#,r#"[]"#,r#"[[]]"#]{let mut t=transfer("GET","2025-03-26");t.cursor="old".into();t.event_id=Some("new".into());assert!(t.batch(records(bad)).is_err());assert!(t.controls.is_empty()&&t.control_pending.is_empty());assert_eq!(t.server_messages,0);assert_eq!(t.cursor,"old");}
  for bad in [r#"[{"jsonrpc":"2.0","id":1,"result":{}},{"jsonrpc":"2.0","id":99,"error":{"code":1.5,"message":"bad"}}]"#,r#"[{"jsonrpc":"2.0","id":1,"result":{}},{"jsonrpc":"2.0","id":1,"result":{}}]"#,r#"[{"jsonrpc":"2.0","id":1,"result":null}]"#,r#"[{"jsonrpc":"2.0","id":1,"result":{}},{"jsonrpc":"2.0","id":99,"result":{},"error":null}]"#]{let mut t=transfer("tools/call","2025-03-26");assert!(t.batch(records(bad)).is_err());assert!(t.terminal.is_none());}
  let mut t=transfer("tools/call","2025-03-26");t.batch(records(r#"[{"jsonrpc":"2.0","id":"1","result":{}},{"jsonrpc":"2.0","id":1,"result":{"content":[]}}]"#)).unwrap();assert_eq!(t.terminal.unwrap().get("id"),Ok(&Num("1".into())));
  let mut t=transfer("GET","2025-03-26");assert!(t.batch(records(r#"[{"jsonrpc":"2.0","id":1,"result":{}}]"#)).is_err());
  for(request,version)in[("initialize","2025-03-26"),("GET","2025-06-18"),("tools/call","2025-06-18")]{let mut t=transfer(request,version);assert!(t.batch(records(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)).is_err());assert!(t.controls.is_empty());}
 }
 #[test]fn batch_bounds_count_individual_pending_ids_and_preserve_single_paths(){
  let mut t=transfer("GET","2025-03-26");let rows=(0..8).map(|id|obj(vec![("jsonrpc",s("2.0")),("id",Num(id.to_string())),("method",s("ping"))])).collect();t.batch(rows).unwrap();assert_eq!(t.control_pending.len(),1);assert_eq!(t.pending_count(),8);assert!(t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",s("overflow")),("method",s("ping"))])).is_err());
  t.control_pending.clear();t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num("-0".into())),("method",s("ping"))])).unwrap();assert!(t.batch(records(r#"[{"jsonrpc":"2.0","id":0,"method":"ping"}]"#)).is_err());t.batch(records(r#"[{"jsonrpc":"2.0","id":"0","method":"ping"}]"#)).unwrap();
  for size in [33,257]{let mut t=transfer("GET","2025-03-26");let rows=(0..size).map(|_|obj(vec![("jsonrpc",s("2.0")),("method",s("notifications/custom"))])).collect();assert!(t.batch(rows).is_err());assert_eq!(t.server_messages,0);}
  let mut t=transfer("GET","2025-03-26");t.server_messages=255;assert!(t.batch(records(r#"[{"jsonrpc":"2.0","method":"notifications/custom"},{"jsonrpc":"2.0","method":"notifications/custom"}]"#)).is_err());
  let mut t=transfer("tools/call","2025-03-26");let context=t.control.as_mut().unwrap();if let Obj(fields)=context{fields.insert("endpoint".into(),s(&format!("http://127.0.0.1/{}","r".repeat(8000))));fields.insert("key".into(),s(&"k".repeat(8192)));}
  let rows=(0..8).map(|i|obj(vec![("jsonrpc",s("2.0")),("id",s(&format!("{}{}",i,"\u{0001}".repeat(255)))),("method",s("ping"))])).collect();assert!(t.batch(rows).is_err());assert!(t.controls.is_empty()&&t.control_pending.is_empty()&&t.terminal.is_none());assert_eq!(t.control_count,0);
  let mut t=transfer("GET","2025-06-18");t.control_request(&obj(vec![("jsonrpc",s("2.0")),("id",Num("1.0".into())),("method",s("ping"))])).unwrap();assert_eq!(t.pending_count(),1); // Existing single-message numeric behavior remains unchanged.
 }
}
