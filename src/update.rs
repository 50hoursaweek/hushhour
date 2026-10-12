//! The optional update check: asks GitHub where its "latest release" page points and
//! compares that release's version with this build's. Nothing about the user or their
//! documents is sent; the request is a plain page fetch with redirects turned off, so only
//! the `Location` header (`…/releases/tag/vX.Y.Z`) is read.

use std::time::Duration;

/// `https://github.com/50hoursaweek/hushhour/releases/latest` redirects to the newest release.
const LATEST: &str = "https://github.com/50hoursaweek/hushhour/releases/latest";
/// Checks run at most this often.
pub const INTERVAL_SECS: u64 = 24 * 60 * 60;

/// A release version: major, minor, patch.
pub type Version = (u64, u64, u64);

/// The newest release on GitHub: its version and its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub url: String,
}

impl Release {
    pub fn label(&self) -> String {
        let (a, b, c) = self.version;
        format!("{a}.{b}.{c}")
    }
}

/// This build's version.
pub fn current() -> Version {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or_default()
}

/// `"0.4.2"` or `"v0.4.2"`; anything else (a pre-release suffix, say) is not a release.
pub fn parse_version(s: &str) -> Option<Version> {
    let mut parts = s.strip_prefix('v').unwrap_or(s).split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// The release a `…/releases/tag/vX.Y.Z` address names.
pub fn release_at(location: &str) -> Option<Release> {
    let (_, tag) = location.rsplit_once("/releases/tag/")?;
    Some(Release { version: parse_version(tag)?, url: location.to_owned() })
}

/// Ask GitHub for the newest release. Blocks for up to ten seconds: run it off the UI thread.
pub fn fetch_latest() -> Result<Release, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent(concat!("HushHour/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let response = agent.get(LATEST).call().map_err(|e| e.to_string())?;
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("no redirect from GitHub (status {})", response.status()))?;
    release_at(location).ok_or_else(|| format!("unexpected release address {location}"))
}

/// [`fetch_latest`] on its own thread, as a future for `Task::perform`.
pub async fn check() -> Result<Release, String> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("hushhour-update".into())
        .spawn(move || {
            let _ = tx.send(fetch_latest());
        })
        .map_err(|e| e.to_string())?;
    rx.await.map_err(|_| "the update check stopped".to_owned())?
}

/// Seconds since the Unix epoch, for remembering when the last check ran.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Inside a Flatpak, updates come from Flatpak itself.
pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.4.2"), Some((0, 4, 2)));
        assert_eq!(parse_version("1.10.0"), Some((1, 10, 0)));
        assert_eq!(parse_version("v0.5.0-rc1"), None);
        assert_eq!(parse_version("v0.5"), None);
        assert_eq!(parse_version("v0.5.0.1"), None);
        assert!((0, 10, 0) > (0, 9, 9), "compared numerically, not as text");
        assert_eq!(current(), parse_version(env!("CARGO_PKG_VERSION")).unwrap());
    }

    #[test]
    fn release_addresses() {
        let r = release_at("https://github.com/50hoursaweek/hushhour/releases/tag/v0.4.2").unwrap();
        assert_eq!((r.version, r.label().as_str()), ((0, 4, 2), "0.4.2"));
        assert_eq!(r.url, "https://github.com/50hoursaweek/hushhour/releases/tag/v0.4.2");
        assert_eq!(release_at("https://github.com/50hoursaweek/hushhour/releases"), None);
    }
}
