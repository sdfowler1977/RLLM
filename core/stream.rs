use super::*;
use super::provider::{copy_document_text, parse_stream_document, serialize_stream_document, DOCUMENT_MEMORY};
use std::sync::{Mutex, OnceLock};

const MCP_WIRE_LIMIT: usize = 2 * 1024 * 1024;
const MCP_TEXT_LIMIT: usize = 1024 * 1024;
const STREAM_MEMORY: &str = "Insufficient memory to decode the provider stream";
const CHUNK_HEADER_TOO_LONG: &str = "Stream chunk header exceeds limit";

/// Incremental text is provisional. A completed reply is returned only after
/// the ordinary HTTP decoder validates the complete wire response.
#[derive(Default)]
struct Transfer {
    chat: bool,
    mcp: bool,
    mcp_event_stream: bool,
    server_stream: bool,
    resumable: bool,
    cursor: String,
    event_id: Option<String>,
    pending_cursor: Option<String>,
    resume_until: Option<std::time::Instant>,
    resumes: usize,
    retry_ms: Option<u64>,
    server_notifications: usize,
    tool_changes: usize,
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
    close_delimited: bool,
    control: Option<Json>,
    controls: Vec<Json>,
    control_pending: BTreeMap<u64,Vec<String>>,
    control_count: usize,
    raw: Vec<u8>,
    offset: usize,
    headers: bool,
    bodyless: bool,
    interim: usize,
    expected: Option<usize>,
    chunked: bool,
    remaining: usize,
    trailers: bool,
    trailer_bytes: usize,
    trailers_complete: bool,
    json: bool,
    line: Vec<u8>,
    text: String,
    terminal: Option<Json>,
    event: String,
    carriage: bool,
    first_line: bool,
    finish_reason: Option<String>,
    usage: Option<Json>,
    call_fragments: super::provider_calls::Fragments,
    native_calls: Vec<Json>,
    text_part_start: usize,
    open_call: Option<Json>,
    call_input: String,
    call_block: Option<usize>,
}
impl Transfer {
    fn request_id(&self)->Result<Json>{super::mcp_identity::captured(self.control.as_ref().ok_or("MCP request binding unavailable")?)?.ok_or("MCP GET has no client request ID".into())}
    fn server_observation(&self)->Json {
        if !self.server_stream { return Null; }
        obj(vec![
            ("notifications",Num(self.server_notifications.to_string())),
            ("requests",Num(self.control_count.to_string())),
            ("messages",Num(self.server_messages.to_string())),
            ("tool_changes",Num(self.tool_changes.to_string())),
        ])
    }
    fn catalog_notification(&self, record: &Json) -> bool {
        self.terminal.is_none()
            && self.control.as_ref().is_some_and(|context| {
                context.get("tools_list_changed") == Ok(&Bool(true))
                    && (self.server_stream || (self.mcp_event_stream
                        && matches!(field(context,"request"), Ok("tools/list" | "tools/call"))))
            })
            && record.get("method").and_then(Json::text) == Ok("notifications/tools/list_changed")
    }
    fn lifecycle_session(&self)->Result<&str>{
        let context=self.control.as_ref().ok_or("No MCP lifecycle binding")?;
        let configured=field(context,"session")?;
        if !configured.is_empty(){return Ok(configured);}
        if self.headers&&context.get("request").and_then(Json::text)==Ok("initialize"){return Ok(&self.mcp_session);}
        Ok("")
    }
    // Structured calls are retained until the complete provider turn is framed.
    // They remain proposals; no stream event executes application operations.
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
                if block_kind=="server_tool_use" {return Err("Provider-hosted tool blocks require their configured hosted workflow".into());}
                if block_kind=="tool_use" {
                    super::provider_calls::canonical(field(block,"name")?,block.get("input")?)?;
                    let id=field(block,"id")?;if id.is_empty()||id.len()>256||id.chars().any(char::is_control){return Err("Invalid streamed tool identity".into());}
                    self.open_call=Some(block.clone());self.call_input.clear();self.call_block=Some(index);
                }
                let text=block_kind=="text";
                if text {
                    if self.text_blocks > 0 { self.append("\n")?; }
                    self.text_blocks += 1;
                    self.text_part_start = self.text.len();
                    self.append(field(block, "text")?)?;
                }
                self.block=Some((index,text));self.blocks+=1;
            },
            "content_block_delta" => {
                let (index,is_text)=self.block.ok_or("Messages delta has no open block")?;
                if index!=message_index(record)? {return Err("Messages delta has the wrong block index".into());}
                let delta=record.get("delta")?;let kind=field(delta,"type")?;
                if kind=="text_delta" {if !is_text {return Err("Text delta does not belong to a text block".into());}self.append(field(delta,"text")?)?;}
                else if kind=="input_json_delta" {
                    if is_text||self.call_block!=Some(index){return Err("Tool input delta does not belong to a tool block".into());}
                    let fragment=field(delta,"partial_json")?;
                    if self.call_input.len().saturating_add(fragment.len())>65536{return Err("Streamed tool arguments exceed 64 KiB".into());}
                    self.call_input.push_str(fragment);
                }
                else if is_text && ["thinking_delta","signature_delta"].contains(&kind) {return Err("Non-text delta does not belong to a text block".into());}
                // Other typed deltas are not promoted to generated text or tools.
            },
            "content_block_stop" => {
                let (index,is_text)=self.block.ok_or("Messages stop has no open block")?;
                if index!=message_index(record)? {return Err("Messages stop has the wrong block index".into());}
                if is_text {
                    let text = copy_document_text(&self.text[self.text_part_start..])?;
                    self.native_calls.push(obj(vec![("type", s("text")), ("text", Str(text))]));
                }
                if self.call_block==Some(index){
                    let mut call=self.open_call.take().ok_or("Messages stop has no open tool call")?;
                    if !self.call_input.is_empty(){
                        if call.get("input")!=Ok(&obj(vec![])){return Err("Streamed tool input supplied both initial fields and JSON deltas".into());}
                        let input=parse(&self.call_input).unwrap_or_else(|_|s(&self.call_input));
                        if let Obj(fields)=&mut call{fields.insert("input".into(),input);}
                        self.call_input.clear();
                    }
                    self.native_calls.push(call);
                    self.call_block=None;
                }
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
                if reason=="pause_turn" {return Err("Messages reply requires its configured hosted continuation".into());}
                let mut message=self.message.take().ok_or("Messages stream has no start")?;
                if let Obj(fields)=&mut message {
                    fields.insert("content".into(),Arr(std::mem::take(&mut self.native_calls)));
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
                for part in parts {
                    if let Some(text)=content_part(part)? {self.append(text)?;}
                    if part.get("functionCall").is_ok() {
                        self.flush_content_text()?;
                        self.native_calls.push(part.clone());
                    }
                }
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
    fn flush_content_text(&mut self) -> Result<()> {
        if self.text_part_start < self.text.len() {
            let text = copy_document_text(&self.text[self.text_part_start..])?;
            self.native_calls.push(obj(vec![("text", Str(text))]));
            self.text_part_start = self.text.len();
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
        super::mcp_identity::session_binding(context.get("request").and_then(Json::text).unwrap_or(""),configured,if self.mcp_session.is_empty(){None}else{Some(&self.mcp_session)})?;
        let session=if self.mcp_session.is_empty(){configured}else{&self.mcp_session};
        let mut headers=vec![("Content-Type","application/json".into()),("Accept","application/json, text/event-stream".into())];
        if context.get("request").and_then(Json::text)!=Ok("initialize"){headers.push(("MCP-Protocol-Version",version.into()));}
        let key=context.get("key").and_then(Json::text).unwrap_or("");
        if let Some(token)=network::mcp_bearer_token(&key)?{headers.push(network::mcp_authorization_header(&token)?);}
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
    fn accept_mcp_terminal(&mut self, record: Json) -> Result<()> {
        if self.terminal.is_some() {
            return Err("Duplicate correlated MCP terminal response".into());
        }
        self.terminal = Some(record);
        Ok(())
    }
    fn batch(&mut self,records:Vec<Json>)->Result<()> {
        let context=self.control.as_ref().ok_or("MCP batch transport binding unavailable")?;
        if field(context,"version")?!="2025-03-26"||!["tools/list","tools/call","GET"].contains(&field(context,"request")?){return Err("MCP SSE batches require an already-negotiated March tools or GET stream".into());}
        if records.is_empty()||records.len()>32{return Err("MCP batch requires one to 32 messages".into());}
        let requests=records[0].get("method").is_ok();
        if records.iter().any(|record|!matches!(record,Obj(_))||record.get("method").is_ok()!=requests){return Err("MCP batch directions must be homogeneous objects".into());}
        if requests {
            if self.server_stream&&self.server_messages+records.len()>256{return Err("MCP server stream exceeds 256 messages".into());}
            let mut ids=Vec::new();let mut replies=Vec::new();let mut notifications=0;let mut tool_changes=0;
            // Stage only bounded IDs and canonical replies. No raw/text buffers are cloned.
            for record in &records {
                if let Ok(id)=record.get("id"){batch_id(id)?;}else if record.get("params").map(|v|!matches!(v,Obj(_))).unwrap_or(false){return Err("Invalid MCP batch notification parameters".into());}
                if let Some((key,reply))=Self::control_reply(record,field(self.control.as_ref().ok_or("MCP notification binding unavailable")?,"version")?)? {
                    if self.terminal.is_some(){return Err("MCP request followed the completed response".into());}
                    if ids.contains(&key)||self.control_pending.values().any(|pending|pending.contains(&key)){return Err("Duplicate pending MCP server request ID".into());}
                    ids.push(key);replies.push(reply);
                }else{notifications+=1;tool_changes+=usize::from(self.catalog_notification(record));}
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
            self.tool_changes+=tool_changes;
        }else{
            if self.server_stream{return Err("MCP GET-origin streams cannot receive response batches".into());}
            if let Some(terminal)=batch_result_for(records,&self.request_id()?)? {
                self.accept_mcp_terminal(terminal)?;
            }
        }
        self.cursor_event();Ok(())
    }
    fn append(&mut self, content: &str) -> Result<()> {
        self.text.try_reserve(content.len()).map_err(|_| STREAM_MEMORY)?;
        self.text.push_str(content); Ok(())
    }
    fn event(&mut self) -> Result<()> {
        let data = std::mem::take(&mut self.event);
        let event_name = std::mem::take(&mut self.event_name);
        let data = data.strip_suffix('\n').unwrap_or(&data);
        if data.is_empty() { self.cursor_event(); return Ok(()); }
        if self.mcp {
            if self.server_stream&&data.len()>4096{return Err("MCP server message exceeds 4 KiB".into());}
            let record=parse(data)?;
            if let Arr(records)=record{return self.batch(records);}
            if self.server_stream {
                if self.server_messages>=256{return Err("MCP server stream exceeds 256 messages".into());}
                if record.get("method").is_err(){return Err("Unsolicited MCP responses are not allowed on a non-resumed server stream".into());}
                if record.get("id").is_err()&&record.get("params").map(|v|!matches!(v,Obj(_))).unwrap_or(false){return Err("Invalid MCP server notification parameters".into());}
                self.control_request(&record)?;
                if record.get("id").is_err(){self.server_notifications+=1;self.tool_changes+=usize::from(self.catalog_notification(&record));}
                self.server_messages+=1;self.cursor_event();return Ok(());
            }
            if record.get("method").is_ok(){
                if self.terminal.is_some()&&record.get("id").is_ok(){return Err("MCP request followed the completed response".into());}
                self.control_request(&record)?;
                if record.get("id").is_err() {self.tool_changes+=usize::from(self.catalog_notification(&record));}
            }else if rpc_result_for(&record,&self.request_id()?)? {
                self.accept_mcp_terminal(record)?;
            }
            return Ok(());
        }
        if self.content {return self.content_event(&parse(data)?);}
        if self.terminal.is_some() { return Err("Unexpected data after the completed reply".into()); }
        if self.messages {return self.message_event(&parse(data)?, &event_name);}
        if data == "[DONE]" {
            let reason = self.finish_reason.as_ref().ok_or("Stream ended without a completed choice")?;
            let mut message=obj(vec![("role",s("assistant")),("content",Str(copy_document_text(&self.text)?))]);
            if !super::provider::limit_reason("chat",reason){
                let calls=self.call_fragments.complete()?;
                if !calls.is_empty(){if let Obj(fields)=&mut message{fields.insert("tool_calls".into(),Arr(calls));}}
            }
            let mut fields = vec![("choices",Arr(vec![obj(vec![("index",Num("0".into())),("message",message),("finish_reason",s(reason))])]))];
            if let Some(usage) = &self.usage {fields.push(("usage",usage.clone()));}
            self.terminal = Some(obj(fields)); return Ok(());
        }
        let record = parse(data)?;
        if let Ok(error) = record.get("error") {return Err(format!("Server error: {}",error.string()));}
        for choice in record.get("choices")?.array()? {
            if choice.get("index")? != &Num("0".into()) {continue;}
            let delta = choice.get("delta")?;
            if let Some(completed)=&self.finish_reason {
                // Some compatible servers include an empty choice alongside
                // final usage, or repeat the same terminal marker. These carry
                // no new choice data. Never discard new text/structured fields,
                // change the completion reason, or extend past [DONE].
                // A repeat names the same completion reason; only then may it
                // restate the assistant role, as MiniMax's repeated marker does.
                let repeat=choice.get("finish_reason").ok().and_then(|reason|reason.text().ok())==Some(completed.as_str());
                let empty=matches!(delta,Obj(fields) if fields.iter().all(|(key,value)| value==&Null || (key=="content" && value==&Str(String::new())) || (repeat && key=="role" && value==&Str("assistant".into()))));
                if !empty{return Err("Choice data followed its completion".into());}
                if let Ok(reason)=choice.get("finish_reason") {if reason!=&Null && reason.text()?!=completed{return Err("Choice completion reason changed after completion".into());}}
                continue;
            }
            if let Ok(content) = delta.get("content") {if content != &Null {self.append(content.text()?)?;}}
            if delta.get("function_call").is_ok(){return Err("Legacy streamed function_call is not a supported proposal".into());}
            if let Ok(calls)=delta.get("tool_calls"){self.call_fragments.append(calls)?;}
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
        if self.mcp && name=="retry" {
            let value=value.strip_prefix(' ').unwrap_or(value);
            if !value.is_empty() && value.bytes().all(|b|b.is_ascii_digit()){
                self.retry_ms=Some(value.bytes().fold(0u64,|n,b|n.saturating_mul(10).saturating_add((b-b'0') as u64)));
            }
        }
        if self.resumable && name=="id" {
            let value=value.strip_prefix(' ').unwrap_or(value);
            if value.len()>256||value.trim()!=value||!value.bytes().all(|b|(0x20..=0x7e).contains(&b)){return Err("MCP resume cursor requires at most 256 printable ASCII bytes without edge whitespace".into());}
            self.event_id=Some(value.into());
        }
        if name == "data" {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if self.mcp {
                if self.event.len().saturating_add(value.len()).saturating_add(1)>MCP_TEXT_LIMIT {return Err("Stream event exceeds 1 MiB".into());}
            } else {
                let extra = value.len().checked_add(1).ok_or(STREAM_MEMORY)?;
                self.event.try_reserve(extra).map_err(|_| STREAM_MEMORY)?;
            }
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
            if let Ok(calls)=message.get("tool_calls") {if calls!=&Null {for call in calls.array()? {
                if call.string().len()>65536{return Err("Streamed tool proposal exceeds 64 KiB".into());}
                self.native_calls.push(call.clone());
            }}}
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
                else {
                    if self.mcp {
                        if self.line.len()>=MCP_TEXT_LIMIT {return Err("Stream line exceeds 1 MiB".into());}
                    } else {
                        self.line.try_reserve(1).map_err(|_| STREAM_MEMORY)?;
                    }
                    self.line.push(*byte);
                }
            }
            return Ok(());
        }
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            self.line.try_reserve(part.len()).map_err(|_| STREAM_MEMORY)?;
            self.line.extend_from_slice(part);
            if part.last() == Some(&b'\n') { let line = std::mem::take(&mut self.line); self.record(&line)?; }
        }
        Ok(())
    }
    fn feed_trailers(&mut self) -> Result<()> {
        while !self.trailers_complete {
            let pending = &self.raw[self.offset..];
            let Some(end) = pending.windows(2).position(|pair| pair == b"\r\n") else {
                if self.trailer_bytes + pending.len() > network::RESPONSE_TRAILER_LIMIT {
                    return Err("Chunk trailers exceed limit".into());
                }
                return Ok(());
            };
            self.trailer_bytes += end + 2;
            if self.trailer_bytes > network::RESPONSE_TRAILER_LIMIT {
                return Err("Chunk trailers exceed limit".into());
            }
            if end == 0 {
                self.trailers_complete = true;
            } else {
                network::response_trailer(&pending[..end])?;
            }
            self.offset += end + 2;
        }
        if self.offset != self.raw.len() {
            return Err("Bytes after framed response".into());
        }
        Ok(())
    }
    /// The framing a refused head declares, judged with the streaming rules.
    ///
    /// Every head is judged before its status decides the outcome, so a head
    /// the product's own decoder refuses is refused here too and the two
    /// surfaces never disagree about a head they both read.
    fn refused_head_framing<'a>(lines:impl Iterator<Item=&'a str>)->Result<()>{
        let mut transfer=false;let mut coding=false;let mut length=false;
        for line in lines{
            let (name,value)=network::response_field(line)?;
            match name.to_ascii_lowercase().as_str(){
                "transfer-encoding"=>{if transfer{return Err("Duplicate streaming framing header".into());}transfer=true;if !value.eq_ignore_ascii_case("chunked"){return Err("Unsupported streaming transfer encoding".into());}}
                "content-encoding"=>{if coding{return Err("Duplicate streaming framing header".into());}coding=true;if !value.eq_ignore_ascii_case("identity"){return Err("Unsupported streaming content encoding".into());}}
                "content-length"=>{if length{return Err("Duplicate streaming framing header".into());}length=true;network::response_length(value)?;}
                _=>{}
            }
        }
        if transfer&&length{return Err("Ambiguous streaming response framing".into());}
        Ok(())
    }
    /// The MCP client's own wording for a refused exchange.
    ///
    /// A refused MCP head is refused where the head is read, in the MCP
    /// client's own words. The shared decoder's sentence belongs to the web
    /// reader and never reaches an MCP surface.
    fn mcp_refusal(&self,code:u16,head:&str)->String{
        let challenge=super::mcp_lifecycle::mcp_challenge(head);
        let context=self.control.clone().unwrap_or(Null);
        super::mcp_lifecycle::mcp_refusal(super::mcp_lifecycle::mcp_credential_sent(&context),code,challenge.as_deref(),if self.server_stream{"listen"}else{"connect"})
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        if self.mcp {
            if self.raw.len().saturating_add(bytes.len()) > MCP_WIRE_LIMIT { return Err("Stream response exceeds 2 MiB".into()); }
        } else {
            self.raw.try_reserve(bytes.len()).map_err(|_| STREAM_MEMORY)?;
        }
        self.raw.extend_from_slice(bytes);
        while !self.headers {
            let Some(relative_end)=network::response_head_end(&self.raw[self.offset..]).map_err(|_|"HTTP headers exceed limit")? else {return Ok(());};
            let end=self.offset+relative_end;
            let header = std::str::from_utf8(&self.raw[self.offset..end]).map_err(|_| "Invalid response headers")?;
            let mut lines = header.split("\r\n");
            let status = lines.next().ok_or("Missing status")?;
            let code = network::response_status(status)?;
            if (100..200).contains(&code) {
                self.interim+=1;
                if self.interim>4{return Err("Too many informational HTTP responses".into());}
                if code==101 {return Err("Unsupported informational response".into());}
                let mut length_seen = false;
                let mut coding_seen = false;
                for line in lines {
                    let (name, value) = network::response_field(line)?;
                    if name.eq_ignore_ascii_case("transfer-encoding") {
                        return Err("Unsupported informational response".into());
                    }
                    if name.eq_ignore_ascii_case("content-encoding") {
                        // One field per name per head, as the frame session
                        // enforces it. An informational head is framing too, so
                        // a repeated coding header is refused here as well.
                        if coding_seen {
                            return Err("Duplicate streaming framing header".into());
                        }
                        coding_seen = true;
                        if !value.eq_ignore_ascii_case("identity") {
                            return Err("Unsupported streaming content encoding".into());
                        }
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        if length_seen || network::response_length(value)? != 0 {
                            return Err("Unsupported informational response".into());
                        }
                        length_seen = true;
                    }
                }
                self.offset=end+4;
                continue;
            }
            if self.server_stream&&code==405{return Err("This MCP endpoint does not offer an optional server GET stream (HTTP 405); no retry was sent".into());}
            if !(200..300).contains(&code) {
                // A refused head is still a head. Its framing is judged before
                // the status decides the outcome, so the streaming surface
                // never accepts a head the product's own decoder refuses.
                if self.mcp{
                    Self::refused_head_framing(lines)?;
                    return Err(self.mcp_refusal(code,header));
                }
                return Ok(());
            }
            if (self.mcp||self.messages||self.content)&&code!=200{return Err("Streaming result requires HTTP 200 with a response body".into());}
            let body_start = end + 4;
            let mut names=std::collections::BTreeSet::new();
            let mut event_stream=false;
            self.close_delimited=self.mcp&&header.starts_with("HTTP/1.0 ");
            for line in lines {
                let (name, value)=network::response_field(line)?;
                let name=name.to_ascii_lowercase();
                if matches!(name.as_str(),"transfer-encoding"|"content-encoding")&&!names.insert(name.clone()){return Err("Duplicate streaming framing header".into());}
                if name=="transfer-encoding" {
                    if !value.eq_ignore_ascii_case("chunked"){return Err("Unsupported streaming transfer encoding".into());}
                    self.chunked=true;
                }
                if name=="content-encoding"&&!value.eq_ignore_ascii_case("identity"){return Err("Unsupported streaming content encoding".into());}
                if self.mcp||self.messages||self.content {
                    if ["content-type","mcp-session-id"].contains(&name.as_str())&&!names.insert(name.clone()){return Err("Duplicate streaming framing header".into());}
                    if name=="content-type"&&value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/event-stream"){event_stream=true;}
                    if self.mcp&&name=="connection"&&value.split(',').any(|token|token.trim().eq_ignore_ascii_case("close")){self.close_delimited=true;}
                    if name=="mcp-session-id"{if value.len()>256||value.chars().any(char::is_control)||(self.mcp&&(value.is_empty()||!value.bytes().all(|b|(0x21..=0x7e).contains(&b)))){return Err("Invalid MCP session ID".into());}self.mcp_session=value.into();}
                }
                if name.eq_ignore_ascii_case("content-length") {
                    if self.expected.is_some() { return Err("Duplicate streaming framing header".into()); }
                    let length = network::response_length(value)?;
                    self.expected=Some(body_start.checked_add(length).ok_or("Response length overflow")?);
                }
                if name.eq_ignore_ascii_case("content-type") { self.json = value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"); }
            }
            if self.chunked && self.expected.is_some() { return Err("Ambiguous streaming response framing".into()); }
            if code == 204 {
                if self.chunked || self.expected.is_some_and(|expected| expected != body_start) {
                    return Err("Body on bodyless response".into());
                }
                self.bodyless = true;
                self.expected = Some(body_start);
            }
            if self.mcp {
                let context=self.control.as_ref().ok_or("MCP response requires its captured context")?;
                super::mcp_identity::session_binding(context.get("request").and_then(Json::text).unwrap_or(""),field(context,"session")?,if self.mcp_session.is_empty(){None}else{Some(&self.mcp_session)})?;
                self.mcp_event_stream=event_stream;
            }
            if self.server_stream {
                if !event_stream{return Err("MCP server GET requires text/event-stream".into());}
                let configured=self.control.as_ref().ok_or("No MCP server stream binding")?.get("session")?.text()?;
                if !self.mcp_session.is_empty()&&self.mcp_session!=configured{return Err("MCP server GET changed its session".into());}
            }
            self.headers = true; self.offset = body_start;
        }
        if self.bodyless {
            if self.raw.len() > self.offset { return Err("Body on bodyless response".into()); }
            return Ok(());
        }
        if self.expected.is_some_and(|end| self.raw.len() > end) {
            return Err("Streaming response exceeds Content-Length".into());
        }
        loop {
            if !self.chunked {
                let start = self.offset;
                self.offset = self.raw.len();
                self.feed_body(start, self.offset)?;
                break;
            }
            if self.trailers {
                self.feed_trailers()?;
                break;
            }
            if self.remaining == 0 {
                let pending = &self.raw[self.offset..];
                let Some(size_end)=network::chunk_line_end(pending).map_err(|_|CHUNK_HEADER_TOO_LONG)? else {break;};
                let line = &pending[..size_end];
                self.remaining = usize::try_from(network::chunk_size(line)?)
                    .map_err(|_| "Stream chunk length is not representable")?;
                if self.mcp && self.remaining > MCP_WIRE_LIMIT { return Err("Stream chunk exceeds response limit".into()); }
                self.offset += size_end + 2;
                if self.remaining == 0 { self.trailers = true; continue; }
            }
            // A chunk's text or controls can be published as soon as it is decoded.
            let start = self.offset;
            let next = start.checked_add(self.remaining).and_then(|end| end.checked_add(2))
                .ok_or("Stream chunk length overflow")?;
            if self.raw.len() < next { break; }
            let end = next - 2;
            if &self.raw[end..next] != b"\r\n" {
                return Err("Invalid stream chunk terminator".into());
            }
            self.offset = next;
            self.remaining = 0;
            self.feed_body(start, end)?;
        }
        Ok(())
    }
    fn feed_body(&mut self, start: usize, end: usize) -> Result<()> {
        let raw = std::mem::take(&mut self.raw);
        let result = self.body(&raw[start..end]);
        self.raw = raw;
        result
    }
    fn framed(&self) -> bool {
        if !self.headers {return false;}
        if self.mcp&&!self.control_pending.is_empty(){return false;}
        // A correlated SSE reply can end an unframed persistent stream. A
        // declared HTTP body must finish before any terminal result is adopted.
        if self.mcp&&self.terminal.is_some()&&!self.chunked&&self.expected.is_none()&&!self.close_delimited{return true;}
        if self.chunked {self.trailers_complete}
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
                let mut summary=self.server_observation();
                if let Obj(fields)=&mut summary {fields.insert("stream_schema".into(),Num("1".into()));}
                return Ok(summary.string());
            }
            if !self.control_pending.is_empty(){return Err("MCP control response is not acknowledged".into());}
            if self.terminal.is_some()&&(self.chunked||self.expected.is_some()){
                if !self.framed(){return Err("MCP SSE response ended before the declared HTTP body completed".into());}
                network::decoded(&self.raw)?;
            }else if self.terminal.is_some()&&self.close_delimited{
                if !self.observed_eof{return Err("Close-delimited MCP SSE response requires observed EOF".into());}
                network::decoded(&self.raw)?;
            }
            if let Some(result)=self.terminal.take(){return Ok(result.string());}
            return Ok(self.json_terminal()?.string());
        }
        let (_, body) = network::decoded_provider_stream(&self.raw)?;
        self.raw = Vec::new();
        if self.json || !self.headers || (self.chat && self.text.is_empty() && self.terminal.is_none() && body.trim_start().starts_with('{')) {
            // A compatible server may ignore stream=true and return one JSON reply.
            let protocol = if self.content {"content"} else if self.messages {"messages"} else if self.chat {"chat"} else {"local"};
            let mut document = parse_stream_document(&body)?;
            let changed = namespaced_markup(&body) && adopt_namespaced_calls(protocol, &mut document);
            super::provider::stream_reply(protocol, &mut document)?;
            if document.get("done")==Ok(&Bool(false)){return Err("Connection ended before the model completed its reply".into());}
            if changed { return serialize_stream_document(&document); }
            return Ok(body);
        }
        drop(body);
        // SSE discards an event that lacks a final blank line; NDJSON allows
        // a final complete record without a trailing newline.
        if !self.chat {let line = std::mem::take(&mut self.line); self.record(&line)?;}
        if self.content {
            let reason=self.finish_reason.clone().ok_or("Connection ended before the Content candidate completed")?;
            if self.text.trim().is_empty()&&self.native_calls.is_empty()&&!super::provider::limit_reason("content",&reason){return Err("Server returned no text response".into());}
            self.flush_content_text()?;
            let parts = std::mem::take(&mut self.native_calls);
            let mut fields=vec![("candidates",Arr(vec![obj(vec![("index",Num("0".into())),("content",obj(vec![("role",s("model")),("parts",Arr(parts))])),("finishReason",s(&reason))])]))];
            if let Some(usage)=&self.usage {fields.push(("usageMetadata",usage.clone()));}
            let mut built=obj(fields);
            super::provider::stream_reply("content", &mut built)?;
            return serialize_stream_document(&built);
        }
        let mut complete = self.terminal.take().ok_or("Connection ended before the model completed its reply")?;
        let protocol=if self.messages{"messages"}else if self.chat{"chat"}else{"local"};
        // A refused completion never returns this terminal, so the provider stop reason has to ride on the error.
        let reason=if protocol=="chat"{complete.get("choices").ok().and_then(|v|v.array().ok()).and_then(|v|v.first()).and_then(|v|v.get("finish_reason").ok()).and_then(|v|v.text().ok()).unwrap_or("").to_string()}else{complete.get(if protocol=="messages"{"stop_reason"}else{"done_reason"}).ok().and_then(|v|v.text().ok()).unwrap_or("").to_string()};
        if protocol=="local" {if let Obj(fields) = &mut complete {
            let mut message=obj(vec![("role",s("assistant")),("content",Str(std::mem::take(&mut self.text)))]);
            if !self.native_calls.is_empty(){if let Obj(fields)=&mut message{fields.insert("tool_calls".into(),Arr(std::mem::take(&mut self.native_calls)));}}
            fields.insert("message".into(),message);
        }}
        if let Err(error)=super::provider::stream_reply(protocol,&mut complete){
            if reason.is_empty() || error == DOCUMENT_MEMORY || error == STREAM_MEMORY {return Err(error);}
            return Err(format!("{error}: {reason}"));
        }
        serialize_stream_document(&complete)
    }
}

const NAMESPACE_MARK: &str = "]<]minimax[>[";

fn provider_completion_data(header: &str, body: &str) -> Result<String> {
    let mut wire = String::new();
    let length = header.len().checked_add(body.len()).ok_or(DOCUMENT_MEMORY)?;
    wire.try_reserve_exact(length).map_err(|_| DOCUMENT_MEMORY)?;
    wire.push_str(header);
    wire.push_str(body);

    let groups = length / 3 + usize::from(length % 3 != 0);
    let mut data = String::new();
    data.try_reserve_exact(groups.checked_mul(4).ok_or(DOCUMENT_MEMORY)?)
        .map_err(|_| DOCUMENT_MEMORY)?;
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in wire.as_bytes().chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        data.push(char::from(DIGITS[usize::from(first >> 2)]));
        data.push(char::from(DIGITS[usize::from((first & 3) << 4 | second >> 4)]));
        data.push(if chunk.len() >= 2 { char::from(DIGITS[usize::from((second & 15) << 2 | third >> 6)]) } else { '=' });
        data.push(if chunk.len() == 3 { char::from(DIGITS[usize::from(third & 63)]) } else { '=' });
    }
    Ok(data)
}

