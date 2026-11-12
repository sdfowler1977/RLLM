//! Provider tool proposals are data. The application still validates and reviews
//! the single proposed operation through its existing permission boundary.
use super::*;
const LIMIT: usize = 65536;
fn value<'a>(v: &'a Json, k: &str) -> &'a Json { v.get(k).unwrap_or(&Null) }
fn bounded(v: &Json) -> Result<()> {
    if v.string().len() > LIMIT { return Err("Tool proposal exceeds 64 KiB".into()); }
    Ok(())
}
pub(super) fn canonical(name: &str, args: &Json) -> Result<Json> {
    if name.is_empty() || name.len() > 64 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("Invalid tool proposal name".into());
    }
    if !matches!(args, Obj(_)) { return Err("Tool proposal arguments must be an object".into()); }
    bounded(args)?;
    if name=="workspace" {
        let Obj(fields)=args else{unreachable!()};
        if fields.len()!=2||!fields.contains_key("tool")||!fields.contains_key("arguments_json"){return Err("Workspace proposal requires tool and arguments_json only".into());}
        let tool=field(args,"tool")?;if tool=="workspace"{return Err("Nested workspace proposals are invalid".into());}
        return canonical(tool,&parse(field(args,"arguments_json")?)?);
    }
    Ok(obj(vec![("tool", s(name)), ("args", args.clone())]))
}
fn args(v: &Json, encoded: bool) -> Result<Json> {
    if encoded { let text=v.text()?; if text.len()>LIMIT{return Err("Tool arguments exceed 64 KiB".into());} parse(text) }
    else { bounded(v)?; Ok(v.clone()) }
}
fn push(out: &mut Option<Json>, name: &str, input: &Json) -> Result<()> {
    if out.is_some() { return Err("Multiple tool proposals require separate reviewed turns; none was executed".into()); }
    *out=Some(canonical(name,input)?); Ok(())
}
pub(super) fn candidate(body: &Json) -> Result<&Json> {
    let rows=body.get("candidates")?.array()?; let mut selected=None;
    for row in rows {
        if value(row,"index")==&Null || value(row,"index")==&Num("0".into()) {
            if selected.replace(row).is_some(){return Err("Duplicate response candidate zero".into());}
        }
    }
    selected.ok_or("No response candidate zero".into())
}
pub(super) fn choice(body: &Json) -> Result<&Json> {
    let rows=body.get("choices")?.array()?; let mut selected=None;
    for row in rows {
        if value(row,"index")==&Null || value(row,"index")==&Num("0".into()) {
            if selected.replace(row).is_some(){return Err("Duplicate response choice zero".into());}
        }
    }
    selected.ok_or("No response choice zero".into())
}
pub(super) fn proposal(protocol: &str, body: &Json) -> Result<Option<Json>> {
    let mut out=None;
    match protocol {
        "chat"|"local" => {
            let message=if protocol=="chat"{choice(body)?.get("message")?}else{body.get("message")?};
            if value(message,"function_call")!=&Null {return Err("Legacy function_call is not a supported tool proposal".into());}
            if let Ok(calls)=message.get("tool_calls") {
                if calls!=&Null {for call in calls.array()? {
                    bounded(call)?;
                    if protocol=="chat" && field(call,"type")?!="function" {return Err("Unsupported tool proposal type".into());}
                    let f=call.get("function")?;
                    push(&mut out,field(f,"name")?,&args(f.get("arguments")?,protocol=="chat")?)?;
                }}
            }
        },
        "messages" => for part in body.get("content")?.array()? {
            match field(part,"type")? {
                "tool_use" => {bounded(part)?;let id=field(part,"id")?;if id.is_empty()||id.len()>256||id.chars().any(char::is_control){return Err("Invalid tool proposal identity".into());}push(&mut out,field(part,"name")?,part.get("input")?)?;},
                "server_tool_use"|"tool_result" => return Err("Provider-hosted tool blocks require their configured hosted workflow".into()),
                _=>{}
            }
        },
        "content" => if let Ok(content)=candidate(body)?.get("content") {for part in content.get("parts")?.array()? {
            if let Ok(call)=part.get("functionCall") {bounded(call)?;push(&mut out,field(call,"name")?,call.get("args")?)?;}
        }},
        _=>return Err("Unknown tool proposal format".into())
    }
    Ok(out)
}

