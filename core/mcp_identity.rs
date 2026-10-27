use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

// Sessions are process-local and always freshly initialized after restart.
// Allocation is irreversible even when later preparation or dispatch refuses.
fn serial(counter:&AtomicU64)->Result<Json>{
    let n=counter.try_update(Ordering::Relaxed,Ordering::Relaxed,|n|n.checked_add(1))
        .map_err(|_|"MCP request identifier exhausted")?;
    Ok(s(&format!("client-{n}")))
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

#[cfg(test)] mod checks{
    use super::*;
    #[test] fn allocation_is_unique_and_exhaustion_does_not_wrap(){
        let count=AtomicU64::new(u64::MAX-1);
        assert_eq!(serial(&count).unwrap(),s(&format!("client-{}",u64::MAX-1)));
        assert!(serial(&count).is_err());assert!(serial(&count).is_err());
        assert_eq!(count.load(Ordering::Relaxed),u64::MAX);
        assert_ne!(next().unwrap(),next().unwrap());
    }
    #[test] fn binding_is_typed_explicit_and_directional(){
        let context=|binding:Json,id:Json|obj(vec![("request_binding",binding),("request",s("tools/call")),("request_id",id)]);
        assert_eq!(captured(&context(Num("2".into()),s("client-9"))).unwrap(),Some(s("client-9")));
        for id in [Null,Num("1".into()),s(""),s(&"x".repeat(257))]{assert!(captured(&context(Num("2".into()),id)).is_err());}
        assert!(captured(&obj(vec![("request",s("tools/call"))])).is_err());
        assert!(captured(&context(Num("1".into()),s("client-9"))).is_err());
        let legacy=obj(vec![("request_binding",Num("1".into())),("request",s("tools/call"))]);
        assert_eq!(captured(&legacy).unwrap(),Some(Num("1".into())));
        assert!(matches(&Num("10e-1".into()),&Num("1".into())));
        assert!(!matches(&Num("1".into()),&s("1")));assert!(!matches(&s("1"),&Num("1".into())));
        assert!(!matches(&s("client-8"),&s("client-9")));
        assert!(captured(&obj(vec![("request_binding",Num("2".into())),("request",s("GET"))])).unwrap().is_none());
        assert!(captured(&obj(vec![("request_binding",Num("2".into())),("request",s("GET")),("request_id",s("old"))])).is_err());
    }
}
