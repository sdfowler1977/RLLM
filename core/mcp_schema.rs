use super::{Json, Json::*, Result};

pub(super) const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];
pub(super) const PREFERRED: &str = "2025-11-25";
fn modern(version: &str) -> bool { matches!(version, "2025-06-18" | "2025-11-25") }

const CATALOG_LIMIT: usize = 65536;

// Android persistence quotes slashes and has short backspace/form-feed escapes.
// Non-integral numbers reserve space for platform number normalization.
pub(super) fn text_cost(c: char) -> usize {
    if matches!(c,'"'|'\\'|'/'|'\n'|'\r'|'\t'|'\u{8}'|'\u{c}') {2}
    else if c<' ' {6} else {c.len_utf8()}
}
fn quoted_bytes(value: &str) -> usize {value.chars().fold(2usize,|n,c|n.saturating_add(text_cost(c)))}
pub(super) fn state_bytes(value: &Json) -> usize {
    match value {
        Null=>4, Bool(v)=>if *v {4}else{5}, Str(v)=>quoted_bytes(v),
        Num(v)=>if !v.contains(['.','e','E'])&&v.parse::<i64>().is_ok(){v.len()}else{v.len().saturating_add(2).max(32)},
        Arr(items)=>items.iter().fold(2usize.saturating_add(items.len().saturating_sub(1)),|n,v|n.saturating_add(state_bytes(v))),
        Obj(items)=>items.iter().fold(2usize.saturating_add(items.len().saturating_sub(1)),|n,(k,v)|n.saturating_add(quoted_bytes(k)).saturating_add(1).saturating_add(state_bytes(v)))
    }
}

// The renderer owns one bounded string; untrusted schemas never fetch references.
struct CatalogText(String);
impl CatalogText {
    fn push(&mut self, value: &str) -> Result<()> {
        if value.len() > CATALOG_LIMIT.saturating_sub(self.0.len()) {
            return Err("MCP tools exceed 64 KiB display limit".into());
        }
        self.0.push_str(value); Ok(())
    }
    fn quoted(&mut self, value: &str) -> Result<()> {
        self.push("\"")?;
        for c in value.chars() {
            match c {
                '"' => self.push("\\\"")?, '\\' => self.push("\\\\")?,
                '\n' => self.push("\\n")?, '\r' => self.push("\\r")?, '\t' => self.push("\\t")?,
                c if c < ' ' => self.push(&format!("\\u{:04x}", c as u32))?,
                c => { let mut bytes=[0;4]; self.push(c.encode_utf8(&mut bytes))?; }
            }
        }
        self.push("\"")
    }
    fn json(&mut self, value: &Json, depth: usize) -> Result<()> {
        if depth > 64 { return Err("MCP schema nesting exceeds limit".into()); }
        match value {
            Null => self.push("null"), Bool(v) => self.push(if *v {"true"} else {"false"}),
            Num(v) => self.push(v), Str(v) => self.quoted(v),
            Arr(items) => {
                self.push("[")?;
                for (i,item) in items.iter().enumerate() { if i!=0 {self.push(",")?;} self.json(item,depth+1)?; }
                self.push("]")
            }
            Obj(items) => {
                self.push("{")?;
                for (i,(key,item)) in items.iter().enumerate() {
                    if i!=0 {self.push(",")?;} self.quoted(key)?; self.push(":")?; self.json(item,depth+1)?;
                }
                self.push("}")
            }
        }
    }
}

fn shape(value: &Json) -> Result<()> {
    let Obj(fields)=value else {return Err("MCP tool schema must be an object".into());};
    if fields.get("type")!=Some(&Str("object".into())) {return Err("MCP tool schema type must be object".into());}
    if let Some(properties)=fields.get("properties") {
        let Obj(properties)=properties else {return Err("MCP schema properties must be an object".into());};
        if properties.values().any(|v|!matches!(v,Obj(_))) {return Err("MCP schema property definitions must be objects".into());}
    }
    if let Some(required)=fields.get("required") {
        let Arr(required)=required else {return Err("MCP schema required must be a string array".into());};
        if required.iter().any(|v|!matches!(v,Str(_))) {return Err("MCP schema required entries must be strings".into());}
    }
    Ok(())
}

pub(super) struct Catalog { pub text: String, pub cursors: Vec<Json>, pub next: Option<String> }

