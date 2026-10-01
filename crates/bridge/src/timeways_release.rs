//! The programs of Timeways from its GitHub release (SPEC.md 11.4): the manifest, the
//! archive of this computer, and its SHA-256 sums. Every asset name is here.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use protocol::apps::App;
use protocol::version::{VersionFit, version_fit};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::download_failure::download;
use crate::update::{Replaced, install_program, parse_sum, target, tool};

pub const RELEASES: &str = "https://github.com/eserilev/timeways/releases/latest/download";
/// Changes the release folder, as `GNOMISH_URL` does for the desktop app.
pub const URL_VAR: &str = "TIMEWAYS_URL";
pub const MANIFEST: &str = "timeways-manifest.json";
pub const SUMS: &str = "SHA256SUMS";
pub const STORY: &str = "timeways-story";
pub const PACK: &str = "timeways-pack";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// The `ns.App.version` of the Timeways addon of this release (SPEC.md 7.7).
    pub app_version: u32,
    pub targets: BTreeMap<String, Target>,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    pub asset: String,
    pub sha256: String,
    pub programs: Vec<String>,
}

/// No folder, no `..`, and no hidden file: the name comes from the network.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', ':'])
}

/// The entry of this computer, with plain names and both programs.
pub fn target_of<'a>(manifest: &'a Manifest, target: &str) -> Result<&'a Target> {
    let entry = manifest
        .targets
        .get(target)
        .context("Timeways has no build for this computer yet.")?;
    if !is_plain_name(&entry.asset) || !entry.programs.iter().all(|p| is_plain_name(p)) {
        bail!("the Timeways manifest names a file in another folder");
    }
    for program in [STORY, PACK] {
        if !entry.programs.iter().any(|p| p == program) {
            bail!("the Timeways release has no {program}");
        }
    }
    Ok(entry)
}

/// The addon of the release must speak the version range of this desktop app.
pub fn check_app_version(manifest: &Manifest) -> Result<()> {
    match version_fit(App::Timeways, manifest.app_version) {
        VersionFit::Supported => Ok(()),
        VersionFit::TooNew => {
            bail!("This Timeways needs a newer desktop app. Run gnomish-relay update first.")
        }
        VersionFit::TooOld => bail!("This Timeways is older than this desktop app supports."),
    }
}

pub fn parse_manifest(text: &str) -> Result<Manifest> {
    serde_json::from_str(text).context("the Timeways manifest is damaged")
}

/// The sum of `name` in a `sha256sum` file. `*name` is the binary mode of `sha256sum`.
pub fn sum_in(sums: &str, name: &str) -> Option<[u8; 32]> {
    sums.lines()
        .find(|line| {
            let file = line.split_whitespace().nth(1).unwrap_or_default();
            file.trim_start_matches('*') == name
        })
        .and_then(parse_sum)
}

/// The archive must match the manifest and `SHA256SUMS`, which come from the same
/// release. They find a broken download, not a changed release (SPEC.md 11.3).
fn check_sums(archive: &Path, entry: &Target, sums: &str) -> Result<()> {
    let have: [u8; 32] = Sha256::digest(fs::read(archive)?).into();
    let in_manifest = parse_sum(&entry.sha256);
    let in_sums = sum_in(sums, &entry.asset);
    if in_manifest != Some(have) || in_sums != Some(have) {
        bail!("the download of {} has a wrong SHA-256 sum", entry.asset);
    }
    Ok(())
}

fn program_file(name: &str) -> String {
    let suffix = std::env::consts::EXE_SUFFIX;
    if name.ends_with(suffix) {
        name.to_owned()
    } else {
        format!("{name}{suffix}")
    }
}

/// The unpacked programs of the latest release, in `dir`.
pub struct Download {
    pub version: String,
    /// Each program name with its unpacked file.
    pub programs: Vec<(String, PathBuf)>,
}

