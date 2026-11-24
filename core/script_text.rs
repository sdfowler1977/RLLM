//! Recover `\xNN` and `\uNNNN` escapes that appear inside a quoted string.
//! Every other source character is copied unchanged.
//! Recovered text is kept to 4096 characters; anything past that is named and omitted.
//! A regular-expression literal is not a separate mode.
//! Packed evaluation is not expanded. The script is not executed.

const BODY_LIMIT: usize = 4096;
const NOTICE: &str = "The script was not executed.";

pub fn script_text(source: &str) -> String {
    let (body, unread) = recover(source);
    with_notice(&body, unread)
}

enum Mode {
    Code,
    Line,
    Block,
    Quote(char),
}

fn recover(source: &str) -> (String, bool) {
    let mut body = String::new();
    let mut count = 0usize;
    let mut index = 0usize;
    let mut mode = Mode::Code;
    while index < source.len() {
        if count >= BODY_LIMIT {
            return (body, true);
        }
        let rest = &source[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        match mode {
            Mode::Line => {
                body.push(ch);
                count += 1;
                index += ch.len_utf8();
                if line_end(ch) {
                    mode = Mode::Code;
                }
            }
            Mode::Block => {
                if rest.starts_with("*/") {
                    body.push('*');
                    count += 1;
                    if count >= BODY_LIMIT {
                        return (body, true);
                    }
                    body.push('/');
                    count += 1;
                    index += 2;
                    mode = Mode::Code;
                } else {
                    body.push(ch);
                    count += 1;
                    index += ch.len_utf8();
                }
            }
            Mode::Code => {
                if rest.starts_with("//") {
                    body.push('/');
                    count += 1;
                    if count >= BODY_LIMIT {
                        return (body, true);
                    }
                    body.push('/');
                    count += 1;
                    index += 2;
                    mode = Mode::Line;
                } else if rest.starts_with("/*") {
                    body.push('/');
                    count += 1;
                    if count >= BODY_LIMIT {
                        return (body, true);
                    }
                    body.push('*');
                    count += 1;
                    index += 2;
                    mode = Mode::Block;
                } else if ch == '"' || ch == '\'' {
                    body.push(ch);
                    count += 1;
                    index += ch.len_utf8();
                    mode = Mode::Quote(ch);
                } else {
                    body.push(ch);
                    count += 1;
                    index += ch.len_utf8();
                }
            }
            Mode::Quote(quote) => {
                if ch == '\\' {
                    if let Some((decoded, consumed)) = hex_or_unicode(rest) {
                        body.push(decoded);
                        count += 1;
                        index += consumed;
                    } else {
                        body.push('\\');
                        count += 1;
                        index += 1;
                        if index >= source.len() {
                            continue;
                        }
                        if count >= BODY_LIMIT {
                            return (body, true);
                        }
                        let Some(next) = source[index..].chars().next() else {
                            continue;
                        };
                        body.push(next);
                        count += 1;
                        index += next.len_utf8();
                    }
                } else if ch == quote {
                    body.push(ch);
                    count += 1;
                    index += ch.len_utf8();
                    mode = Mode::Code;
                } else {
                    body.push(ch);
                    count += 1;
                    index += ch.len_utf8();
                }
            }
        }
    }
    (body, false)
}

fn line_end(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `\xNN` and `\uNNNN` inside the current string. A lone surrogate stays source text.
fn hex_or_unicode(rest: &str) -> Option<(char, usize)> {
    let bytes = rest.as_bytes();
    if bytes.len() >= 4
        && bytes[0] == b'\\'
        && bytes[1] == b'x'
        && hex_digit(bytes[2])
        && hex_digit(bytes[3])
    {
        let value = (hex_value(bytes[2]) << 4) | hex_value(bytes[3]);
        return Some((char::from(value), 4));
    }
    if bytes.len() >= 6
        && bytes[0] == b'\\'
        && bytes[1] == b'u'
        && (2..6).all(|slot| hex_digit(bytes[slot]))
    {
        let unit = hex4(&bytes[2..6]);
        if (0xD800..=0xDBFF).contains(&unit) {
            if bytes.len() >= 12
                && bytes[6] == b'\\'
                && bytes[7] == b'u'
                && (8..12).all(|slot| hex_digit(bytes[slot]))
            {
                let low = hex4(&bytes[8..12]);
                if (0xDC00..=0xDFFF).contains(&low) {
                    let scalar = 0x10000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32;
                    return Some((char::from_u32(scalar)?, 12));
                }
            }
            return None;
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return None;
        }
        return Some((char::from_u32(unit as u32)?, 6));
    }
    None
}

fn hex_digit(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

fn hex4(digits: &[u8]) -> u16 {
    ((hex_value(digits[0]) as u16) << 12)
        | ((hex_value(digits[1]) as u16) << 8)
        | ((hex_value(digits[2]) as u16) << 4)
        | hex_value(digits[3]) as u16
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