// Parsed JSON number spelling is retained. Compare the closed unit interval
// without floating-point underflow/rounding or expanding exponent powers.
fn exponent_value(exponent:&str)->i64 {
    let negative=exponent.starts_with('-');
    let value=exponent.trim_start_matches(['+','-']).bytes().fold(0i64,|n,b|n.saturating_mul(10).saturating_add((b-b'0')as i64));
    if negative {value.saturating_neg()}else{value}
}
pub(super) fn integer_number(number:&str)->bool {
    let unsigned=number.strip_prefix('-').unwrap_or(number);
    let (mantissa,exponent)=unsigned.split_once(['e','E']).unwrap_or((unsigned,"0"));
    if mantissa.bytes().all(|b|b==b'.'||b==b'0') {return true;}
    let fraction=mantissa.split_once('.').map(|(_,f)|f.len()).unwrap_or(0);
    let trailing=mantissa.bytes().rev().filter(|b|*b!=b'.').take_while(|b|*b==b'0').count();
    exponent_value(exponent).saturating_add(trailing as i64).saturating_sub(fraction as i64)>=0
}
// Explicit version-one low-level bindings retain numeric ID one. Accept
// equivalent bounded spellings; current product requests use string IDs.
pub(super) fn reply_id(value:&Json)->bool {
    matches!(value,Num(number) if number.len()<=64 && integer_number(number) && unit_interval(number) && number.split(['e','E']).next().unwrap_or("").bytes().any(|b|matches!(b,b'1'..=b'9')))
}
// Common payload shape only. Callers retain their existing direction, ID,
// negotiated phase, framing and correlation rules; no payload is executed.
pub(super) fn response(value:&Json)->Result<()> {
    if !matches!(value,Obj(_))||value.get("jsonrpc")!=Ok(&Str("2.0".into()))||value.get("method").is_ok() {return Err("Invalid MCP response direction or version".into());}
    let result=value.get("result");let error=value.get("error");
    if result.is_ok()==error.is_ok() {return Err("MCP response requires exactly one result or error".into());}
    if let Ok(result)=result {
        if !matches!(result,Obj(_)) {return Err("MCP response result must be an object".into());}
        meta(result)?;
    }
    if let Ok(error)=error {
        if !matches!(error,Obj(_)) {return Err("MCP error must be an object".into());}
        if !matches!(error.get("code"),Ok(Num(code)) if integer_number(code)) {return Err("MCP error code must be an integer number".into());}
        text_field(error,"message",true)?;
    }
    Ok(())
}
fn unit_interval(number: &str) -> bool {
    let negative=number.starts_with('-');let unsigned=number.strip_prefix('-').unwrap_or(number);
    let (mantissa,exponent)=unsigned.split_once(['e','E']).unwrap_or((unsigned,"0"));
    let before=mantissa.find('.').unwrap_or(mantissa.len()) as i64;
    let mut first=None;let mut index=0i64;let mut one=true;
    for byte in mantissa.bytes().filter(|b|*b!=b'.') {
        if first.is_none() && byte!=b'0' {first=Some(index);one=byte==b'1';}
        else if first.is_some() && byte!=b'0' {one=false;}
        index+=1;
    }
    let Some(first)=first else {return true;};
    if negative {return false;}
    let power=before.saturating_sub(first).saturating_sub(1).saturating_add(exponent_value(exponent));
    power<0 || (power==0 && one)
}
fn text_field(value: &Json, key: &str, required: bool) -> Result<()> {
    match value.get(key) {
        Ok(Str(_))=>Ok(()), Err(_) if !required=>Ok(()),
        _=>Err(format!("MCP content {key} must be text"))
    }
}
fn meta(value: &Json) -> Result<()> {
    if let Ok(value)=value.get("_meta") {if !matches!(value,Obj(_)) {return Err("MCP _meta must be an object".into());}}
    Ok(())
}
pub(super) fn notification_params(value:&Json)->Result<()> {
    if let Ok(params)=value.get("params") {
        if !matches!(params,Obj(_)) {return Err("MCP notification params must be an object".into());}
        meta(params)?;
    }
    Ok(())
}
// Versioned notification shapes; payloads remain untrusted data.
pub(super) fn notification(version:&str,value:&Json)->Result<()> {
    if !VERSIONS.contains(&version){return Err("Unsupported MCP notification revision".into());}
    notification_params(value)?;
    let method=value.get("method")?.text()?;
    if !["notifications/progress","notifications/message","notifications/cancelled","notifications/resources/updated"].contains(&method){return Ok(());}
    let params=value.get("params")?;
    match method {
        "notifications/progress"=>{
            payload_id(params.get("progressToken")?)?;
            if !matches!(params.get("progress"),Ok(Num(_))){return Err("MCP progress requires a number".into());}
            if let Ok(total)=params.get("total"){if !matches!(total,Num(_)){return Err("MCP progress total must be a number".into());}}
            text_field(params,"message",false)?;
        }
        "notifications/message"=>{
            if !["debug","info","notice","warning","error","critical","alert","emergency"].contains(&params.get("level")?.text()?){return Err("Invalid MCP logging level".into());}
            params.get("data")?; // Presence matters: null is valid data.
            text_field(params,"logger",false)?;
        }
        "notifications/cancelled"=>{payload_id(params.get("requestId")?)?;text_field(params,"reason",false)?;}
        "notifications/resources/updated"=>text_field(params,"uri",true)?,
        _=>{}
    }
    Ok(())
}
fn payload_id(value:&Json)->Result<()> {
    // Payload IDs are borrowed under the event bound, not transport-ID caps.
    if matches!(value,Str(_))||matches!(value,Num(n) if integer_number(n)){Ok(())}else{Err("MCP notification payload ID must be text or an integer number".into())}
}

