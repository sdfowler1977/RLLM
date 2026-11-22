//! Break one packed script line after `{`, `}`, and `;`.
//! A packed line longer than 4096 characters keeps that many and names the rest.
//! Packed evaluation is not expanded.

const PACKED_MIN: usize = 40;
const BODY_LIMIT: usize = 4096;
const NOTICE: &str = "The script was reformatted. Packed evaluation was not expanded.";

pub fn script_layout(source: &str) -> String {
    let (body, unread) = if source.contains('\n') || source.chars().nth(PACKED_MIN).is_none() {
        (source.to_string(), false)
    } else {
        break_packed(source)
    };
    with_notice(&body, unread)
}

fn break_packed(source: &str) -> (String, bool) {
    let mut body = String::new();
    let mut count = 0usize;
    let mut taken = 0usize;
    for c in source.chars() {
        if count >= BODY_LIMIT {
            break;
        }
        body.push(c);
        count += 1;
        taken += 1;
        if count < BODY_LIMIT && matches!(c, '{' | '}' | ';') {
            body.push('\n');
            count += 1;
        }
    }
    (body, source.chars().nth(taken).is_some())
}

fn with_notice(body: &str, unread: bool) -> String {
    let mut out = String::with_capacity(body.len() + NOTICE.len() + 48);
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
    if unread {
        out.push_str("later characters were not listed\n");
    }
    out.push_str(NOTICE);
    out
}

#[cfg(test)]
mod tests {
    use super::script_layout;

    #[test]
    fn short_fn() {
        let out = script_layout("function a(){return 1;}");
        assert!(out.contains("return 1"), "{out}");
        assert!(out.contains("not expanded"), "{out}");
    }

    #[test]
    fn short_and_multiline_stay_intact() {
        let short = "function a(){return 1;}";
        let out = script_layout(short);
        assert!(out.starts_with(short));
        assert!(out.ends_with(
            "The script was reformatted. Packed evaluation was not expanded."
        ));
        assert_eq!(out.matches('\n').count(), 1);

        let lines = "function a(){\nreturn 1;\n}";
        let kept = script_layout(lines);
        assert!(kept.starts_with(lines));
        assert!(kept.ends_with(
            "\nThe script was reformatted. Packed evaluation was not expanded."
        ));
    }

    #[test]
    fn packed_line_breaks_and_caps_the_body() {
        let packed = format!("function exampleName(){{var value=1;return value;}}{}", "x".repeat(20));
        assert!(packed.chars().count() > 40);
        assert!(!packed.contains('\n'));
        let out = script_layout(&packed);
        let body = out.strip_suffix(
            "The script was reformatted. Packed evaluation was not expanded.",
        ).expect("notice");
        assert!(body.contains("{\n"));
        assert!(body.contains(";\n"));
        assert!(body.contains("}\n"));
        assert!(body.chars().count() <= 4096);
        assert!(!out.contains("later characters were not listed"));

        let huge = "{".repeat(5000);
        let capped = script_layout(&huge);
        let capped_body = capped
            .strip_suffix("The script was reformatted. Packed evaluation was not expanded.")
            .expect("notice");
        let braces = capped_body
            .strip_suffix("later characters were not listed\n")
            .expect("unread");
        assert_eq!(braces.chars().count(), 4096);
    }
}