fn namespaced_markup(text: &str) -> bool {
    text.contains(NAMESPACE_MARK)
        || text.contains("<tool_call>")
        || text.contains("<tool_calls>")
        || text.contains("<invoke")
        || !json_call_spans(text).is_empty()
}

struct Rewrite { changed: bool, inserted: bool }

pub(super) fn adopt_namespaced_calls(protocol: &str, body: &mut Json) -> bool {
    match protocol {
        "chat" => adopt_chat(body),
        "local" => adopt_local(body),
        "messages" => adopt_messages(body),
        "content" => adopt_content(body),
        _ => false,
    }
}

fn adopt_chat(body: &mut Json) -> bool {
    let Obj(fields) = body else { return false };
    let Some(Arr(choices)) = fields.get_mut("choices") else { return false };
    let Some(choice) = choices.iter_mut().find(|choice| match choice.get("index") {
        Ok(value) => value == &Null || value == &Num("0".into()),
        Err(_) => true,
    }) else { return false };
    let Obj(choice_fields) = choice else { return false };
    let reason = choice_fields.get("finish_reason").and_then(|value| value.text().ok()).unwrap_or("").to_string();
    let limited = super::provider::limit_reason("chat", &reason);
    let rewrite = {
        let Some(message) = choice_fields.get_mut("message") else { return false };
        rewrite_message(message, "chat", limited)
    };
    if rewrite.inserted { choice_fields.insert("finish_reason".into(), s("tool_calls")); }
    rewrite.changed || rewrite.inserted
}

