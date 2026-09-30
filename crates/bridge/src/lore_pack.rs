//! The lore pack of Timeways, built on this computer from the public Wowpedia dump
//! (SPEC.md 11.4). The pack is never shipped.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub const DUMP_URL: &str =
    "https://s3.amazonaws.com/wikia_xml_dumps/w/wo/wowpedia_pages_current.xml.7z";
/// Changes the dump, for a mirror or a test.
pub const DUMP_URL_VAR: &str = "TIMEWAYS_DUMP_URL";
pub const DUMP_FILE: &str = "wowpedia_pages_current.xml.7z";
pub const PACK_FILE: &str = "lore.sqlite";

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

fn size_of(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |m| m.len())
}

/// Downloads the dump into `dir` with `curl`, and calls `progress` with the bytes so far
/// a few times a second. A download that fails leaves no dump.
pub fn download_dump(url: &str, dir: &Path, mut progress: impl FnMut(u64)) -> Result<PathBuf> {
    let dump = dir.join(DUMP_FILE);
    let part = dir.join(format!("{DUMP_FILE}.part"));
    let mut curl = Command::new("curl")
        .args(["-fsSL", url, "-o"])
        .arg(&part)
        .spawn()
        .context("cannot run curl")?;
    let status = loop {
        if let Some(status) = curl.try_wait()? {
            break status;
        }
        progress(size_of(&part));
        std::thread::sleep(PROGRESS_EVERY);
    };
    if !status.success() {
        let _ = fs::remove_file(&part);
        bail!("the download of the Wowpedia lore failed");
    }
    progress(size_of(&part));
    fs::rename(&part, &dump)?;
    Ok(dump)
}

/// The last two lines of `timeways-pack`: "read N pages, skipped M" and "wrote N
/// passages to <path>". Every line before them names one page.
fn summary(stdout: &str) -> Vec<String> {
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(2);
    lines[from..].iter().map(|l| (*l).to_owned()).collect()
}

/// Runs `<pack program> from-dump <dump> <pack>.new`, then puts the new pack in place.
/// The program never writes over a file. On a failure the old pack stays.
pub fn build(pack_program: &Path, dump: &Path, pack: &Path) -> Result<Vec<String>> {
    let dir = pack.parent().context("the lore pack has no folder")?;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let new = pack.with_extension("sqlite.new");
    let _ = fs::remove_file(&new);
    let output = Command::new(pack_program)
        .arg("from-dump")
        .arg(dump)
        .arg(&new)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("cannot run {}", pack_program.display()))?;
    if !output.status.success() || !new.is_file() {
        let _ = fs::remove_file(&new);
        bail!("Couldn't build the Timeways lore. Your old lore stays.");
    }
    fs::rename(&new, pack)?;
    Ok(summary(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_two_lines_of_the_pack_program_show() {
        let out = "page Thrall\npage Jaina\nread 2 pages, skipped 0\nwrote 5 passages to /x\n\n";

        assert_eq!(
            summary(out),
            ["read 2 pages, skipped 0", "wrote 5 passages to /x"]
        );
        assert_eq!(summary("one\n"), ["one"]);
        assert!(summary("").is_empty());
    }
}
