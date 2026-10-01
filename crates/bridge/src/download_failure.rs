//! A failed `curl` download of Timeways: the reason in the words of the player, and the
//! details for the log (SPEC.md 11.4). The player never sees the command line of `curl`.

use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result};

/// Why a download failed, from the exit code and the error output of `curl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// HTTP 404: for a release, it isn't published yet.
    Missing,
    /// No DNS answer, no connection, a timeout, or a failed TLS start.
    Offline,
    Other,
}

/// The exit codes of `curl` for a network that does not answer.
const OFFLINE_CODES: [i32; 5] = [6, 7, 28, 35, 56];
/// `--fail` makes `curl` exit with 22 for an HTTP error.
const HTTP_ERROR: i32 = 22;

pub fn reason_of(code: Option<i32>, stderr: &str) -> Reason {
    match code {
        Some(HTTP_ERROR) if stderr.contains("404") => Reason::Missing,
        Some(code) if OFFLINE_CODES.contains(&code) => Reason::Offline,
        _ => Reason::Other,
    }
}

#[derive(Debug)]
pub struct DownloadFailed {
    pub reason: Reason,
    /// The URL, the exit code, and the error of `curl`, for the log.
    pub details: String,
}

impl DownloadFailed {
    pub fn of(url: &str, status: ExitStatus, stderr: &[u8]) -> DownloadFailed {
        let stderr = String::from_utf8_lossy(stderr);
        DownloadFailed {
            reason: reason_of(status.code(), &stderr),
            details: format!("the download of {url} failed ({status}): {}", stderr.trim()),
        }
    }
}

impl std::fmt::Display for DownloadFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self.reason {
            Reason::Missing => "the download isn't there",
            Reason::Offline => "no internet connection",
            Reason::Other => "the download failed",
        };
        f.write_str(text)
    }
}

impl std::error::Error for DownloadFailed {}

/// `curl -fsSL url -o to`, with its error output caught.
pub fn download(url: &str, to: &Path) -> Result<()> {
    let output = Command::new("curl")
        .args(["-fsSL", url, "-o"])
        .arg(to)
        .stdin(Stdio::null())
        .output()
        .context("cannot run curl")?;
    if output.status.success() {
        return Ok(());
    }
    Err(DownloadFailed::of(url, output.status, &output.stderr).into())
}

/// The details of a failed download in the chain of `error`.
pub fn details_of(error: &anyhow::Error) -> Option<&str> {
    let failed = error.downcast_ref::<DownloadFailed>()?;
    Some(&failed.details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_404_is_a_missing_download() {
        let stderr = "curl: (22) The requested URL returned error: 404";
        assert_eq!(reason_of(Some(22), stderr), Reason::Missing);
    }

    #[test]
    fn another_http_error_is_no_missing_download() {
        let stderr = "curl: (22) The requested URL returned error: 500";
        assert_eq!(reason_of(Some(22), stderr), Reason::Other);
    }

    #[test]
    fn no_dns_no_connection_and_a_timeout_mean_no_internet() {
        for code in [6, 7, 28, 35, 56] {
            assert_eq!(reason_of(Some(code), ""), Reason::Offline, "{code}");
        }
    }

    #[test]
    fn a_stop_by_a_signal_is_another_failure() {
        assert_eq!(reason_of(None, ""), Reason::Other);
        assert_eq!(reason_of(Some(37), ""), Reason::Other);
    }

    #[test]
    fn a_failed_download_names_the_reason_and_keeps_the_details_for_the_log() {
        let error = anyhow::Error::new(DownloadFailed {
            reason: Reason::Offline,
            details: "the download of https://x failed (exit status: 7): curl: (7)".into(),
        });

        assert_eq!(error.to_string(), "no internet connection");
        assert_eq!(
            details_of(&error),
            Some("the download of https://x failed (exit status: 7): curl: (7)")
        );
        assert_eq!(details_of(&anyhow::anyhow!("other")), None);
    }
}