fn adopt_local(body: &mut Json) -> bool {
    let Obj(fields) = body else { return false };
    let reason = fields.get("done_reason").and_then(|value| value.text().ok()).unwrap_or("").to_string();
    let done = fields.get("done") == Some(&Bool(true));
    let limited = super::provider::limit_reason("local", &reason) || !done;
    let Some(message) = fields.get_mut("message") else { return false };
    let rewrite = rewrite_message(message, "local", limited);
    rewrite.changed || rewrite.inserted
}

fn adopt_messages(body: &mut Json) -> bool {
    let Obj(fields) = body else { return false };
    let reason = fields.get("stop_reason").and_then(|value| value.text().ok()).unwrap_or("").to_string();
    let limited = super::provider::limit_reason("messages", &reason);
    let mut changed = false;
    let mut inserted = false;
    {
        let Some(Arr(parts)) = fields.get_mut("content") else { return false };
        let mut index = 0;
        for mut part in std::mem::take(parts) {
            let calls = if part.get("type").and_then(Json::text) == Ok("text") {
                rewrite_text_part(&mut part, &mut changed)
            } else { Vec::new() };
            parts.push(part);
            for call in calls.iter().filter(|_| !limited) {
                if let (Ok(name), Ok(args)) = (call.get("tool").and_then(|value| value.text()), call.get("args")) {
                    parts.push(obj(vec![("type", s("tool_use")), ("id", s(&format!("namespaced-{index}"))), ("name", s(name)), ("input", args.clone())]));
                    index += 1;
                    inserted = true;
                }
            }
        }
    }
    if inserted { fields.insert("stop_reason".into(), s("tool_use")); }
    changed || inserted
}

