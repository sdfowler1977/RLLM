use super::*;

const LIMIT: usize = 1024 * 1024;
fn value<'a>(v: &'a Json, key: &str) -> Result<&'a str> { v.get(key)?.text() }
fn set(v: &mut Json, key: &str, value: Json) { if let Obj(fields)=v {fields.insert(key.into(),value);} }
fn text(bytes: &[u8]) -> Result<&str> {
    if bytes.len()>LIMIT || bytes.contains(&0) {return Err("Phone text must be UTF-8 without NUL and at most 1 MiB".into());}
    std::str::from_utf8(bytes).map_err(|_|"Phone text is not valid UTF-8".into())
}
pub fn validate(args: &Json) -> Result<()> {
    let path=value(args,"path")?;let content=value(args,"content")?;text(content.as_bytes())?;
    if path.is_empty() || path.len()>1024 || path.contains(['\\','\0','%',':']) || path.split('/').any(|part|part.is_empty()||part=="."||part==".."||part.len()>255) {return Err("Use a plain relative phone file path inside the selected folder".into());}
    Ok(())
}
pub fn begin(args: &Json, tree: &str, ticket: &str, stamp: &str) -> Result<Json> {
    validate(args)?;let path=value(args,"path")?;let content=value(args,"content")?;
    if tree.is_empty() {return Err("Grant a writable phone folder first".into());}
    let name=format!("RLLM__Phone-Revision__{stamp}.txt");
    Ok(obj(vec![("phase",s("read")),("ticket",s(ticket)),("tree",s(tree)),("path",s(path)),("content",s(content)),("revision_name",s(&name)),("expected",args.get("expected").unwrap_or(&Null).clone())]))
}
pub fn effect(record: &Json) -> Result<Json> {
    if !["read","backup","write"].contains(&value(record,"phase")?) {return Err("Phone revision has no active operation".into());}
    let mut result=record.clone();set(&mut result,"kind",s("phone_revision"));Ok(result)
}
/// Compare the actual currently opened document before any truncation. The
/// Android binding keeps that descriptor open through the corresponding write.
pub fn check(record: &Json, actual_id: &str, current: &[u8]) -> Result<()> {
    if value(record,"phase")?!="write" {return Err("Phone revision is not ready to update".into());}
    if actual_id!=value(record,"document_id")? || text(current)?!=value(record,"previous")? {return Err("Phone file changed since its revision was read; the target was not updated".into());}
    Ok(())
}
pub fn advance(record: &Json, phase: &str, result: &Json) -> Result<Json> {
    if value(record,"phase")?!=phase {return Err("Phone revision result is no longer current".into());}
    let mut next=record.clone();
    match phase {
        "read" => {
            let bytes=network::unbase64(value(result,"data")?)?;let previous=text(&bytes)?;
            if let Ok(expected)=record.get("expected") {if expected!=&Null && expected.text()?!=previous {return Err("Phone file changed since it was loaded into the editor; reload it before updating".into());}}
            for name in ["document_id","parent_id"] {let id=value(result,name)?;if id.is_empty()||id.len()>8192{return Err("Invalid phone document identity".into());}set(&mut next,name,s(id));}
            set(&mut next,"previous",s(previous));set(&mut next,"expected",Null);set(&mut next,"phase",s("backup"));
        }
        "backup" => {
            let bytes=network::unbase64(value(result,"data")?)?;
            if text(&bytes)?!=value(record,"previous")? {return Err("Phone revision readback differs; the target was not updated".into());}
            let name=value(result,"name")?;let id=value(result,"document_id")?;
            if name.is_empty()||name.len()>255||name.contains(['/','\\','\0'])||id.is_empty()||id==value(record,"document_id")? {return Err("Provider did not create a separate phone revision".into());}
            let parent=value(record,"path")?.rsplit_once('/').map(|(p,_)|format!("{p}/")).unwrap_or_default();
            set(&mut next,"backup",s(&format!("{parent}{name}")));set(&mut next,"revision_id",s(id));set(&mut next,"phase",s("write"));
        }
        "write" => {
            let bytes=network::unbase64(value(result,"data")?)?;
            if text(&bytes)?!=value(record,"content")? {return Err(format!("Updated phone file readback differs. Prior contents remain at {}. Inspect the target before retrying.",value(record,"backup")?));}
            set(&mut next,"phase",s("complete"));set(&mut next,"previous",Null);set(&mut next,"content",Null);
        }
        _ => return Err("Unexpected phone revision phase".into()),
    }
    Ok(next)
}
pub fn operation(v: &Json) -> Result<Json> {
    let bytes=network::unbase64(value(v,"data")?)?;
    check(v.get("record")?,value(v,"document_id")?,&bytes)?;Ok(Null)
}
pub fn interrupt(record: &Json) -> Json {
    let mut next=record.clone();set(&mut next,"phase",s("interrupted"));set(&mut next,"previous",Null);set(&mut next,"content",Null);set(&mut next,"expected",Null);next
}
pub fn interruption(record: &Json) -> String {
    if let Ok(backup)=value(record,"backup") {format!("Phone update is unconfirmed. Prior contents were verified at {backup}. Read the target before retrying; no update will be replayed automatically.")}
    else {"Phone update stopped before the target-write stage. The target was not intentionally changed; an incomplete new revision document may remain. No update will be replayed automatically.".into()}
}