pub(super) fn declare(body:&mut Json,protocol:&str,tools:&[String])->Result<()> {
    if tools.is_empty(){return Ok(());}
    if tools.len()>64{return Err("Too many declared workspace operations".into());}
    let parameters=obj(vec![("type",s("object")),("properties",obj(vec![
        ("tool",obj(vec![("type",s("string")),("enum",Arr(tools.iter().map(|t|s(t)).collect()))])),
        ("arguments_json",obj(vec![("type",s("string")),("description",s("One JSON object containing the exact arguments of the selected operation, as documented in the system instructions. Not code or a sequence of operations."))]))
    ])),("required",Arr(vec![s("tool"),s("arguments_json")]))]);
    let description="Propose one permitted workspace operation for user review. This declaration grants no permissions and does not execute anything. Use the actual result in a later turn. Never combine multiple operations.";
    let function=obj(vec![("name",s("workspace")),("description",s(description)),("parameters",parameters.clone())]);
    let declaration=match protocol {
        "chat"|"local"=>obj(vec![("type",s("function")),("function",function)]),
        "messages"=>obj(vec![("name",s("workspace")),("description",s(description)),("input_schema",parameters)]),
        "content"=>obj(vec![("functionDeclarations",Arr(vec![function]))]),
        _=>return Err("Unknown structured proposal format".into())
    };
    if let Obj(fields)=body {fields.insert("tools".into(),Arr(vec![declaration]));}else{return Err("Invalid request body".into());}
    if protocol=="chat" {if let Obj(fields)=body{fields.insert("parallel_tool_calls".into(),Bool(false));}}
    Ok(())
}

/// Bounded aggregation of a single streamed Chat function. A second index is
/// refused before any completed proposal can reach the application.
#[derive(Default)]
pub(super) struct Fragments { pub present: bool, name: String, input: String, id: String }
impl Fragments {
    pub(super) fn append(&mut self, calls:&Json)->Result<()> {
        if calls==&Null{return Ok(());}
        if calls.array()?.len()>1{return Err("Multiple streamed tool proposals require separate reviewed turns".into());}
        for call in calls.array()? {
            bounded(call)?;
            if call.get("index")!=Ok(&Num("0".into())) {return Err("Only one reviewed streamed tool proposal is supported".into());}
            self.present=true;
            if let Ok(kind)=call.get("type") {if kind!=&Null&&kind!=&s("function"){return Err("Unsupported streamed tool type".into());}}
            if let Ok(id)=call.get("id") {if id!=&Null {self.id.push_str(id.text()?);}}
            if let Ok(f)=call.get("function") {
                if let Ok(name)=f.get("name") {if name!=&Null{self.name.push_str(name.text()?);}}
                if let Ok(input)=f.get("arguments") {if input!=&Null{self.input.push_str(input.text()?);}}
            }
            if self.name.len()>64||self.id.len()>256||self.input.len()>LIMIT {return Err("Streamed tool proposal exceeds its bounds".into());}
        }
        Ok(())
    }
    pub(super) fn complete(&self)->Result<Json> {
        if !self.present{return Err("No streamed tool proposal".into());}
        canonical(&self.name,&parse(&self.input)?)?;
        if self.id.is_empty()||self.id.chars().any(char::is_control){return Err("Invalid streamed tool identity".into());}
        Ok(obj(vec![("id",s(&self.id)),("type",s("function")),("function",obj(vec![("name",s(&self.name)),("arguments",s(&self.input))]))]))
    }
}
