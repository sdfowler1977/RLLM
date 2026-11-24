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
            messages.iter().rev().find_map(|m|{let raw=text(m,"content");let shown=if text(m,"role")=="assistant"{assistant_display(&raw,false)}else{raw};let content=compact(&shown,120);if content.is_empty(){None}else{Some(content)}}).unwrap_or_else(||"No messages yet".into())
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
