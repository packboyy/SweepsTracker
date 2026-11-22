//! "Check for updates": asks GitHub for the repository's latest published *release* (not
//! the latest commit) and compares its version tag ("v0.2.0") with this build's version.
//! Only runs when asked (the title bar's version, or the app menu); nothing is sent but the
//! request itself. Releases are published with `scripts/release.sh`.

use eframe::egui;
use std::sync::mpsc::{self, Receiver};

pub const REPO: &str = "packboyy/SweepsTracker";
const USER_AGENT: &str = concat!("SweepsTracker/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// e.g. "v0.2.0"
    pub tag: String,
    /// The release's page on GitHub, with its download.
    pub url: String,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    UpToDate,
    Available(Release),
    /// The repository has no published releases yet.
    NoReleases,
}

/// What the title bar shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Idle,
    Checking,
    Available(Release),
}

/// Checks on a background thread; the result arrives on the receiver.
pub fn check_in_background(ctx: egui::Context) -> Receiver<Result<Outcome, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(check());
        ctx.request_repaint();
    });
    rx
}

fn check() -> Result<Outcome, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call();
    let json: serde_json::Value = match response {
        Ok(r) => r
            .into_string()
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))?,
        // GitHub answers 404 when there's no release.
        Err(ureq::Error::Status(404, _)) => return Ok(Outcome::NoReleases),
        Err(e) => return Err(e.to_string()),
    };
    let field = |name: &str| json[name].as_str().unwrap_or_default().to_string();
    let release = Release {
        tag: field("tag_name"),
        url: field("html_url"),
    };
    Ok(if is_newer(&release.tag, env!("CARGO_PKG_VERSION")) {
        Outcome::Available(release)
    } else {
        Outcome::UpToDate
    })
}

/// "v1.2.3" / "1.2.3" / "v1.2.3-beta" -> (1, 2, 3); missing parts count as 0.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>());
    let major = parts.next()?.ok()?;
    let minor = parts.next().unwrap_or(Ok(0)).ok()?;
    let patch = parts.next().unwrap_or(Ok(0)).ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// Whether release tag `tag` is a newer version than `current`. A tag that isn't a version
/// never counts as newer.
pub fn is_newer(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(tag), Some(current)) => tag > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0")); // numbers, not text
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(is_newer("v0.2.0-beta", "0.1.0"));
        assert!(!is_newer("latest", "0.1.0"));
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("v1.2.3.4", "0.1.0"));
        assert_eq!(parse_version("V2"), Some((2, 0, 0)));
    }
}