/// Downloads the manifest, the sums, and the archive of this computer from `base` into
/// `dir`, checks them, and unpacks the archive.
pub fn fetch(base: &str, dir: &Path) -> Result<Download> {
    let target = target().context("Timeways has no build for this computer yet.")?;
    download(&format!("{base}/{MANIFEST}"), &dir.join(MANIFEST))?;
    download(&format!("{base}/{SUMS}"), &dir.join(SUMS))?;
    let manifest = parse_manifest(&fs::read_to_string(dir.join(MANIFEST))?)?;
    check_app_version(&manifest)?;
    let entry = target_of(&manifest, target)?;
    let archive = dir.join(&entry.asset);
    download(&format!("{base}/{}", entry.asset), &archive)?;
    check_sums(&archive, entry, &fs::read_to_string(dir.join(SUMS))?)?;
    let unpacked = dir.join("unpacked");
    fs::create_dir_all(&unpacked)?;
    // The tar of Windows (bsdtar) also unpacks a zip.
    let (from, to) = (archive.to_string_lossy(), unpacked.to_string_lossy());
    tool("tar", &["-xf", &from, "-C", &to])?;
    // Another name in the list goes nowhere: `bin` also holds the desktop app, and on
    // the PATH it can hide a tool such as git.
    let mut programs = Vec::new();
    for name in [STORY, PACK] {
        let file = unpacked.join(program_file(name));
        if !file.is_file() {
            bail!("{} has no {}", entry.asset, program_file(name));
        }
        programs.push((name.to_owned(), file));
    }
    Ok(Download {
        version: manifest.version,
        programs,
    })
}

/// Puts each program of `download` into `bin`. Returns the programs that changed.
pub fn install(download: &Download, bin: &Path) -> Result<Vec<String>> {
    let mut changed = Vec::new();
    for (name, file) in &download.programs {
        if install_program(&bin.join(program_file(name)), file)? == Replaced::New {
            changed.push(name.clone());
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        parse_manifest(json).unwrap()
    }

    const GOOD: &str = r#"{"version": "0.1.0", "tag": "v0.1.0", "app_version": 1,
        "targets": {"x": {"asset": "timeways-x.tar.gz", "sha256": "00",
        "programs": ["timeways-story", "timeways-pack"]}},
        "addon": {"asset": "timeways-addon.zip", "sha256": "00"}}"#;

    #[test]
    fn the_manifest_gives_the_archive_and_the_programs_of_a_target() {
        let manifest = manifest(GOOD);

        let entry = target_of(&manifest, "x").unwrap();

        assert_eq!(entry.asset, "timeways-x.tar.gz");
        assert_eq!(entry.programs, [STORY, PACK]);
        assert_eq!(manifest.version, "0.1.0");
    }

    #[test]
    fn a_target_with_no_build_says_so() {
        let error = target_of(&manifest(GOOD), "y").unwrap_err();
        assert!(error.to_string().contains("no build for this computer"));
    }

    #[test]
    fn a_name_with_a_folder_is_refused() {
        for bad in ["../x.tar.gz", "a/b", "a\\b", ".hidden", "", "c:x"] {
            let json = GOOD.replace("timeways-x.tar.gz", &bad.replace('\\', "\\\\"));
            assert!(target_of(&manifest(&json), "x").is_err(), "{bad}");
        }
        let program = GOOD.replace("\"timeways-pack\"]", "\"timeways-pack\", \"../sh\"]");
        assert!(target_of(&manifest(&program), "x").is_err());
    }

    #[test]
    fn a_release_with_no_pack_program_is_refused() {
        let json = GOOD.replace(", \"timeways-pack\"", "");
        let error = target_of(&manifest(&json), "x").unwrap_err();
        assert!(error.to_string().contains("no timeways-pack"), "{error}");
    }

    #[test]
    fn an_addon_version_out_of_range_stops_the_install() {
        let newer = GOOD.replace("\"app_version\": 1", "\"app_version\": 99");
        let older = GOOD.replace("\"app_version\": 1", "\"app_version\": 0");

        assert!(check_app_version(&manifest(GOOD)).is_ok());
        let error = check_app_version(&manifest(&newer)).unwrap_err();
        assert!(
            error.to_string().contains("gnomish-relay update"),
            "{error}"
        );
        assert!(check_app_version(&manifest(&older)).is_err());
    }

    #[test]
    fn a_damaged_manifest_is_an_error() {
        assert!(parse_manifest("{").is_err());
        assert!(parse_manifest(r#"{"version": "1"}"#).is_err());
    }

    #[test]
    fn the_sum_of_a_name_comes_from_its_own_line() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let sums = format!("{a}  timeways-x.tar.gz\n{b} *timeways-addon.zip\n");

        assert_eq!(sum_in(&sums, "timeways-x.tar.gz"), parse_sum(&a));
        assert_eq!(sum_in(&sums, "timeways-addon.zip"), parse_sum(&b));
        assert_eq!(sum_in(&sums, "timeways-y.tar.gz"), None);
    }

    #[test]
    fn a_program_name_gets_the_suffix_of_this_os_once() {
        let suffix = std::env::consts::EXE_SUFFIX;
        assert_eq!(program_file(STORY), format!("timeways-story{suffix}"));
        assert_eq!(
            program_file(&format!("timeways-story{suffix}")),
            format!("timeways-story{suffix}")
        );
    }
}
