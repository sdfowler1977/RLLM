use super::*;

fn value(v: &Json, name: &str, fallback: usize) -> usize {
    match v.get(name) {
        Ok(Num(n)) | Ok(Str(n)) => n.parse().unwrap_or(fallback),
        _ => fallback,
    }
}
pub fn plan(v: &Json) -> Result<Json> {
    let width = value(v, "width", 344).clamp(160, 16384);
    let height = value(v, "height", 560).clamp(48, 16384);
    let small = value(v, "small_line", 14).clamp(8, 128);
    let body = value(v, "body_line", 20).clamp(10, 192);
    let title = value(v, "title_line", 26).clamp(12, 192);
    let tab = value(v, "tab_width", 80).clamp(48, 1024);
    let chat = v.get("chat") == Ok(&Bool(true));
    let short_chat=chat && height<260;
    let composer_line=value(v,"composer_line",body).clamp(10,192);
    let composer_height=(composer_line+if short_chat {8}else{28}).max(56);
    // Phone-first layout: configuration is a separate route, never a side panel.
    let config=0;let gap=0;let main=width;
    let icons=true;let columns=5;let nav_height=48;
    let header=if short_chat && height<180.max(composer_height+48) {0}else{(title+12).clamp(48,72)};let status=small+8;
    let footer=if chat {(body+24).max(56)}else{0};
    let content=height.saturating_sub(header+status+footer+48).max(48);
    let rows=(content/(body*2+24).max(48)).clamp(1,6);
    let preview_lines=(content/rows).saturating_sub(12)/body;
    let compact=false;let inline_count=false;let monitor_lines=2;let config_height=0;
    let _=tab;
    Ok(obj(vec![
        ("short_chat", Bool(short_chat)),
        ("chat_status_height", Num(if short_chat {0}else{48}.to_string())),
        ("chrome_gap", Num(if short_chat {0}else{8}.to_string())),
        ("conversation_header_height", Num(if height>=420 && small<=24 {(small+16).max(36)}else{0}.to_string())),
        ("config_width", Num(config.to_string())),
        ("main_width", Num(main.to_string())),
        ("gap", Num(gap.to_string())),
        ("header_height", Num(header.to_string())),
        ("nav_columns", Num(columns.to_string())),
        ("nav_height", Num(nav_height.to_string())),
        ("nav_icons", Bool(icons)),
        ("compose_icon", Bool(main < 96 + body * 4)),
        ("config_icon", Bool(small > 26)),
        ("status_height", Num(status.to_string())),
        ("rows", Num(rows.to_string())),
        ("preview_lines", Num(preview_lines.to_string())),
        ("content_height", Num(content.to_string())),
        ("config_compact", Bool(compact)),
        ("config_inline_count", Bool(inline_count)),
        ("monitor_lines", Num(monitor_lines.to_string())),
        ("config_height", Num(config_height.to_string())),
        ("small_line", Num(small.to_string())),
    ]))
}

#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn landscape_keyboard_prioritizes_composer_over_chrome() {
        for height in [80,104,112,160] {
            for line in [20,42,58] {
                let plan=plan(&obj(vec![("height",Num(height.to_string())),("chat",Bool(true)),("composer_line",Num(line.to_string()))])).unwrap();
                assert_eq!(value(&plan,"header_height",99),0);
                assert_eq!(value(&plan,"chat_status_height",99),0);
                assert_eq!(value(&plan,"conversation_header_height",99),0);
                assert_eq!(value(&plan,"chrome_gap",99),0);
                assert!(plan.get("short_chat")==Ok(&Bool(true)));
                assert!((line+8).max(56)<=height);
            }
        }
    }
    #[test]
    fn phone_and_tablet_controls_fit_measured_text() {
        for (width, height) in [(304, 552), (624, 252), (784, 1080), (1184, 700)] {
            for scale in [100, 130, 200] {
                for chat in [false, true] {
                    let p = plan(&obj(vec![
                        ("width", Num(width.to_string())), ("height", Num(height.to_string())),
                        ("small_line", Num((12 * scale / 100).to_string())),
                        ("body_line", Num((18 * scale / 100).to_string())),
                        ("title_line", Num((24 * scale / 100).to_string())),
                        ("tab_width", Num((72 * scale / 100).to_string())), ("chat", Bool(chat)),
                    ])).unwrap();
                    assert!(value(&p,"config_height",usize::MAX) <= height, "{width}x{height} at {scale}%");
                    assert!(value(&p,"content_height",0) >= 48, "{width}x{height} at {scale}%");
                    assert!(value(&p,"rows",0) > 0);
                    assert_eq!(value(&p,"main_width",0),width);
                    assert_eq!(value(&p,"config_width",99),0);
                }
            }
        }
    }
}

/// Product-owned palette for the translucent phone surfaces; Android binds it
/// to native drawables without changing navigation or saved conversations.
pub fn palette(dark: bool) -> Json {
    let colors=if dark {[
        ("ink","#F7FAFF"),("surface","#050910"),("card","#B31A3242"),
        ("muted","#A2BCC5"),("accent","#4EDFEA"),("glass","#BF0F1724"),
        ("border","#557AC7D8"),("veil","#66050910"),
    ]}else{[
        ("ink","#182737"),("surface","#F4F7F9"),("card","#E6FFFFFF"),
        ("muted","#415D6B"),("accent","#00696E"),("glass","#DEEFF6F7"),
        ("border","#887B999F"),("veil","#99EFF7F8"),
    ]};
    let mut result=obj(colors.into_iter().map(|(name,value)|(name,s(value))).collect());
    if let Obj(fields)=&mut result {for (key,value) in if dark {[("violet","#A78BFA"),("success","#34D399"),("warning","#FBBF24"),("error","#FB7185")]}else{[("violet","#6941A5"),("success","#08784E"),("warning","#946000"),("error","#B42340")]}{fields.insert(key.into(),s(value));}}
    result
}
