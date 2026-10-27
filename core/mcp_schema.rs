use super::{Json, Json::*, Result};

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
// Known March/June notification shapes only; payloads remain discarded data.
pub(super) fn notification(version:&str,value:&Json)->Result<()> {
    if !["2025-03-26","2025-06-18"].contains(&version){return Err("Unsupported MCP notification revision".into());}
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
    if version=="2025-06-18" {text_field(value,"lastModified",false)?;}
    Ok(())
}
fn resource_contents(version: &str, value: &Json) -> Result<()> {
    if !matches!(value,Obj(_)) {return Err("MCP embedded resource contents must be an object".into());}
    text_field(value,"uri",true)?;text_field(value,"mimeType",false)?;
    if version=="2025-06-18" {meta(value)?;}
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
    if version=="2025-06-18" {meta(value)?;}
    match value.get("type")?.text()? {
        "text"=>text_field(value,"text",true)?,
        "image"|"audio"=>{text_field(value,"data",true)?;text_field(value,"mimeType",true)?;},
        "resource"=>resource_contents(version,value.get("resource")?)?,
        "resource_link" if version=="2025-06-18"=>{
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
    if !["2025-03-26","2025-06-18"].contains(&version){return Err("Unsupported MCP initialization revision".into());}
    text_field(value,"instructions",false)?;
    if version=="2025-06-18" {text_field(value.get("serverInfo")?,"title",false)?;}
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
    if !["2025-03-26","2025-06-18"].contains(&version) {return Err("MCP result revision was not negotiated".into());}
    meta(value)?;
    let Arr(items)=value.get("content")? else {return Err("MCP tool result content must be an array".into());};
    for item in items {content(version,item)?;}
    if let Some(value)=fields.get("isError") {
        if !matches!(value,Bool(_)) {return Err("MCP tool result isError must be Boolean".into());}
    }
    if version=="2025-06-18" {
        if let Some(value)=fields.get("structuredContent") {
            if !matches!(value,Obj(_)) {return Err("MCP structuredContent must be an object".into());}
        }
    }
    Ok(())
}

// Pure whole-page preflight. No partial catalog or cursor reaches application state.
pub(super) fn catalog(version: &str, previous: &str, cursors: &[Json], result: &Json) -> Result<Catalog> {
    if !["2025-03-26","2025-06-18"].contains(&version) {return Err("MCP catalog revision was not negotiated".into());}
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
        if version=="2025-06-18" {if let Some(v)=fields.get("title") {if !matches!(v,Str(_)) {return Err("MCP tool title must be text".into());}}}
        if let Some(v)=fields.get("annotations") {
            let Obj(hints)=v else {return Err("MCP tool annotations must be an object".into());};
            if let Some(v)=hints.get("title") {if !matches!(v,Str(_)) {return Err("MCP annotation title must be text".into());}}
            for key in ["readOnlyHint","destructiveHint","idempotentHint","openWorldHint"] {
                if let Some(v)=hints.get(key) {if !matches!(v,Bool(_)) {return Err("MCP tool annotation hints must be Boolean".into());}}
            }
        }
        let input=tool.get("inputSchema")?;shape(input)?;
        let output=if version=="2025-06-18" {fields.get("outputSchema")} else {None};
        if let Some(output)=output {shape(output)?;}
        if i!=0 {out.push("\n\n")?;}out.push(name)?;out.push("\n")?;out.json(input,0)?;
        if let Some(output)=output {out.push("\nOutput schema (untrusted):\n")?;out.json(output,0)?;}
    }
    Ok(Catalog{text:out.0,cursors:staged_cursors,next})
}

#[cfg(test)] mod checks {
    use super::*;
    use crate::{obj,s,parse};
    fn page(schema:Json)->Json {obj(vec![("tools",Arr(vec![obj(vec![("name",s("tool")),("inputSchema",schema)])]))])}
    fn init()->Json {obj(vec![("serverInfo",obj(vec![])),("capabilities",obj(vec![("tools",obj(vec![]))]))])}
    #[test] fn initialization_advertisements_are_versioned_and_extensions_untrusted(){
        for version in ["2025-03-26","2025-06-18"] {
            let mut value=init();let mut caps=value.get("capabilities").unwrap().clone();
            for key in ["logging","completions"] {if let Obj(fields)=&mut caps{fields.insert(key.into(),obj(vec![("listChanged",Null),("unknown",s("never authority"))]));}}
            if let Obj(fields)=&mut caps {
                fields.insert("experimental".into(),obj(vec![("",obj(vec![("permissions",Bool(true)),("uri",s("never:fetch"))]))]));
                fields.insert("unknown".into(),Arr(vec![Null,Bool(false)]));
            }
            if let Obj(fields)=&mut value{fields.insert("capabilities".into(),caps);fields.insert("instructions".into(),s("untrusted never copied"));}
            let prior=value.clone();initialization(version,&value).unwrap();assert_eq!(value,prior);
        }
        let mut value=init();if let Obj(fields)=&mut value{fields.insert("serverInfo".into(),obj(vec![("title",Null)]));}
        assert!(initialization("2025-03-26",&value).is_ok());assert!(initialization("2025-06-18",&value).is_err());
    }
    #[test] fn initialization_known_shapes_refuse_null_and_wrong_types(){
        for version in ["2025-03-26","2025-06-18"] {
            for key in ["logging","completions","prompts","resources","tools","experimental"] {
                for wrong in [Null,Bool(false),Arr(vec![]),s("object")] {
                    let mut value=init();if let Obj(fields)=&mut value{fields.insert("capabilities".into(),obj(vec![(key,wrong)]));}assert!(initialization(version,&value).is_err(),"{key}");
                }
                for flag in match key {"prompts"|"tools"=>vec!["listChanged"],"resources"=>vec!["subscribe","listChanged"],_=>vec![]} {
                    for bad in [Null,s("true"),Num("1".into()),Arr(vec![])] {
                        let mut value=init();if let Obj(fields)=&mut value{fields.insert("capabilities".into(),obj(vec![(key,obj(vec![(flag,bad)]))]));}assert!(initialization(version,&value).is_err());
                    }
                }
            }
            for bad in [Null,s("object"),Bool(false),Arr(vec![])] {
                let mut value=init();if let Obj(fields)=&mut value{fields.insert("capabilities".into(),obj(vec![("experimental",obj(vec![("feature",bad)]))]));}assert!(initialization(version,&value).is_err());
            }
            for bad in [Null,Bool(false),Num("0".into()),Arr(vec![])] {let mut value=init();if let Obj(fields)=&mut value{fields.insert("instructions".into(),bad);}assert!(initialization(version,&value).is_err());}
        }
    }
    #[test] fn catalog_shapes_are_typed_and_extensions_stay_data() {
        for version in ["2025-03-26","2025-06-18"] {
            let schema=parse(r#"{"type":"object","properties":{"é":{}},"required":["é"],"$ref":"https://untrusted.invalid/schema","unknown":[true,null]}"#).unwrap();
            let p=page(schema.clone());let c=catalog(version,"",&[],&p).unwrap();assert_eq!(c.text,format!("\ntool\n{}",schema.string()));
            for schema in [Null,obj(vec![]),obj(vec![("type",s("array"))]),parse(r#"{"type":"object","properties":null}"#).unwrap(),parse(r#"{"type":"object","properties":{"x":true}}"#).unwrap(),parse(r#"{"type":"object","required":[null]}"#).unwrap()] {assert!(catalog(version,"prior",&[],&page(schema)).is_err());}
        }
    }
    #[test] fn catalog_output_revision_hints_and_atomic_last_member() {
        let mut p=page(obj(vec![("type",s("object"))]));
        if let Obj(v)=&mut p {if let Some(Arr(tools))=v.get_mut("tools") {if let Obj(t)=&mut tools[0] {t.insert("outputSchema".into(),Null);t.insert("annotations".into(),obj(vec![("readOnlyHint",Bool(true)),("idempotentHint",Bool(true))]));}}}
        assert!(catalog("2025-03-26","",&[],&p).is_ok());assert!(catalog("2025-06-18","",&[],&p).is_err());
        if let Obj(v)=&mut p {if let Some(Arr(tools))=v.get_mut("tools") {if let Obj(t)=&mut tools[0] {t.insert("outputSchema".into(),obj(vec![("type",s("object"))]));}}}
        assert!(catalog("2025-06-18","",&[],&p).unwrap().text.contains("Output schema (untrusted)"));
        let before=p.clone();if let Obj(v)=&mut p {if let Some(Arr(tools))=v.get_mut("tools") {tools.push(obj(vec![("name",s("bad")),("inputSchema",Null)]));}}
        assert!(catalog("2025-06-18","prior",&[s("old")],&p).is_err());assert!(catalog("2025-06-18","prior",&[s("old")],&before).is_ok());
    }
    #[test] fn catalog_rendering_and_pagination_are_bounded_without_partial_state() {
        let mut p=page(obj(vec![("type",s("object")),("description",s(&"\\\n😀".repeat(20000)))]));
        assert!(catalog("2025-06-18","prior",&[],&p).is_err());
        p=page(obj(vec![("type",s("object"))]));if let Obj(v)=&mut p {v.insert("nextCursor".into(),s("old"));}
        assert!(catalog("2025-06-18","prior",&[s("old")],&p).is_err());
        assert!(catalog("2025-06-18",&"x".repeat(65536),&[],&page(obj(vec![("type",s("object"))]))).is_err());
        let empty=obj(vec![("tools",Arr(vec![]))]);assert_eq!(catalog("2025-03-26","",&[],&empty).unwrap().text,"\n");
        let value=parse(r#"{"a":"\u0001\b\n\\\"😀","b":[null,true,12.3]}"#).unwrap();let mut text=CatalogText(String::new());text.json(&value,0).unwrap();assert_eq!(text.0,value.string());
    }
    #[test] fn tool_result_flags_are_typed_before_evidence() {
        for version in ["2025-03-26","2025-06-18"] {
            for value in [obj(vec![("content",Arr(vec![]))]),obj(vec![("content",Arr(vec![])),("isError",Bool(false))]),obj(vec![("content",Arr(vec![])),("isError",Bool(true))])] {assert!(result(version,&value).is_ok());}
            for value in [Null,s("true"),Num("1".into()),Arr(vec![])] {assert!(result(version,&obj(vec![("content",Arr(vec![])),("isError",value)])).is_err());}
        }
        assert!(result("2025-06-18",&obj(vec![("content",Arr(vec![])),("structuredContent",obj(vec![("untrusted",s("data"))]))])).is_ok());
        for value in [Null,s("data"),Arr(vec![]),Bool(false)] {
            let value=obj(vec![("content",Arr(vec![])),("structuredContent",value)]);
            assert!(result("2025-06-18",&value).is_err());assert!(result("2025-03-26",&value).is_ok());
        }
    }
    #[test] fn persistence_budget_counts_platform_string_quoting_and_numeric_room() {
        let value=obj(vec![("/key",s("/\\\"\n\u{8}\u{c}😀")),("number",Num("1e6".into()))]);
        let canonical=value.string().len();assert!(state_bytes(&value)>=canonical+2);
        let integers=obj(vec![("i",Num("42".into())),("text",s("plain"))]);assert_eq!(state_bytes(&integers),integers.string().len());
        assert_eq!(quoted_bytes("/"),4);assert_eq!(quoted_bytes("\u{8}\u{c}"),6);assert_eq!(quoted_bytes("😀"),6);
    }
    fn tool_output(items:Vec<Json>)->Json {obj(vec![("content",Arr(items))])}
    #[test] fn content_envelopes_preserve_untrusted_data_and_revision_boundaries() {
        let mut items=vec![
            parse(r#"{"type":"text","text":"😀\n","extension":{"permission":true}}"#).unwrap(),
            parse(r#"{"type":"image","data":"unparsed base64","mimeType":"image/x-custom"}"#).unwrap(),
            parse(r#"{"type":"audio","data":"","mimeType":"audio/x-custom"}"#).unwrap(),
            parse(r#"{"type":"resource","resource":{"uri":"custom:never-fetch","text":"literal"}}"#).unwrap()
        ];
        for version in ["2025-03-26","2025-06-18"] {let value=tool_output(items.clone());let before=value.clone();assert!(result(version,&value).is_ok());assert_eq!(value,before);assert!(result(version,&tool_output(vec![])).is_ok());}
        let link=parse(r#"{"type":"resource_link","name":"","uri":"untrusted:not-fetched","size":-0.5,"annotations":{"priority":1}}"#).unwrap();
        items.push(link);assert!(result("2025-06-18",&tool_output(items.clone())).is_ok());assert!(result("2025-03-26",&tool_output(items)).is_err());
    }
    #[test] fn malformed_content_whole_result_refuses_without_partial_adoption() {
        let good=parse(r#"{"type":"text","text":"must remain unadopted"}"#).unwrap();
        for version in ["2025-03-26","2025-06-18"] {
            for bad in [Null,Arr(vec![]),s("text"),obj(vec![]),parse(r#"{"type":null}"#).unwrap(),parse(r#"{"type":"unknown","text":"x"}"#).unwrap(),parse(r#"{"type":"text","text":null}"#).unwrap(),parse(r#"{"type":"image","data":"x"}"#).unwrap(),parse(r#"{"type":"audio","data":false,"mimeType":"x"}"#).unwrap(),parse(r#"{"type":"resource","resource":null}"#).unwrap()] {
                let value=tool_output(vec![good.clone(),bad]);let before=value.clone();assert!(result(version,&value).is_err());assert_eq!(value,before);
            }
            for value in [obj(vec![]),obj(vec![("content",Null)]),obj(vec![("content",s("x"))]),obj(vec![("content",obj(vec![]))])] {assert!(result(version,&value).is_err());}
        }
    }
    #[test] fn embedded_resource_anyof_branches_are_not_exclusive() {
        for version in ["2025-03-26","2025-06-18"] {
            for fields in [r#""text":"a""#,r#""blob":"b""#,r#""text":"a","blob":"b""#,r#""text":"a","blob":null"#,r#""text":false,"blob":"b""#] {
                let value=parse(&format!(r#"{{"type":"resource","resource":{{"uri":"","mimeType":"",{fields}}}}}"#)).unwrap();assert!(result(version,&tool_output(vec![value])).is_ok());
            }
            for fields in [r#""uri":"x""#,r#""uri":null,"text":"a""#,r#""uri":"x","text":null,"blob":false"#,r#""uri":"x","text":"a","mimeType":null"#] {
                let value=parse(&format!(r#"{{"type":"resource","resource":{{{fields}}}}}"#)).unwrap();assert!(result(version,&tool_output(vec![value])).is_err());
            }
        }
    }
    #[test] fn content_metadata_annotations_and_priority_are_exact_and_versioned() {
        for n in ["0","-0","0.0","-0e999999999999999999999999999","1","1.0","10e-1","0.10e1","0.99999999999999999999999","1e-999999999999999999999999999"] {assert!(unit_interval(n),"{n}");}
        for n in ["-1","-1e-9999999999999999999999","1.00000000000000000000001","10e0","1e999999999999999999999","0.100000000000000000001e1"] {assert!(!unit_interval(n),"{n}");}
        for version in ["2025-03-26","2025-06-18"] {
            for annotations in [r#"null"#,r#"[]"#,r#"{"audience":["system"]}"#,r#"{"audience":[null]}"#,r#"{"audience":"user"}"#,r#"{"priority":null}"#,r#"{"priority":-1e-999}"#,r#"{"priority":1.000000000000000000001}"#] {
                let item=parse(&format!(r#"{{"type":"text","text":"x","annotations":{annotations}}}"#)).unwrap();assert!(result(version,&tool_output(vec![item])).is_err());
            }
            let value=parse(r#"{"content":[],"_meta":null}"#).unwrap();assert!(result(version,&value).is_err());
            let item=parse(r#"{"type":"text","text":"x","annotations":{"audience":["user","assistant","user"],"priority":0.01}}"#).unwrap();assert!(result(version,&tool_output(vec![item])).is_ok());
        }
        for item in [r#"{"type":"text","text":"x","_meta":null}"#,r#"{"type":"text","text":"x","annotations":{"lastModified":null}}"#,r#"{"type":"resource","resource":{"uri":"x","text":"x","_meta":null}}"#] {let value=tool_output(vec![parse(item).unwrap()]);assert!(result("2025-03-26",&value).is_ok());assert!(result("2025-06-18",&value).is_err());}
    }
    #[test] fn content_fields_use_existing_response_bounds_not_catalog_limit() {
        let value=tool_output(vec![obj(vec![("type",s("text")),("text",s(&"😀/\\".repeat(20000)))])]);assert!(value.string().len()>65536);assert!(result("2025-06-18",&value).is_ok());
        for size in ["-1.5","1.25","1e999999999999999999"] {let item=parse(&format!(r#"{{"type":"resource_link","name":"x","uri":"x","size":{size}}}"#)).unwrap();assert!(result("2025-06-18",&tool_output(vec![item])).is_ok());}
        for size in ["null","true",r#""1""#] {let item=parse(&format!(r#"{{"type":"resource_link","name":"x","uri":"x","size":{size}}}"#)).unwrap();assert!(result("2025-06-18",&tool_output(vec![item])).is_err());}
    }

    #[test] fn common_error_envelopes_require_exact_integer_codes_and_text_messages() {
        for number in ["1","-1","1.0","10e-1","12.00e0","1e999999999999999999999999999","-0e-999999999999999999999999","1000000000000000000000000000000000000000000000000000000000000000000000"] {
            assert!(integer_number(number),"{number}");
            let value=parse(&format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":{number},"message":"","data":null,"_meta":false}}}}"#)).unwrap();assert!(response(&value).is_ok());
        }
        for number in ["1.1","-0.01","1e-999999999999999999999","100.01e-1","1.000000000000000000000001"] {assert!(!integer_number(number),"{number}");let value=parse(&format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":{number},"message":"x"}}}}"#)).unwrap();assert!(response(&value).is_err());}
        for error in ["null","false","[]","{}",r#"{"code":1}"#,r#"{"message":"x"}"#,r#"{"code":"1","message":"x"}"#,r#"{"code":1,"message":null}"#] {let value=parse(&format!(r#"{{"jsonrpc":"2.0","id":1,"error":{error}}}"#)).unwrap();assert!(response(&value).is_err());}
    }
    #[test] fn common_result_metadata_direction_and_presence_remain_separate() {
        for result in [r#"{}"#,r#"{"_meta":{}}"#,r#"{"_meta":{"unknown":null},"extension":false}"#] {let value=parse(&format!(r#"{{"jsonrpc":"2.0","id":1,"result":{result},"_meta":false}}"#)).unwrap();assert!(response(&value).is_ok());}
        for value in [r#"{"jsonrpc":"2.0","id":1,"result":null}"#,r#"{"jsonrpc":"2.0","id":1,"result":{"_meta":null}}"#,r#"{"jsonrpc":"2.0","id":1,"result":{},"error":null}"#,r#"{"jsonrpc":"2.0","id":1,"error":null,"method":null}"#,r#"{"jsonrpc":"2.0","id":1,"result":{},"method":"ping"}"#] {assert!(response(&parse(value).unwrap()).is_err());}
    }

    #[test] fn notification_parameters_are_optional_objects_with_untrusted_metadata() {
        for fields in [r#"{}"#,r#"{"params":{}}"#,r#"{"params":{"_meta":{"progressToken":null,"arbitrary":[true,null]},"unknown":{"_meta":null}},"_meta":null}"#] {assert!(notification_params(&parse(fields).unwrap()).is_ok());}
        for fields in [r#"{"params":null}"#,r#"{"params":[]}"#,r#"{"params":false}"#,r#"{"params":{"_meta":null}}"#,r#"{"params":{"_meta":[]}}"#,r#"{"params":{"_meta":"x"}}"#] {assert!(notification_params(&parse(fields).unwrap()).is_err());}
    }

    #[test] fn numeric_reply_identity_is_exact_bounded_and_not_a_string() {
        for number in ["1","1.0","1e0","10e-1","0.01e2","1.00000000000000000000000000000000"] {assert!(reply_id(&Num(number.into())),"{number}");}
        assert!(reply_id(&Num(format!("1e{}","0".repeat(62)))));assert!(!reply_id(&Num(format!("1e{}","0".repeat(63)))));
        for value in [s("1"),Null,Bool(true),Num("0".into()),Num("-0e99999999999999999999999".into()),Num("-1".into()),Num("1e999999999999999999999".into()),Num("1e-999999999999999999999".into()),Num("1.000000000000000000001".into()),Num("0.999999999999999999999".into()),Num("9007199254740993".into())] {assert!(!reply_id(&value),"{}",value.string());}
    }

}

#[cfg(test)] mod notification_checks {
 use super::*;use crate::{obj,s,parse};
 fn note(method:&str,params:Json)->Json{obj(vec![("jsonrpc",s("2.0")),("method",s(method)),("params",params)])}
 #[test]fn known_notification_shapes_preserve_untrusted_payloads_and_exact_numbers(){
  for version in ["2025-03-26","2025-06-18"] {
   for level in ["debug","info","notice","warning","error","critical","alert","emergency"] {
    let value=note("notifications/message",obj(vec![("level",s(level)),("data",Null),("logger",s(""))]));let prior=value.clone();notification(version,&value).unwrap();assert_eq!(value,prior);
   }
   for id in [s(""),s(&"x".repeat(1000)),Num("10e-1".into()),Num(format!("1e{}","9".repeat(200))),Num("-0e-9999999999999999999999".into())] {
    let value=note("notifications/progress",obj(vec![("progressToken",id.clone()),("progress",Num("-1.5e999999999999999999999".into())),("total",Num("0.1".into())),("message",s("untrusted")),("_meta",obj(vec![("progressToken",Null)]))]));notification(version,&value).unwrap();
    notification(version,&note("notifications/cancelled",obj(vec![("requestId",id),("reason",s("never cancel local state"))]))).unwrap();
   }
   notification(version,&note("notifications/resources/updated",obj(vec![("uri",s("never:fetch"))]))).unwrap();
   for method in ["notifications/resources/list_changed","notifications/tools/list_changed","notifications/prompts/list_changed","notifications/logging","unknown"] {
    notification(version,&obj(vec![("jsonrpc",s("2.0")),("method",s(method))])).unwrap();
   }
  }
 }
 #[test]fn missing_known_fields_and_wrong_types_refuse_before_side_effects(){
  for version in ["2025-03-26","2025-06-18"] {
   for raw in [r#"{"method":"notifications/progress"}"#,r#"{"method":"notifications/progress","params":{}}"#,r#"{"method":"notifications/progress","params":{"progressToken":null,"progress":1}}"#,r#"{"method":"notifications/progress","params":{"progressToken":1.5,"progress":1}}"#,r#"{"method":"notifications/progress","params":{"progressToken":"x","progress":"1"}}"#,r#"{"method":"notifications/progress","params":{"progressToken":"x","progress":1,"message":null}}"#,r#"{"method":"notifications/progress","params":{"progressToken":"x","progress":1,"total":false}}"#,r#"{"method":"notifications/message","params":{"level":"info"}}"#,r#"{"method":"notifications/message","params":{"level":"INFO","data":null}}"#,r#"{"method":"notifications/message","params":{"level":"debug","data":null,"logger":null}}"#,r#"{"method":"notifications/cancelled","params":{"requestId":0.01}}"#,r#"{"method":"notifications/cancelled","params":{"requestId":1,"reason":false}}"#,r#"{"method":"notifications/resources/updated","params":{"uri":null}}"#] {
    let value=parse(raw).unwrap();let prior=value.clone();assert!(notification(version,&value).is_err(),"{raw}");assert_eq!(value,prior);
   }
  }
 }
}