fn annotations(version: &str, value: &Json) -> Result<()> {
    let Ok(value)=value.get("annotations") else {return Ok(());};
    let Obj(fields)=value else {return Err("MCP content annotations must be an object".into());};
    if let Some(value)=fields.get("audience") {
        let Arr(audience)=value else {return Err("MCP content audience must be a role array".into());};
        if audience.iter().any(|role|!matches!(role,Str(role) if role=="user"||role=="assistant")) {return Err("MCP content audience contains an unsupported role".into());}
    }
    if let Some(value)=fields.get("priority") {
        if !matches!(value,Num(number) if unit_interval(number)) {return Err("MCP content priority must be a number between zero and one".into());}
    }
    if modern(version) {text_field(value,"lastModified",false)?;}
    Ok(())
}
fn resource_contents(version: &str, value: &Json) -> Result<()> {
    if !matches!(value,Obj(_)) {return Err("MCP embedded resource contents must be an object".into());}
    text_field(value,"uri",true)?;text_field(value,"mimeType",false)?;
    if modern(version) {meta(value)?;}
    // The official alternatives are anyOf, not XOR. A valid text branch may
    // coexist with an unknown blob extension (and conversely).
    if !matches!(value.get("text"),Ok(Str(_))) && !matches!(value.get("blob"),Ok(Str(_))) {
        return Err("MCP embedded resource requires text or blob text".into());
    }
    Ok(())
}
fn content(version: &str, value: &Json) -> Result<()> {
    if !matches!(value,Obj(_)) {return Err("MCP content member must be an object".into());}
    annotations(version,value)?;
    if modern(version) {meta(value)?;}
    match value.get("type")?.text()? {
        "text"=>text_field(value,"text",true)?,
        "image"|"audio"=>{text_field(value,"data",true)?;text_field(value,"mimeType",true)?;},
        "resource"=>resource_contents(version,value.get("resource")?)?,
        "resource_link" if modern(version)=>{
            text_field(value,"name",true)?;text_field(value,"uri",true)?;
            for key in ["title","description","mimeType"] {text_field(value,key,false)?;}
            if let Ok(value)=value.get("size") {if !matches!(value,Num(_)) {return Err("MCP resource link size must be a number".into());}}
        }
        _=>return Err("Unsupported MCP content type for the negotiated revision".into())
    }
    Ok(())
}