fn adopt_content(body: &mut Json) -> bool {
    let Obj(fields) = body else { return false };
    let Some(Arr(candidates)) = fields.get_mut("candidates") else { return false };
    let Some(candidate) = candidates.iter_mut().find(|candidate| match candidate.get("index") {
        Ok(Num(value)) => value == "0",
        Ok(Null) | Err(_) => true,
        _ => false,
    }) else { return false };
    let Obj(candidate_fields) = candidate else { return false };
    let reason = candidate_fields.get("finishReason").and_then(|value| value.text().ok()).unwrap_or("").to_string();
    let limited = super::provider::limit_reason("content", &reason);
    let mut changed = false;
    let mut inserted = false;
    {
        let Some(Obj(content_fields)) = candidate_fields.get_mut("content") else { return false };
        let Some(Arr(parts)) = content_fields.get_mut("parts") else { return false };
        for mut part in std::mem::take(parts) {
            let calls = if part.get("functionCall").is_err() && part.get("thought") != Ok(&Bool(true)) {
                rewrite_text_part(&mut part, &mut changed)
            } else { Vec::new() };
            parts.push(part);
            for call in calls.iter().filter(|_| !limited) {
                if let (Ok(name), Ok(args)) = (call.get("tool").and_then(|value| value.text()), call.get("args")) {
                    parts.push(obj(vec![("functionCall", obj(vec![("name", s(name)), ("args", args.clone())]))]));
                    inserted = true;
                }
            }
        }
    }
    if inserted { candidate_fields.insert("finishReason".into(), s("STOP")); }
    changed || inserted
}

