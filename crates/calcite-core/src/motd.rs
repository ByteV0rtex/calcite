//! Server descriptions (MOTDs) as coloured text runs.
//!
//! Servers send either a JSON text component (`{"text": …, "color": …,
//! "extra": […]}`) or a plain string with legacy `§` formatting codes, and
//! often a mix of both.

use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    /// 0xRRGGBB; `None` uses the default text colour.
    pub color: Option<u32>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Copy, Default)]
struct Style {
    color: Option<u32>,
    bold: bool,
    italic: bool,
}

/// The 16 named colours (also the `§0`–`§f` codes, in order).
const NAMED: [(&str, u32); 16] = [
    ("black", 0x000000),
    ("dark_blue", 0x0000AA),
    ("dark_green", 0x00AA00),
    ("dark_aqua", 0x00AAAA),
    ("dark_red", 0xAA0000),
    ("dark_purple", 0xAA00AA),
    ("gold", 0xFFAA00),
    ("gray", 0xAAAAAA),
    ("dark_gray", 0x555555),
    ("blue", 0x5555FF),
    ("green", 0x55FF55),
    ("aqua", 0x55FFFF),
    ("red", 0xFF5555),
    ("light_purple", 0xFF55FF),
    ("yellow", 0xFFFF55),
    ("white", 0xFFFFFF),
];

fn named(name: &str) -> Option<u32> {
    if let Some(hex) = name.strip_prefix('#') {
        return u32::from_str_radix(hex, 16).ok();
    }
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Parse a server description into runs of styled text.
pub fn parse(description: &Value) -> Vec<Span> {
    let mut out = Vec::new();
    walk(description, Style::default(), &mut out);
    merge(out)
}

fn walk(value: &Value, inherited: Style, out: &mut Vec<Span>) {
    match value {
        Value::String(s) => legacy(s, inherited, out),
        Value::Array(parts) => {
            for part in parts {
                walk(part, inherited, out);
            }
        }
        Value::Object(map) => {
            let mut style = inherited;
            if let Some(c) = map.get("color").and_then(Value::as_str) {
                style.color = named(c).or(style.color);
            }
            if let Some(b) = map.get("bold").and_then(Value::as_bool) {
                style.bold = b;
            }
            if let Some(i) = map.get("italic").and_then(Value::as_bool) {
                style.italic = i;
            }
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                legacy(text, style, out);
            } else if let Some(text) = map.get("translate").and_then(Value::as_str) {
                legacy(text, style, out);
            }
            if let Some(extra) = map.get("extra") {
                walk(extra, style, out);
            }
        }
        _ => {}
    }
}

/// Text with `§` codes: colours reset formatting, `§r` resets everything.
fn legacy(text: &str, start: Style, out: &mut Vec<Span>) {
    let mut style = start;
    let mut current = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            current.push(c);
            continue;
        }
        let Some(code) = chars.next() else { break };
        if !current.is_empty() {
            out.push(span(std::mem::take(&mut current), style));
        }
        match code.to_ascii_lowercase() {
            d @ ('0'..='9' | 'a'..='f') => {
                let ix = d.to_digit(16).unwrap_or(15) as usize;
                style = Style {
                    color: Some(NAMED[ix].1),
                    ..Style::default()
                };
            }
            'l' => style.bold = true,
            'o' => style.italic = true,
            'r' => style = start,
            _ => {} // obfuscated, strikethrough, underline: shown plainly
        }
    }
    if !current.is_empty() {
        out.push(span(current, style));
    }
}

fn span(text: String, style: Style) -> Span {
    Span {
        text,
        color: style.color,
        bold: style.bold,
        italic: style.italic,
    }
}

/// Join neighbours with the same style.
fn merge(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for s in spans {
        if let Some(last) = out.last_mut()
            && last.color == s.color
            && last.bold == s.bold
            && last.italic == s.italic
        {
            last.text.push_str(&s.text);
            continue;
        }
        out.push(s);
    }
    out
}

/// The description without formatting, on one line.
pub fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split runs at line breaks (MOTDs have up to two lines).
pub fn lines(spans: &[Span]) -> Vec<Vec<Span>> {
    let mut out = vec![Vec::new()];
    for s in spans {
        for (i, part) in s.text.split('\n').enumerate() {
            if i > 0 {
                out.push(Vec::new());
            }
            if !part.is_empty() {
                out.last_mut().unwrap().push(Span {
                    text: part.to_string(),
                    ..s.clone()
                });
            }
        }
    }
    out.retain(|l| l.iter().any(|s| !s.text.trim().is_empty()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_legacy_codes() {
        let spans = parse(&json!("§aGreen §lBold§r plain"));
        assert_eq!(
            spans[0],
            Span {
                text: "Green ".into(),
                color: Some(0x55FF55),
                bold: false,
                italic: false
            }
        );
        assert_eq!(
            spans[1],
            Span {
                text: "Bold".into(),
                color: Some(0x55FF55),
                bold: true,
                italic: false
            }
        );
        assert_eq!(spans[2].text, " plain");
        assert_eq!(spans[2].color, None);
    }

    #[test]
    fn reads_json_components_with_inheritance() {
        let spans = parse(&json!({
            "text": "",
            "extra": [
                {"text": "Hypixel ", "color": "green", "extra": [{"text": "Network", "bold": true}]},
                {"text": " [1.8-1.21]", "color": "#FF0000"}
            ]
        }));
        assert_eq!(plain(&spans), "Hypixel Network [1.8-1.21]");
        assert_eq!(spans[1].text, "Network");
        assert_eq!(spans[1].color, Some(0x55FF55));
        assert!(spans[1].bold);
        assert_eq!(spans[2].color, Some(0xFF0000));
    }

    #[test]
    fn splits_two_line_motds() {
        let spans = parse(&json!("§6Line one\n§7Line two"));
        let l = lines(&spans);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0][0].text, "Line one");
        assert_eq!(l[1][0].color, Some(0xAAAAAA));
    }
}