// Validate advertisements without retaining them as instructions or permissions.
pub(super) fn initialization(version:&str,value:&Json)->Result<()> {
    if !VERSIONS.contains(&version){return Err("Unsupported MCP initialization revision".into());}
    text_field(value,"instructions",false)?;
    if modern(version) {text_field(value.get("serverInfo")?,"title",false)?;}
    let capabilities=value.get("capabilities")?;
    if !matches!(capabilities,Obj(_)){return Err("MCP capabilities must be an object".into());}
    for key in ["logging","completions","prompts","resources","tools","experimental"] {
        let Ok(capability)=capabilities.get(key) else {continue;};
        let Obj(fields)=capability else {return Err(format!("MCP {key} capability must be an object"));};
        if key=="experimental"&&fields.values().any(|v|!matches!(v,Obj(_))) {
            return Err("MCP experimental capability values must be objects".into());
        }
        let flags:&[&str]=match key {"prompts"|"tools"=>&["listChanged"],"resources"=>&["subscribe","listChanged"],_=>&[]};
        for flag in flags {if let Some(value)=fields.get(*flag){if !matches!(value,Bool(_)){return Err(format!("MCP {key} {flag} capability must be Boolean"));}}}
    }
    Ok(())
}

pub(super) fn result(version: &str, value: &Json) -> Result<()> {
    let Obj(fields)=value else {return Err("MCP tool result must be an object".into());};
    if !VERSIONS.contains(&version) {return Err("MCP result revision was not negotiated".into());}
    meta(value)?;
    let Arr(items)=value.get("content")? else {return Err("MCP tool result content must be an array".into());};
    for item in items {content(version,item)?;}
    if let Some(value)=fields.get("isError") {
        if !matches!(value,Bool(_)) {return Err("MCP tool result isError must be Boolean".into());}
    }
    if modern(version) {
        if let Some(value)=fields.get("structuredContent") {
            if !matches!(value,Obj(_)) {return Err("MCP structuredContent must be an object".into());}
        }
    }
    Ok(())
}

// Pure whole-page preflight. No partial catalog or cursor reaches application state.
pub(super) fn catalog(version: &str, previous: &str, cursors: &[Json], result: &Json) -> Result<Catalog> {
    if !VERSIONS.contains(&version) {return Err("MCP catalog revision was not negotiated".into());}
    let tools=match result.get("tools")? {Arr(v) if v.len()<=128=>v,_=>return Err("MCP tools page must contain at most 128 tools".into())};
    let next=match result.get("nextCursor") {Err(_)=>None,Ok(Str(v)) if v.is_empty()=>None,Ok(Str(v)) if v.len()<=2048=>Some(v.clone()),Ok(Str(_))=>return Err("MCP pagination cursor exceeds limit".into()),_=>return Err("Invalid MCP pagination cursor".into())};
    let mut staged_cursors=cursors.to_vec();
    if let Some(cursor)=&next {
        if cursors.len()>=8 || cursors.contains(&Str(cursor.clone())) {return Err("MCP tool pagination exceeded its limit or repeated a cursor".into());}
        staged_cursors.push(Str(cursor.clone()));
    }
    let mut out=CatalogText(String::new());out.push(previous)?;out.push("\n")?;
    for (i,tool) in tools.iter().enumerate() {
        let Obj(fields)=tool else {return Err("MCP tool definition must be an object".into());};
        let name=tool.get("name")?.text()?;
        if name.is_empty() || name.len()>256 || name.chars().any(char::is_control) {return Err("Invalid MCP tool name".into());}
        if let Some(v)=fields.get("description") {if !matches!(v,Str(_)) {return Err("MCP tool description must be text".into());}}
        if modern(version) {if let Some(v)=fields.get("title") {if !matches!(v,Str(_)) {return Err("MCP tool title must be text".into());}}}
        if let Some(v)=fields.get("annotations") {
            let Obj(hints)=v else {return Err("MCP tool annotations must be an object".into());};
            if let Some(v)=hints.get("title") {if !matches!(v,Str(_)) {return Err("MCP annotation title must be text".into());}}
            for key in ["readOnlyHint","destructiveHint","idempotentHint","openWorldHint"] {
                if let Some(v)=hints.get(key) {if !matches!(v,Bool(_)) {return Err("MCP tool annotation hints must be Boolean".into());}}
            }
        }
        let input=tool.get("inputSchema")?;shape(input)?;
        let output=if modern(version) {fields.get("outputSchema")} else {None};
        if let Some(output)=output {shape(output)?;}
        if i!=0 {out.push("\n\n")?;}out.push(name)?;out.push("\n")?;out.json(input,0)?;
        if let Some(output)=output {out.push("\nOutput schema (untrusted):\n")?;out.json(output,0)?;}
    }
    Ok(Catalog{text:out.0,cursors:staged_cursors,next})
}