#[cfg(test)]
mod checks {
    use super::*;
    fn initial()->Json {begin(&obj(vec![("path",s("nested/note.txt")),("content",s("New café 中文"))]),"content://folder","job","2026-10-25__23-00-00Z").unwrap()}
    fn read()->Json {advance(&initial(),"read",&obj(vec![("data",s(&network::b64(b"Original"))),("document_id",s("original")),("parent_id",s("parent"))])).unwrap()}
    fn backup()->Json {advance(&read(),"backup",&obj(vec![("data",s(&network::b64(b"Original"))),("document_id",s("revision")),("name",s("saved.txt"))])).unwrap()}
    #[test] fn revision_must_verify_before_the_original_can_change(){
        let start=initial();assert!(check(&start,"original",b"Original").is_err());let staged=read();assert!(check(&staged,"original",b"Original").is_err());
        let prepared=backup();assert_eq!(value(&prepared,"backup"),Ok("nested/saved.txt"));check(&prepared,"original",b"Original").unwrap();assert!(check(&prepared,"other",b"Original").is_err());assert!(check(&prepared,"original",b"Changed").is_err());
        let complete=advance(&prepared,"write",&obj(vec![("data",s(&network::b64("New café 中文".as_bytes())))] )).unwrap();assert_eq!(value(&complete,"phase"),Ok("complete"));assert_eq!(complete.get("previous"),Ok(&Null));assert!(effect(&complete).is_err());
    }
    #[test] fn altered_backups_and_unverified_updates_never_report_success(){
        let mut result=obj(vec![("data",s(&network::b64(b"Altered"))),("document_id",s("revision")),("name",s("saved.txt"))]);assert!(advance(&read(),"backup",&result).is_err());
        set(&mut result,"data",s(&network::b64(b"Original")));set(&mut result,"document_id",s("original"));assert!(advance(&read(),"backup",&result).is_err());
        assert!(advance(&backup(),"write",&obj(vec![("data",s(&network::b64(b"Partial")))])).unwrap_err().contains("nested/saved.txt"));
        let mut interrupted=backup();set(&mut interrupted,"phase",s("interrupted"));assert!(effect(&interrupted).is_err());assert!(check(&interrupted,"original",b"Original").is_err());assert!(advance(&interrupted,"write",&result).is_err());
    }
    #[test] fn invalid_paths_binary_originals_and_oversize_contents_are_refused(){
        for path in ["../note","/note","note/","a//b","a/./b","a%2fb",""] {assert!(begin(&obj(vec![("path",s(path)),("content",s("new"))]),"tree","ticket","stamp").is_err());}
        for data in [vec![0],vec![255],vec![b'x';LIMIT+1]] {assert!(advance(&initial(),"read",&obj(vec![("data",s(&network::b64(&data))),("document_id",s("original")),("parent_id",s("parent"))])).is_err());}
    }
}