fn rewrite_text_part(part: &mut Json, changed: &mut bool) -> Vec<Json> {
    let Ok(original) = part.get("text").and_then(Json::text) else { return Vec::new() };
    if !namespaced_markup(original) { return Vec::new(); }
    let (clean, calls) = namespaced_tool_calls(original);
    if clean != original {
        if let Obj(fields) = part { fields.insert("text".into(), s(&clean)); }
        *changed = true;
    }
    calls
}

fn rewrite_message(message: &mut Json, protocol: &str, limited: bool) -> Rewrite {
    let Obj(fields) = message else { return Rewrite { changed: false, inserted: false } };
    let Some(original) = fields.get("content").and_then(|value| value.text().ok()) else { return Rewrite { changed: false, inserted: false } };
    if !namespaced_markup(&original) { return Rewrite { changed: false, inserted: false } };
    let (clean, calls) = namespaced_tool_calls(&original);
    let changed = clean != original;
    if changed { fields.insert("content".into(), Str(clean)); }
    if !limited && !calls.is_empty() {
        let mut wires = match fields.get("tool_calls") {
            Some(Arr(items)) => items.clone(),
            _ => Vec::new(),
        };
        for call in &calls {
            if let Some(wire) = wire_call(protocol, call, wires.len()) {
                wires.push(wire);
            }
        }
        if !wires.is_empty() {
            fields.insert("tool_calls".into(), Arr(wires));
            return Rewrite { changed: true, inserted: true };
        }
    }
    Rewrite { changed, inserted: false }
}

fn wire_call(protocol: &str, call: &Json, index: usize) -> Option<Json> {
    let name = call.get("tool").ok()?.text().ok()?;
    let args = call.get("args").ok()?;
    if !matches!(args, Obj(_)) { return None; }
    let function = match protocol {
        "chat" => obj(vec![("name", s(name)), ("arguments", s(&args.string()))]),
        "local" => obj(vec![("name", s(name)), ("arguments", args.clone())]),
        _ => return None,
    };
    Some(obj(vec![("id", s(&format!("namespaced-{index}"))), ("type", s("function")), ("function", function)]))
}

pub(super) fn namespaced_tool_calls(text: &str) -> (String, Vec<Json>) {
    if !namespaced_markup(text) { return (text.to_string(), Vec::new()); }
    let mut ordered: Vec<(usize, usize, Json)> = Vec::new();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while let Some((start, open_end)) = find_wrapped(text, cursor, "tool_call", false) {
        if let Some((close_start, close_end)) = find_wrapped(text, open_end, "tool_call", true) {
            let inner = &text[open_end..close_start];
            let invokes = invoke_spans(inner);
            let objects = json_call_spans(inner);
            for (call, from, to) in invokes.into_iter().chain(objects) {
                ordered.push((open_end + from, open_end + to, call));
            }
            ranges.push((start, close_end));
            cursor = close_end;
        } else {
            ranges.push((start, open_end));
            cursor = open_end;
        }
        if cursor <= start { break; }
    }
    for (call, start, end) in json_call_spans(text) {
        if range_covers(&ranges, start) { continue; }
        ordered.push((start, end, call));
        ranges.push((start, end));
    }
    for (call, start, end) in invoke_spans(text) {
        if range_covers(&ranges, start) { continue; }
        ordered.push((start, end, call));
        ranges.push((start, end));
    }
    ordered.sort_by_key(|(start, end, _)| (*start, *end));
    let mut calls = Vec::new();
    for (start, end, call) in ordered {
        push_call(&mut calls, call);
        if calls.len() >= 32 { break; }
        ranges.push((start, end));
    }
    for needle in [NAMESPACE_MARK, "<tool_call>", "</tool_call>", "<tool_calls>", "</tool_calls>"] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(needle) {
            let start = from + rel;
            ranges.push((start, start + needle.len()));
            from = start + needle.len();
        }
    }
    (excise(text, &ranges).trim().to_string(), calls)
}

