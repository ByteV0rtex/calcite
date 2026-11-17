//! Is there a newer Calcite? One request to GitHub's public releases API,
//! nothing sent about the computer or the player.

use serde::Deserialize;

pub const RELEASES_API: &str = "https://api.github.com/repos/ByteV0rtex/calcite/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    /// The release page, where the installers are.
    pub url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The newest release if it's newer than `current`. No releases yet (404)
/// is not an error.
pub async fn check(http: &reqwest::Client, current: &str) -> anyhow::Result<Option<Update>> {
    let response = http
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release: Release = response.error_for_status()?.json().await?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    Ok(newer(&version, current).then_some(Update {
        version,
        url: release.html_url,
    }))
}

/// `1.2.10` > `1.2.9`; anything after a `-` (like `-beta`) is ignored.
pub fn newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.split('-')
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(candidate), parts(current));
    let len = a.len().max(b.len());
    let pad = |mut v: Vec<u64>| {
        v.resize(len, 0);
        v
    };
    pad(a) > pad(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("1.2.10", "1.2.9"));
        assert!(newer("1.1", "1.0.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert!(!newer("0.1.0-beta", "0.1.0"));
    }
}
