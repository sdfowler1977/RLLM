//! Compact conversation presentation; never mutates stored conversations.
use super::*;
fn compact(value:&str,limit:usize)->String {
    let mut out=String::new();let mut gap=false;let mut count=0;
    for c in value.chars().take(4096) {
        if c.is_whitespace()||c.is_control()||matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {gap=!out.is_empty();continue;}
        if count>=limit {out.push('…');break;}
        if gap {out.push(' ');gap=false;}out.push(c);count+=1;
    }out
}
pub(super) fn rows(st:&Json)->Vec<Json> {
    let mut rows=vec![button("New conversation","new")];
    // Storage order is retained; timestamps are not guessed from opaque IDs.
    for c in array(st,"chats") {
        let id=text(&c,"id");let messages=array(&c,"messages");let mut title=compact(&text(&c,"title"),80);
        if title.is_empty(){title="Untitled conversation".into();}
        let draft=compact(&text(&c,"draft"),120);
        let active=id==text(st,"active");let live_draft=if active{compact(&text(st,"draft"),120)}else{draft};
        let preview=if !live_draft.is_empty(){format!("Draft: {live_draft}")}else{
            messages.iter().rev().find_map(|m|{let content=compact(&text(m,"content"),120);if content.is_empty(){None}else{Some(content)}}).unwrap_or_else(||"No messages yet".into())
        };
        let count=messages.len();let detail=format!("{}{} {}",if active{"Current · "}else{""},count,if count==1{"message"}else{"messages"});
        let mut row=button(&format!("{title} · {detail}"),&format!("history:{id}"));
        for (key,value) in [("kind","conversation".into()),("title",title),("preview",preview),("detail",detail),("delete_event",format!("chat_delete_choose:{id}")),("options_event",format!("chat_options:{id}"))] {put(&mut row,key,s(&value));}
        put(&mut row,"active",Bool(active));rows.push(row);
    }
    rows.push(button(&format!("Trash ({})",array(st,"trash").len()),"trash"));
    rows.push(button("Import conversation backup","conversation_import"));
    if chat(st)!=Null {rows.push(button("Export current conversation","conversation_export"));}
    rows
}

#[cfg(test)]mod checks {
    use super::*;
    #[test]fn rows_bind_exact_ids_and_preserve_stored_text(){
        let message=obj(vec![("role",s("assistant")),("content",s("Last reply"))]);
        let conversation=obj(vec![("id",s("opaque:one")),("title",s("Title\nwith\u{202e}controls")),("messages",Arr(vec![message]))]);
        let st=obj(vec![("active",s("opaque:one")),("draft",s("unsent\ntext")),("chats",Arr(vec![conversation])),("trash",Arr(vec![]))]);
        let before=st.string();let result=rows(&st);let row=&result[1];
        assert_eq!(text(row,"title"),"Title with controls");assert_eq!(text(row,"preview"),"Draft: unsent text");
        assert_eq!(text(row,"delete_event"),"chat_delete_choose:opaque:one");assert_eq!(text(row,"options_event"),"chat_options:opaque:one");assert_eq!(text(row,"detail"),"Current · 1 message");assert_eq!(before,st.string());
    }
    #[test]fn empty_history_and_bounded_unicode_preview(){
        let empty=obj(vec![("chats",Arr(vec![])),("trash",Arr(vec![]))]);let result=rows(&empty);assert!(result.iter().all(|r|text(r,"kind")!="conversation"));assert!(result.iter().all(|r|text(r,"event")!="conversation_export"));
        let text_value="日".repeat(1000);let shortened=compact(&text_value,80);assert_eq!(shortened.chars().count(),81);assert!(shortened.ends_with('…'));
    }
}
