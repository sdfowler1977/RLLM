//! Conversation choices and persistent visual preferences.
use super::*;

pub(super) fn connection_label(p:&Json)->String {let name=text(p,"name");if name.trim().is_empty(){text(p,"model")}else{name.trim().into()}}
pub(super) fn choice(key:&str,label:&str,value:&str,items:&[(&str,&str)])->Json {
    let mut f=field_ui(key,label,value,"choice");put(&mut f,"options",Arr(items.iter().map(|(label,value)|obj(vec![("label",s(label)),("value",s(value))])).collect()));f
}
pub(super) fn locked(st:&Json)->bool {boolean(st,"busy")||get(st,"pending")!=&Null||get(st,"task_run")!=&Null||boolean(st,"agent_running")}
pub(super) fn agent_index(st:&Json)->usize {let c=chat(st);if get(&c,"agent_profile")!=&Null{number(&c,"agent_profile")}else{number(st,"agent_selected")}}
pub(super) fn select_agent(st:&mut Json,index:usize)->Result<()> {
    if locked(st){return Err("Finish or cancel the current task or tool review first".into());}
    if index>=array(st,"agents").len(){return Err("Agent profile not found".into());}
    // Freeze older conversations at their previous global default once.
    let previous=number(st,"agent_selected");let mut chats=array(st,"chats");
    for c in &mut chats {if get(c,"agent_profile")==&Null{put(c,"agent_profile",Num(previous.to_string()));}}
    put(st,"chats",Arr(chats));let mut trash=array(st,"trash");for c in &mut trash{if get(c,"agent_profile")==&Null{put(c,"agent_profile",Num(previous.to_string()));}}put(st,"trash",Arr(trash));put(st,"agent_selected",Num(index.to_string()));
    let mut c=chat(st);if c!=Null{put(&mut c,"agent_profile",Num(index.to_string()));replace_chat(st,c);}
    put(st,"dialog",Null);put(st,"notice",s(""));Ok(())
}
fn target(st:&Json,trash:bool)->Result<Json>{
    let id=text(st,"chat_target");if id.is_empty(){return Err("Choose a conversation first".into());}
    let matches:Vec<Json>=array(st,if trash{"trash"}else{"chats"}).into_iter().filter(|c|text(c,"id")==id).collect();
    if matches.len()>1{return Err("Conversation identity is ambiguous; no conversation was changed".into());}
    matches.into_iter().next().ok_or("Conversation no longer exists".into())
}
fn scoped(action:&str,id:&str)->String{format!("{action}:{id}")}

