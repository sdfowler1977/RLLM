use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

// Sessions are process-local and always freshly initialized after restart.
// Allocation is irreversible even when later preparation or dispatch refuses.
fn serial(counter:&AtomicU64)->Result<Json>{
    let n=counter.try_update(Ordering::Relaxed,Ordering::Relaxed,|n|n.checked_add(1))
        .map_err(|_|"MCP request identifier exhausted")?;
    Ok(s(&format!("client-{n}")))
}
// A session header is bound to the selected connection and current phase.
pub(super) fn session_binding(request:&str,expected:&str,received:Option<&str>)->Result<()> {
    if let Some(value)=received {
        if value.is_empty()||value.len()>256||!value.bytes().all(|b|(0x21..=0x7e).contains(&b)) {
            return Err("MCP session identity requires one to 256 visible ASCII bytes".into());
        }
        if request!="initialize" && value!=expected {
            return Err("MCP response session must match the negotiated session".into());
        }
    }
    Ok(())
}

pub(super) fn next()->Result<Json>{
    static NEXT:AtomicU64=AtomicU64::new(1);
    serial(&NEXT)
}
pub(super) fn valid(value:&Json)->bool{
    matches!(value,Str(v) if !v.is_empty() && v.len()<=256)
}
// Version one is an explicit low-level compatibility contract for the old
// numeric-one client. Product-created contexts always use version two.
pub(super) fn captured(context:&Json)->Result<Option<Json>>{
    let binding=context.get("request_binding")?;
    if binding!=&Num("1".into())&&binding!=&Num("2".into()){
        return Err("Unsupported MCP request identity binding".into());
    }
    let request=context.get("request").and_then(Json::text).unwrap_or("");
    if request=="GET" {
        if context.get("request_id").is_ok(){return Err("MCP GET must not bind a request ID".into());}
        return Ok(None);
    }
    if binding==&Num("1".into()) {
        if context.get("request_id").is_ok(){return Err("Legacy MCP binding must not override numeric ID one".into());}
        return Ok(Some(Num("1".into())));
    }
    let value=context.get("request_id")?;
    if !valid(value){return Err("MCP request identity requires a bounded nonempty string".into());}
    if !["initialize","tools/list","tools/call"].contains(&request){return Err("Invalid MCP request identity method".into());}
    Ok(Some(value.clone()))
}
pub(super) fn matches(value:&Json,expected:&Json)->bool{
    match (value,expected) {
        (Str(a),Str(b))=>a==b&&valid(expected),
        (Num(_),Num(b)) if b=="1"=>super::mcp_schema::reply_id(value),
        _=>false,
    }
}
