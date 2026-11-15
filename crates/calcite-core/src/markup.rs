//! Project descriptions on Modrinth are Markdown mixed with HTML. This turns
//! the common HTML into Markdown and drops the rest, so a Markdown renderer
//! shows clean text instead of raw tags.

/// Convert a project description to plain Markdown.
pub fn clean_description(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        // HTML comments.
        if let Some(stripped) = after.strip_prefix("<!--") {
            rest = stripped
                .find("-->")
                .map(|e| &stripped[e + 3..])
                .unwrap_or("");
            continue;
        }
        let Some(end) = after.find('>') else {
            out.push_str(after);
            rest = "";
            break;
        };
        let tag = &after[1..end];
        rest = &after[end + 1..];
        let lower = tag.to_ascii_lowercase();
        let name: String = lower
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let closing = lower.starts_with('/');
        match name.as_str() {
            "br" => out.push('\n'),
            "p" | "div" | "center" | "details" | "table" | "tr" => {
                if !out.ends_with("\n\n") {
                    out.push_str("\n\n");
                }
            }
            "h1" | "h2" | "h3" | "h4" if !closing => {
                let level = name[1..].parse::<usize>().unwrap_or(2);
                out.push_str("\n\n");
                out.push_str(&"#".repeat(level));
                out.push(' ');
            }
            "h1" | "h2" | "h3" | "h4" => out.push_str("\n\n"),
            "summary" if !closing => out.push_str("**"),
            "summary" => out.push_str("**\n\n"),
            "b" | "strong" => out.push_str("**"),
            "i" | "em" => out.push('*'),
            "code" => out.push('`'),
            "li" if !closing => out.push_str("\n- "),
            "hr" => out.push_str("\n\n---\n\n"),
            "a" if !closing => {
                // Keep the link text and target as a Markdown link.
                if let Some(href) = attr(tag, "href")
                    && let Some(close) = rest.to_ascii_lowercase().find("</a>")
                {
                    let text = clean_description(&rest[..close]);
                    let text = text.trim();
                    if !text.is_empty() {
                        out.push_str(&format!("[{text}]({href})"));
                    }
                    rest = &rest[close + 4..];
                }
            }
            // Images, iframes and anything else are dropped; the gallery
            // tab shows a project's pictures.
            _ => {}
        }
    }
    out.push_str(rest);
    drop_markdown_images(&collapse_blank_lines(&out))
}

/// Remove `![alt](url)` images (including linked badges) from Markdown.
fn drop_markdown_images(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        let after = &rest[start..];
        let Some(mid) = after.find("](") else { break };
        let Some(close) = after[mid..].find(')') else {
            break;
        };
        // An image wrapped in a link: [![alt](img)](url)
        let mut prefix = &rest[..start];
        let mut skip = start + mid + close + 1;
        if prefix.ends_with('[')
            && rest[skip..].starts_with("](")
            && let Some(link_close) = rest[skip..].find(')')
        {
            prefix = &prefix[..prefix.len() - 1];
            skip += link_close + 1;
        }
        out.push_str(prefix);
        rest = &rest[skip..];
        // Don't leave a stray space where an image started a line.
        if (out.is_empty() || out.ends_with('\n')) && rest.starts_with(' ') {
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    collapse_blank_lines(&out)
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.trim().to_string()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))?;
    let value = &tag[at + name.len() + 1..];
    let quote = value.chars().next()?;
    if quote == '"' || quote == '\'' {
        let inner = &value[1..];
        inner.find(quote).map(|e| inner[..e].to_string())
    } else {
        Some(value.split_whitespace().next()?.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::clean_description;

    #[test]
    fn converts_common_html() {
        let input = "<center><img src=\"https://x/banner.png\"></center>\n<h2>Features</h2>\n<p>Fast <b>and</b> <a href=\"https://modrinth.com\">free</a>.</p><br>\n<!-- hidden -->Done";
        let out = clean_description(input);
        assert!(out.contains("## Features"), "{out}");
        assert!(
            out.contains("Fast **and** [free](https://modrinth.com)."),
            "{out}"
        );
        assert!(!out.contains('<'), "{out}");
        assert!(!out.contains("hidden"), "{out}");
        assert!(!out.contains("banner.png"), "{out}");
    }

    #[test]
    fn drops_markdown_images_and_badges() {
        let input = "Intro\n\n![banner](https://x/b.png)\n\n[![discord](https://x/d.svg)](https://discord.gg/x) Join us\n\nText";
        let out = clean_description(input);
        assert_eq!(out, "Intro\n\nJoin us\n\nText");
    }

    #[test]
    fn keeps_plain_markdown() {
        let input = "# Title\n\n- one\n- two\n\n`code` and **bold**";
        assert_eq!(clean_description(input), input);
    }
}