pub(super) fn handles(a:&str)->bool {matches!(a,"chat_agents"|"trash"|"chat_delete_review"|"chat_delete"|"chat_restore"|"chat_purge_review"|"chat_purge"|"appearance_save"|"appearance_preview"|"appearance_reset"|"wallpaper_choose"|"wallpaper_result")||["chat_delete_choose:","chat_delete_review:","chat_delete:","chat_restore:","chat_purge_review:","chat_purge:"].iter().any(|p|a.starts_with(p))||a.starts_with("chat_agent:")||a.starts_with("chat_options:")||a.starts_with("trash_options:")}
pub(super) fn event(st:&mut Json,a:&str,inputs:&Json)->Result<Json>{
    if locked(st){return Err("Finish or cancel the current task or tool review first".into());}
    // A button's captured target must still match the current review. Old row
    // callbacks cannot silently act on a different conversation's confirmation.
    if let Some(id)=a.strip_prefix("chat_delete_choose:") {
        let mut review=st.clone();put(&mut review,"chat_target",s(id));target(&review,false)?;put(st,"chat_target",s(id));put(st,"dialog",s("chat_delete_review"));return Ok(Null);
    }
    let mut action=a;
    for name in ["chat_delete_review","chat_delete","chat_restore","chat_purge_review","chat_purge"] {
        if let Some(id)=a.strip_prefix(&format!("{name}:")) {
            if id.is_empty()||id!=text(st,"chat_target"){return Err("This conversation action is stale; select the conversation again".into());}
            action=name;break;
        }
    }
    let a=action;
    if a=="chat_agents"||a=="trash"{put(st,"dialog",s(a));return Ok(Null);}
    if let Some(raw)=a.strip_prefix("chat_agent:"){select_agent(st,raw.parse().map_err(|_|"Invalid agent")?)?;return Ok(Null);}
    for (prefix,deleted) in [("chat_options:",false),("trash_options:",true)] {if let Some(id)=a.strip_prefix(prefix){if !array(st,if deleted{"trash"}else{"chats"}).iter().any(|c|text(c,"id")==id){return Err("Conversation no longer exists".into());}put(st,"chat_target",s(id));put(st,"dialog",s(if deleted{"trash_options"}else{"chat_options"}));return Ok(Null);}}
    match a {
        "chat_delete_review"=>{target(st,false)?;if text(st,"dialog")!="chat_options"{return Err("Choose a conversation first".into());}put(st,"dialog",s("chat_delete_review"));},
        "chat_delete"=>{
            if text(st,"dialog")!="chat_delete_review"{return Err("Confirm this conversation first".into());}
            let mut c=target(st,false)?;let id=text(&c,"id");let mut trash=array(st,"trash");
            if trash.len()>=200{return Err("Trash is full. Restore or permanently delete an item first".into());}
            if trash.iter().any(|c|text(c,"id")==id){return Err("Conversation identity already exists in Trash; no conversation was changed".into());}
            let was_active=text(st,"active")==id;
            if was_active{put(&mut c,"draft",s(&text(st,"draft")));}
            let remaining:Vec<Json>=array(st,"chats").into_iter().filter(|c|text(c,"id")!=id).collect();
            if was_active {
                // Selecting a surviving conversation preserves its own saved draft.
                // Empty History stays empty; append/new creates a chat on demand.
                let next=remaining.first().cloned().unwrap_or(Null);
                put(st,"active",s(&text(&next,"id")));put(st,"draft",s(&text(&next,"draft")));
            }
            put(st,"history_empty_intentional",Bool(remaining.is_empty()));
            trash.push(c);put(st,"trash",Arr(trash));put(st,"chats",Arr(remaining));
            put(st,"page",s("history"));put(st,"dialog",Null);put(st,"chat_target",Null);
            put(st,"request_chat",Null);put(st,"request_model",Null);
            put(st,"history_status",s("Conversation moved to Trash. You can restore it from Trash."));put(st,"notice",s(""));
        },
        "chat_restore"=>{if text(st,"dialog")!="trash_options"{return Err("Choose a trashed conversation first".into());}let c=target(st,true)?;if array(st,"chats").len()>=200{return Err("History is full".into());}let id=text(&c,"id");if array(st,"chats").iter().any(|v|text(v,"id")==id){return Err("Conversation identity already exists".into());}if chat(st)==Null{put(st,"active",s(&id));put(st,"draft",s(&text(&c,"draft")));}replace_chat(st,c);put(st,"history_empty_intentional",Bool(false));put(st,"trash",Arr(array(st,"trash").into_iter().filter(|c|text(c,"id")!=id).collect()));put(st,"dialog",s("trash"));},
        "chat_purge_review"=>{target(st,true)?;if text(st,"dialog")!="trash_options"{return Err("Choose a trashed conversation first".into());}put(st,"dialog",s("chat_purge_review"));},
        "chat_purge"=>{if text(st,"dialog")!="chat_purge_review"{return Err("Confirm permanent deletion first".into());}let id=text(&target(st,true)?,"id");put(st,"trash",Arr(array(st,"trash").into_iter().filter(|c|text(c,"id")!=id).collect()));put(st,"dialog",s("trash"));},
        "appearance_save"|"appearance_preview"=>{let mut config=get(st,"appearance").clone();if config==Null{config=obj(vec![]);}for (k,options) in [("font",vec!["sans-serif","sans-serif-rounded","serif"]),("background",vec!["waves","solid","image"])]{let v=text(inputs,k);if !options.contains(&v.as_str()){return Err(format!("Choose {k}"));}put(&mut config,k,s(&v));}let size=text(inputs,"scale").parse::<usize>().map_err(|_|"Choose a text size")?;if ![100,115,130,150,175,200].contains(&size){return Err("Choose a text size".into());}put(&mut config,"scale",Num(size.to_string()));for k in ["accent","ink","surface"]{let v=text(inputs,k).trim().to_uppercase();if !v.is_empty()&&(v.len()!=7||!v.starts_with('#')||!v[1..].bytes().all(|b|b.is_ascii_hexdigit())){return Err(format!("{k}: enter #RRGGBB or leave blank"));}put(&mut config,k,s(&v));}let theme=text(inputs,"theme");if !["dark","light"].contains(&theme.as_str()){return Err("Choose a theme".into());}if a=="appearance_preview"{put(st,"appearance_preview",config);put(st,"preview_theme",s(&theme));}else{put(st,"appearance",config);put(st,"theme",s(&theme));put(st,"appearance_preview",Null);put(st,"dialog",Null);}put(st,"notice",s(""));},
        "appearance_reset"=>{put(st,"appearance",Null);put(st,"appearance_preview",Null);put(st,"theme",s("dark"));put(st,"form_inputs",Null);put(st,"form_dialog",Null);put(st,"dialog",s("appearance"));},
        "wallpaper_choose"=>{return Ok(obj(vec![("kind",s("wallpaper_picker"))]));},
        "wallpaper_result"=>{let uri=text(inputs,"uri");if !uri.starts_with("content://")||uri.len()>4096{return Err("Invalid image selection".into());}let mut cfg=get(st,"appearance").clone();if cfg==Null{cfg=obj(vec![]);}put(&mut cfg,"wallpaper",s(&uri));put(&mut cfg,"background",s("image"));put(st,"appearance",cfg);put(st,"appearance_preview",Null);put(st,"form_inputs",Null);put(st,"form_dialog",Null);put(st,"dialog",s("appearance"));},
        _=>return Err("Unknown conversation action".into())
    }Ok(Null)
}
pub(super) fn appearance(st:&Json)->Json {if get(st,"appearance_preview")!=&Null{get(st,"appearance_preview").clone()}else{get(st,"appearance").clone()}}
pub(super) fn theme(st:&Json)->String {if get(st,"appearance_preview")!=&Null{text(st,"preview_theme")}else{text(st,"theme")}}
pub(super) fn palette(st:&Json)->Json{let mut p=super::super::layout::palette(theme(st)=="dark");let c=appearance(st);for k in ["accent","ink","surface"]{let v=text(&c,k);if v.len()==7&&v.starts_with('#')&&v.as_bytes()[1..].iter().all(|b|b.is_ascii_hexdigit()){put(&mut p,k,s(&v));if k=="ink"{put(&mut p,"muted",s(&v));}if k=="surface"{for key in ["card","glass"]{put(&mut p,key,s(&format!("#E6{}",&v[1..])));}}}}p}
pub(super) fn modal_for(st:&Json)->Json {match text(st,"dialog").as_str(){
    "chat_agents"=>{let mut b=vec![button("Create agent profile","agent_new")];for(i,a)in array(st,"agents").iter().enumerate(){b.push(button(&format!("{}{}",if i==agent_index(st){"Selected · "}else{""},text(a,"name")),&format!("chat_agent:{i}")));}menu("Main agent",b)},
    "trash"=>{let mut b=vec![];for c in array(st,"trash"){b.push(button(&text(&c,"title"),&format!("trash_options:{}",text(&c,"id"))));}menu("Trash",b)},
    "chat_options"=>menu(&target(st,false).map(|c|text(&c,"title")).unwrap_or("Conversation".into()),vec![button("Open conversation",&format!("history:{}",text(st,"chat_target"))),button("Move to Trash",&scoped("chat_delete_review",&text(st,"chat_target")))]),
    "trash_options"=>menu(&target(st,true).map(|c|text(&c,"title")).unwrap_or("Conversation".into()),vec![button("Restore conversation",&scoped("chat_restore",&text(st,"chat_target"))),button("Delete permanently",&scoped("chat_purge_review",&text(st,"chat_target")))]),
    "chat_delete_review"|"chat_purge_review"=>{let permanent=text(st,"dialog")=="chat_purge_review";modal(if permanent{"Delete permanently?"}else{"Move to Trash?"},vec![field_ui("target","Conversation",&target(st,permanent).map(|c|text(&c,"title")).unwrap_or_default(),"readonly")],vec![button(if permanent{"Delete permanently"}else{"Move to Trash"},&scoped(if permanent{"chat_purge"}else{"chat_delete"},&text(st,"chat_target")))])},
    "appearance"=>{let a=appearance(st);let fallback=|key:&str,default:&str|{let v=text(&a,key);if v.is_empty(){default.to_string()}else{v}};modal("Appearance",vec![choice("theme","Theme",&theme(st),&[("Dark","dark"),("Light","light")]),choice("background","Background",&fallback("background","waves"),&[("Waves","waves"),("Solid color","solid"),("Selected image","image")]),choice("font","Font",&fallback("font","sans-serif"),&[("Sans serif","sans-serif"),("Rounded","sans-serif-rounded"),("Serif","serif")]),choice("scale","Text size",&if number(&a,"scale")==0{"100".into()}else{number(&a,"scale").to_string()},&[("100%","100"),("115%","115"),("130%","130"),("150%","150"),("175%","175"),("200%","200")]),field_ui("accent","Accent color (#RRGGBB; blank = theme)",&text(&a,"accent"),"text"),field_ui("ink","Text color (#RRGGBB; blank = theme)",&text(&a,"ink"),"text"),field_ui("surface","Background color (#RRGGBB; blank = theme)",&text(&a,"surface"),"text")],vec![button("Choose background image","wallpaper_choose"),button("Preview","appearance_preview"),button("Save appearance","appearance_save"),button("Reset appearance","appearance_reset")])},
    _=>Null
}}
