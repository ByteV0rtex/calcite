//! Share a log as a link on mclo.gs, the paste site Minecraft players and
//! mod authors already use. Only runs when the player asks for it.

use anyhow::{Context as _, bail};
use serde::Deserialize;

/// mclo.gs keeps at most this many lines; the end of a log matters most.
const MAX_LINES: usize = 25_000;

#[derive(Debug, Deserialize)]
struct Reply {
    success: bool,
    url: Option<String>,
    error: Option<String>,
}

/// The last `MAX_LINES` lines of `text`.
pub fn tail(text: &str) -> &str {
    let mut seen = 0;
    for (ix, ch) in text.char_indices().rev() {
        if ch == '\n' {
            seen += 1;
            if seen > MAX_LINES {
                return &text[ix + 1..];
            }
        }
    }
    text
}

fn read_reply(body: &str) -> anyhow::Result<String> {
    let reply: Reply = serde_json::from_str(body).context("mclo.gs sent something unexpected")?;
    if !reply.success {
        bail!(
            "{}",
            reply
                .error
                .unwrap_or_else(|| "mclo.gs didn't accept the log".into())
        );
    }
    reply.url.context("mclo.gs didn't send a link back")
}

/// Upload `text` and return the link to it.
pub async fn mclogs(http: &reqwest::Client, text: &str) -> anyhow::Result<String> {
    let body = http
        .post("https://api.mclo.gs/1/log")
        .form(&[("content", tail(text))])
        .send()
        .await
        .context("couldn't reach mclo.gs")?
        .text()
        .await?;
    read_reply(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_end_of_long_logs() {
        let log: String = (0..MAX_LINES + 10).map(|i| format!("line {i}\n")).collect();
        let kept = tail(&log);
        assert_eq!(kept.lines().count(), MAX_LINES);
        assert!(kept.ends_with(&format!("line {}\n", MAX_LINES + 9)));
        assert_eq!(tail("short\nlog"), "short\nlog");
    }

    #[test]
    fn reads_replies() {
        assert_eq!(
            read_reply(r#"{"success":true,"id":"abc","url":"https://mclo.gs/abc","raw":"x"}"#)
                .unwrap(),
            "https://mclo.gs/abc"
        );
        let error =
            read_reply(r#"{"success":false,"error":"Required POST argument 'content' is empty."}"#)
                .unwrap_err();
        assert!(error.to_string().contains("empty"));
    }
}