fn push_call(calls: &mut Vec<Json>, call: Json) {
    if calls.len() >= 32 { return; }
    calls.push(call);
}

fn range_covers(ranges: &[(usize, usize)], index: usize) -> bool {
    ranges.iter().any(|(start, end)| index >= *start && index < *end)
}

fn invoke_calls(text: &str) -> Vec<Json> { invoke_spans(text).into_iter().map(|(call, _, _)| call).collect() }
fn json_calls(text: &str) -> Vec<Json> { json_call_spans(text).into_iter().map(|(call, _, _)| call).collect() }

fn invoke_spans(text: &str) -> Vec<(Json, usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(at) = find_invoke(text, cursor) {
        if let Some((name, value, end)) = take_element(text, at, 0) {
            if name == "invoke" {
                if let Some(tool) = open_attribute(text, at, "name") {
                    if let Some(call) = invoke_call(&tool, &value) { out.push((call, at, end)); }
                }
            }
            cursor = end.max(at + 1);
        } else {
            cursor = at + "<invoke".len();
        }
        if cursor > text.len() { break; }
    }
    out
}

fn json_call_spans(text: &str) -> Vec<(Json, usize, usize)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < text.len() {
        if bytes[index] == b'{' {
            if let Some((end, value)) = balanced_object(text, index) {
                if let Some(call) = tool_call_value(&value) {
                    out.push((call, index, end));
                    index = end;
                    continue;
                }
            }
        }
        index += text[index..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

fn tool_call_value(value: &Json) -> Option<Json> {
    let name = value.get("tool").ok()?.text().ok()?;
    let args = value.get("args").ok()?;
    super::provider_calls::canonical(name, args).ok()
}

fn invoke_call(name: &str, value: &Json) -> Option<Json> {
    let args = match value {
        Obj(fields) => Obj(fields.clone()),
        Str(text) if text.is_empty() => obj(vec![]),
        _ => return None,
    };
    super::provider_calls::canonical(name, &args).ok()
}

fn balanced_object(text: &str, start: usize) -> Option<(usize, Json)> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'{') { return None; }
    let mut index = start;
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    while index < bytes.len() && index - start <= 65536 {
        let byte = bytes[index];
        if quoted {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { quoted = false; }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = index + 1;
                        return parse(&text[start..end]).ok().map(|value| (end, value));
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn find_invoke(text: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(rel) = text[cursor..].find("<invoke") {
        let at = cursor + rel;
        let after = at + "<invoke".len();
        if tag_boundary(text, after) {
            return Some(mark_start(text, at));
        }
        cursor = after;
    }
    None
}

fn find_wrapped(text: &str, from: usize, name: &str, closing: bool) -> Option<(usize, usize)> {
    let needle = if closing { format!("</{name}") } else { format!("<{name}") };
    let mut cursor = from;
    while cursor < text.len() {
        let Some(rel) = text[cursor..].find(&needle) else { return None };
        let at = cursor + rel;
        let after = at + needle.len();
        if tag_boundary(text, after) {
            if let Some(gt) = text[after..].find('>') {
                return Some((mark_start(text, at), after + gt + 1));
            }
        }
        cursor = after;
    }
    None
}

fn mark_start(text: &str, at: usize) -> usize {
    if at >= NAMESPACE_MARK.len() && text.is_char_boundary(at - NAMESPACE_MARK.len()) && &text[at - NAMESPACE_MARK.len()..at] == NAMESPACE_MARK {
        at - NAMESPACE_MARK.len()
    } else { at }
}

fn tag_boundary(text: &str, pos: usize) -> bool {
    if pos > text.len() || !text.is_char_boundary(pos) { return false; }
    match text[pos..].chars().next() {
        Some(c) if c == '>' || c == '/' || c.is_whitespace() => true,
        _ => false,
    }
}

fn take_element(text: &str, pos: usize, depth: usize) -> Option<(String, Json, usize)> {
    if depth > 16 || pos >= text.len() { return None; }
    let lt = if text[pos..].starts_with(NAMESPACE_MARK) {
        let next = pos + NAMESPACE_MARK.len();
        if !text[next..].starts_with('<') || text[next..].starts_with("</") { return None; }
        next
    } else if text[pos..].starts_with('<') && !text[pos..].starts_with("</") { pos } else { return None; };
    let name_at = lt + 1;
    let name_len = text[name_at..].find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(text.len() - name_at);
    if name_len == 0 || name_len > 64 { return None; }
    let name = text[name_at..name_at + name_len].to_string();
    let after = name_at + name_len;
    if !tag_boundary(text, after) { return None; }
    let gt = text[after..].find('>')?;
    let open_end = after + gt + 1;
    if text[after..open_end].ends_with("/>") { return Some((name, scalar(""), open_end)); }
    let mut level = 1usize;
    let mut cursor = open_end;
    while level > 0 {
        let next_open = find_wrapped(text, cursor, &name, false);
        let next_close = find_wrapped(text, cursor, &name, true);
        match (next_open, next_close) {
            (_, None) => return None,
            (Some((open_at, open_to)), Some((close_at, _))) if open_at < close_at => { level += 1; cursor = open_to; }
            (_, Some((close_at, close_to))) => {
                level -= 1;
                if level == 0 { return Some((name, value_of(&text[open_end..close_at], depth + 1), close_to)); }
                cursor = close_to;
            }
        }
    }
    None
}

fn value_of(inner: &str, depth: usize) -> Json {
    if let Some(items) = parse_sequence(inner, depth) {
        if items.iter().all(|(name, _)| name == "item") { return Arr(items.into_iter().map(|(_, value)| value).collect()); }
        let mut fields = BTreeMap::new();
        for (name, value) in items { fields.entry(name).or_insert(value); }
        return Obj(fields);
    }
    scalar(inner)
}

fn parse_sequence(text: &str, depth: usize) -> Option<Vec<(String, Json)>> {
    let mut pos = 0;
    let mut items = Vec::new();
    let bytes = text.as_bytes();
    while pos < text.len() {
        while pos < text.len() && bytes[pos].is_ascii_whitespace() { pos += 1; }
        if pos >= text.len() { break; }
        let (name, value, next) = take_element(text, pos, depth)?;
        if next <= pos { return None; }
        items.push((name, value));
        pos = next;
    }
    if items.is_empty() { None } else { Some(items) }
}

fn open_attribute(text: &str, pos: usize, key: &str) -> Option<String> {
    let lt = if text[pos..].starts_with(NAMESPACE_MARK) { pos + NAMESPACE_MARK.len() } else { pos };
    let gt = text[lt..].find('>')?;
    attribute(&text[lt..lt + gt + 1], key)
}

fn attribute(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(at) = rest.find(key) {
        let before = at == 0 || rest[..at].chars().next_back().is_some_and(|c| c.is_whitespace() || c == '<');
        rest = &rest[at + key.len()..];
        if !before { continue; }
        let rest_trim = rest.trim_start();
        if !rest_trim.starts_with('=') { continue; }
        let rest_trim = rest_trim[1..].trim_start();
        let quote = rest_trim.chars().next()?;
        if quote != '"' && quote != '\'' { return None; }
        let rest_trim = &rest_trim[quote.len_utf8()..];
        let end = rest_trim.find(quote)?;
        return Some(rest_trim[..end].to_string());
    }
    None
}

fn scalar(text: &str) -> Json {
    let text = xml_text(text);
    match text.as_str() {
        "true" => Bool(true),
        "false" => Bool(false),
        _ if plain_number(&text) => Num(text),
        _ => s(&text),
    }
}

fn xml_text(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let token = &rest[at..];
        let Some(end) = token.find(';') else { out.push_str(token); return out.trim().to_string() };
        match &token[..=end] {
            "&amp;" => out.push('&'),
            "&lt;" => out.push('<'),
            "&gt;" => out.push('>'),
            "&quot;" => out.push('"'),
            "&apos;" => out.push('\''),
            _ => { out.push('&'); rest = &rest[at + 1..]; continue; }
        }
        rest = &token[end + 1..];
    }
    out.push_str(rest);
    out.trim().to_string()
}

pub(super) fn plain_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() { return false; }
    let mut index = 0;
    if bytes[0] == b'-' {
        if bytes.len() == 1 { return false; }
        index = 1;
    }
    let digits = index;
    if bytes[index] == b'0' { index += 1; }
    else if bytes[index].is_ascii_digit() { while index < bytes.len() && bytes[index].is_ascii_digit() { index += 1; } }
    else { return false; }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let fraction = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() { index += 1; }
        if index == fraction { return false; }
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') { index += 1; }
        let exponent = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() { index += 1; }
        if index == exponent { return false; }
    }
    index == bytes.len() && !(bytes[digits] == b'0' && index > digits + 1 && bytes[digits + 1].is_ascii_digit())
}

fn excise(text: &str, ranges: &[(usize, usize)]) -> String {
    let mut spans: Vec<(usize, usize)> = ranges.iter().copied().filter(|(start, end)| *start < *end && *end <= text.len() && text.is_char_boundary(*start) && text.is_char_boundary(*end)).collect();
    spans.sort_by_key(|span| span.0);
    let mut merged = Vec::new();
    for span in spans {
        if let Some((_, end)) = merged.last_mut() {
            if span.0 <= *end { *end = (*end).max(span.1); continue; }
        }
        merged.push(span);
    }
    let mut out = String::new();
    let mut cursor = 0;
    for (start, end) in merged {
        out.push_str(&text[cursor..start]);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn content_part(part:&Json)->Result<Option<&str>> {
    let Obj(_) = part else{return Err("Invalid Content part".into());};
    for key in ["functionResponse","executableCode","codeExecutionResult","inlineData","fileData"] {
        if part.get(key).is_ok(){return Err("Structured Content tools and media are not supported here; no tool was executed".into());}
    }
    let thought=match part.get("thought") {Ok(Bool(v))=>*v,Err(_)=>false,_=>return Err("Invalid Content thought marker".into())};
    if thought {if part.get("functionCall").is_ok(){return Err("A thought part cannot supply a tool proposal".into());}return Ok(None);}
    if let Ok(call)=part.get("functionCall") {super::provider_calls::canonical(field(call,"name")?,call.get("args")?)?;return Ok(None);}
    match part.get("text") {Ok(t)=>Ok(Some(t.text()?)),Err(_)=>Ok(None)}
}
pub(super) fn content_text(body:&Json)->Result<String> {
    let mut transfer=Transfer{content:true,..Transfer::default()};
    transfer.content_event(body)?;
    // Preserve complete-JSON part separators from the existing text adapter.
    // Streamed replies are normalized to one aggregate part at completion.
    for candidate in body.get("candidates")?.array()? {
        if candidate.get("index").unwrap_or(&Num("0".into()))!=&Num("0".into()) {continue;}
        let mut joined=String::new();
        let mut first=true;
        let content=match candidate.get("content"){Ok(content)=>content,Err(_)=>return Ok(String::new())};
        for part in content.get("parts")?.array()? {
            if let Some(text)=content_part(part)? {
                if !first {super::provider::append_document_text(&mut joined,"\n")?;}
                super::provider::append_document_text(&mut joined,text)?;
                first=false;
            }
        }
        return Ok(joined);
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
pub(super) fn json_batch_media(mut raw:&[u8],body_len:usize)->Result<()>{
    if raw.len()>MCP_WIRE_LIMIT||body_len>MCP_TEXT_LIMIT{return Err("MCP JSON batch exceeds bounded response bytes".into());}
    for _ in 0..=4 {
        let end=raw.windows(4).position(|v|v==b"\r\n\r\n").ok_or("Incomplete MCP JSON batch headers")?;if end>65536{return Err("MCP JSON batch headers exceed limit".into());}
        let header=std::str::from_utf8(&raw[..end]).map_err(|_|"Invalid MCP JSON batch header text")?;let mut lines=header.split("\r\n");let mut status=lines.next().unwrap_or("").split_whitespace();let version=status.next().unwrap_or("");let code=status.next().unwrap_or("");
        if !["HTTP/1.0","HTTP/1.1"].contains(&version)||code.len()!=3||!code.bytes().all(|b|b.is_ascii_digit()){return Err("Invalid MCP JSON batch status".into());}
        let code=code.parse::<u16>().map_err(|_|"Invalid MCP JSON batch status")?;if (100..200).contains(&code){raw=&raw[end+4..];continue;}if code!=200{return Err("MCP JSON batch requires HTTP 200".into());}
        let mut media=None;for line in lines{let(key,value)=line.split_once(':').ok_or("Invalid MCP JSON batch header")?;if key.eq_ignore_ascii_case("content-type"){if media.is_some(){return Err("Duplicate MCP JSON batch media type".into());}media=Some(value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"));}}
        return if media==Some(true){Ok(())}else{Err("MCP JSON response batch requires application/json".into())};
    }Err("Too many informational MCP JSON batch responses".into())
}
struct Lease { token:u64, context:Json, cursor:String, until:std::time::Instant, resumes:usize, retry_ms:Option<u64>, resume_after:std::time::Instant, reserved:Option<String> }
struct Sessions { next: u64, entries: BTreeMap<u64, Transfer>, openings:BTreeMap<u64,Json>, lease:Option<Lease> }
fn summary_wire(t:&Transfer,token:Option<u64>,paused:bool)->Json{
 let mut summary=t.server_observation();
 if let Obj(fields)=&mut summary {fields.insert("stream_schema".into(),Num("1".into()));fields.insert("paused".into(),Bool(paused));}
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
 let t=sessions.entries.remove(&id).unwrap();let resume_after=std::time::Instant::now().checked_add(std::time::Duration::from_millis(t.retry_ms.unwrap_or(0))).unwrap_or(until);sessions.lease=Some(Lease{token,context:t.control.clone().unwrap(),cursor:t.cursor.clone(),until,resumes:t.resumes,retry_ms:t.retry_ms,resume_after,reserved:None});Ok(summary_wire(&t,Some(token),paused))
}

static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
pub fn operation(v: &Json) -> Result<Json> {
    let mut sessions = SESSIONS.get_or_init(||Mutex::new(Sessions{next:1,entries:BTreeMap::new(),openings:BTreeMap::new(),lease:None})).lock().map_err(|_| "Stream state unavailable")?;
    let action = field(v,"op")?;
    if action=="stream_prepare" {
        let context=v.get("context")?;let owner=field(context,"owner")?;let endpoint=field(context,"endpoint")?;let key=field(context,"key")?;let session=field(context,"session")?;
        network::url(endpoint,endpoint.starts_with("http://"))?;
        if endpoint.len()>8192||key.len()>8192||owner.is_empty()||owner.len()>256||session.len()>256||!session.bytes().all(|b|(0x21..=0x7e).contains(&b))||!super::mcp_schema::VERSIONS.contains(&field(context,"version")?)||field(context,"request")?!="GET"||context.get("resumable")!=Ok(&Bool(true)){return Err("Invalid resumable MCP GET admission".into());}
        super::mcp_identity::captured(context)?;
        if sessions.openings.len()>=4||sessions.entries.len()+sessions.openings.len()>=4{return Err("MCP GET opening capacity is full".into());}
        if sessions.openings.values().any(|c|c.get("owner")==context.get("owner"))||sessions.entries.values().any(|t|t.control.as_ref().and_then(|c|c.get("owner").ok())==context.get("owner").ok()){return Err("MCP GET owner already has an opening".into());}
        let token=sessions.next;sessions.next=token.checked_add(1).ok_or("MCP admission ticket exhausted")?;
        let context=obj(vec![("endpoint",s(endpoint)),("key",s(key)),("owner",s(owner)),("version",s(field(context,"version")?)),("session",s(session)),("request",s("GET")),("resumable",Bool(true)),("tools_list_changed",Bool(context.get("tools_list_changed")==Ok(&Bool(true)))),("admission_token",Num(token.to_string())),("request_binding",context.get("request_binding")?.clone())]);sessions.openings.insert(token,context.clone());return Ok(context);
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
        if std::time::Instant::now()<lease.resume_after{return Err(format!("MCP stream resume is available after {} milliseconds",lease.resume_after.saturating_duration_since(std::time::Instant::now()).as_millis()));}
        let reservation_token=if action=="stream_resume"{sessions.next}else{lease.token};
        let mut context=lease.context.clone();if let Obj(f)=&mut context{f.insert("owner".into(),v.get("owner")?.clone());f.insert("resumable".into(),Bool(true));f.insert("resume_token".into(),Num(reservation_token.to_string()));}
        let endpoint=field(config,"endpoint")?;let key=field(config,"key")?;
        let mut headers=vec![("Accept","text/event-stream".into()),("MCP-Protocol-Version",field(&context,"version")?.into()),("Last-Event-ID",lease.cursor.clone())];
        if let Some(token)=network::mcp_bearer_token(&key)?{headers.push(network::mcp_authorization_header(&token)?);}let session=field(&context,"session")?;if !session.is_empty(){headers.push(("Mcp-Session-Id",session.into()));}
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
            if endpoint.len()>8192||field(context,"key")?.len()>8192||field(context,"owner")?.is_empty()||field(context,"owner")?.len()>256||!super::mcp_schema::VERSIONS.contains(&field(context,"version")?){return Err("Invalid MCP control binding".into());}
            if let Ok(request)=context.get("request"){if !["initialize","tools/list","tools/call","GET"].contains(&request.text()?){return Err("Invalid MCP lifecycle request binding".into());}}
            let session=field(context,"session")?;if session.len()>256||!session.bytes().all(|b|(0x21..=0x7e).contains(&b)){return Err("Invalid MCP control session".into());}
            let expected=super::mcp_identity::captured(context)?;
            let mut bound=obj(vec![("endpoint",s(endpoint)),("key",s(field(context,"key")?)),("owner",s(field(context,"owner")?)),("version",s(field(context,"version")?)),("session",s(session)),("request",s(context.get("request").and_then(Json::text).unwrap_or(""))),("resumable",Bool(context.get("resumable")==Ok(&Bool(true)))),("tools_list_changed",Bool(context.get("tools_list_changed")==Ok(&Bool(true)))),("request_binding",context.get("request_binding")?.clone())]);
            if context.get("request_binding")==Ok(&Num("2".into())){if let Some(id)=expected{if let Obj(fields)=&mut bound{fields.insert("request_id".into(),id);}}}
            Some(bound)},Err(_)=>return Err("MCP stream requires a request identity binding".into())}}else{None};
        if protocol=="mcp"&&control.as_ref().and_then(|c|c.get("request").ok()).and_then(|m|m.text().ok())==Some("GET"){return Err("MCP GET requires the standalone server stream mode".into());}
        if protocol=="mcp_server"&&control.as_ref().and_then(|c|c.get("request").ok()).and_then(|m|m.text().ok())!=Some("GET"){return Err("MCP server stream requires a GET lifecycle binding".into());}
        let resumable=protocol=="mcp_server"&&v.get("mcp_control").and_then(|c|c.get("resumable"))==Ok(&Bool(true));
        let mut retry_ms=None;let mut cursor=String::new();let mut resume_until=if resumable{Some(std::time::Instant::now()+std::time::Duration::from_secs(600))}else{None};let mut resumes=0;
        if let Ok(token)=v.get("mcp_control").and_then(|c|c.get("resume_token")){
            let lease=sessions.lease.as_ref().ok_or("MCP resume lease was consumed")?;
            if token!=&Num(lease.token.to_string())||std::time::Instant::now()>=lease.until{return Err("MCP resume lease is no longer current".into());}
            if lease.reserved.as_deref()!=control.as_ref().and_then(|c|c.get("owner").ok()).and_then(|v|v.text().ok()){return Err("MCP resume owner is not reserved".into());}
            for key in ["endpoint","key","version","session"]{if control.as_ref().and_then(|c|c.get(key).ok())!=lease.context.get(key).ok(){return Err("MCP resume binding changed".into());}}
            let announced=|c:&Json| c.get("tools_list_changed")==Ok(&Bool(true));
            if control.as_ref().is_some_and(announced)!=announced(&lease.context){return Err("MCP resume capability changed".into());}
            let lease=sessions.lease.take().unwrap();cursor=lease.cursor;resume_until=Some(lease.until);resumes=lease.resumes+1;retry_ms=lease.retry_ms;
        }
        if resumable&&v.get("mcp_control").and_then(|c|c.get("resume_token")).is_err(){
            let context=v.get("mcp_control")?;let token=match context.get("admission_token")?{Num(n)=>n.parse::<u64>().map_err(|_|"Invalid MCP opening ticket")?,_=>return Err("Invalid MCP opening ticket".into())};
            if sessions.openings.get(&token)!=Some(context){return Err("MCP GET opening was cancelled, consumed or changed".into());}sessions.openings.remove(&token);
        }
        sessions.entries.insert(id,Transfer{resumable,cursor,resume_until,resumes,retry_ms,control,server_stream:protocol=="mcp_server",chat:protocol!="local",mcp:protocol=="mcp"||protocol=="mcp_server",messages:protocol=="messages",content:protocol=="content",..Transfer::default()}); return Ok(Num(id.to_string()));
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
            let mut frame = network::ResponseFrame::default();
            let framing = frame.feed(&bytes);
            if action == "stream_control_probe" {
                let ready = framing.is_ok() && frame.done && network::decoded(&bytes).is_ok();
                return Ok(obj(vec![("ready", Bool(ready))]));
            }
            framing?;
            frame.finish()?;
            if !frame.done && v.get("eof") != Ok(&Bool(true)) {
                return Err("Close-delimited MCP acknowledgment requires observed EOF".into());
            }
            let(status,body)=network::decoded(&bytes)?;if status!=202||!body.is_empty(){return Err("MCP control response was not accepted with empty HTTP 202".into());}
            transfer.control_pending.remove(&ticket);transfer.cursor_commit();return Ok(obj(vec![("complete",Bool(transfer.framed()))]));
        }
        if action == "stream_finish" {
            transfer.observed_eof=v.get("eof")==Ok(&Bool(true));
            if transfer.server_stream&&!transfer.framed()&&v.get("eof")!=Ok(&Bool(true)){return Err("Close-delimited MCP server stream requires observed EOF".into());}
            let body = transfer.finish()?;
            let session=if transfer.mcp_session.is_empty(){String::new()}else{format!("Mcp-Session-Id: {}\r\n",transfer.mcp_session)};
            let header = format!("HTTP/1.1 200 OK\r\n{session}Content-Length: {}\r\n\r\n",body.len());
            let data = if transfer.mcp {
                let wire = header + &body;
                network::b64(wire.as_bytes())
            } else {
                provider_completion_data(&header, &body)?
            };
            let mut completion=vec![("data",Str(data))];
            if !transfer.server_stream && transfer.tool_changes>0 {
                completion.push(("mcp_catalog_changed",Bool(true)));
            }
            return Ok(obj(completion));
        }
        let previous = transfer.text.len();
        let prior_observation = transfer.server_observation();
        let bytes=network::unbase64(field(v,"data")?)?;
        if let Err(error)=transfer.feed(&bytes) {
            // Preserve text already decoded in this same read before an error.
            // The native consumer publishes it as interrupted, never completed.
            if transfer.mcp {
                let cleanup=transfer.control.as_ref().and_then(|context|{super::mcp_lifecycle::plan(context,transfer.lifecycle_session().ok()?,false).ok()}).unwrap_or(Arr(vec![]));
                let observation = if transfer.trailers || error == CHUNK_HEADER_TOO_LONG {
                    prior_observation
                } else {
                    transfer.server_observation()
                };
                return Ok(obj(vec![("delta",s("")),("complete",Bool(false)),("error",s(&error)),("cleanup",cleanup),("observation",observation)]));
            }
            return Ok(obj(vec![("delta",s(&transfer.text[previous..])),("complete",Bool(false)),("error",s(&error))]));
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
