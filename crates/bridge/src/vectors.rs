//! Golden vectors: screenshots of known strips from the real game, signed with a public
//! test key (SPEC.md 14.3). The self-test addon draws them, and `selftest collect` keeps them.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use protocol::frame::{Frame, TAG_LEN, decode_frame, encode_frame, signed_len};
use serde::{Deserialize, Serialize};

use crate::ids::hex;
use crate::receive::StripKey;
use crate::saved::from_hex;
use crate::strip::{Image, read_with};

/// PUBLIC. It signs only the test strips of the self-test addon, never a message.
pub const TEST_KEY: &[u8; 32] = b"gnomish-relay public test key 01";
pub const MANIFEST: &str = "manifest.json";
/// A PNG of 4096 by 4096 pixels (`strip::MAX_SIDE`) with no compression is below this.
const MAX_PNG: u64 = 80 * 1024 * 1024;

/// One strip that the self-test addon drew, as its results list it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Shot {
    pub kind: String,
    pub name: String,
    pub frame_id: u16,
    pub time: u32,
    /// Hex.
    pub payload: String,
    /// `time()` in the game at the `Screenshot()` call. `None` when no call came.
    pub unix: Option<u32>,
    pub ui_parent_scale: Option<f64>,
    pub strip_effective_scale: Option<f64>,
    /// The line mode of a shot of kind `line` (SPEC.md 7.1.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u8>,
}

/// One committed screenshot, and the frame that it holds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vector {
    pub file: String,
    #[serde(flatten)]
    pub shot: Shot,
    pub width: usize,
    pub height: usize,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Manifest {
    pub build: String,
    /// Hex of `TEST_KEY`, so a reader of the file sees the key that it needs.
    pub key: String,
    pub vectors: Vec<Vector>,
}

fn test_key() -> Result<StripKey> {
    StripKey::from_hex(&hex(TEST_KEY))
}

fn tag_checks(key: &StripKey, bytes: &[u8]) -> bool {
    decode_frame(bytes).is_ok_and(|frame| key.tag(&bytes[..signed_len(&frame)]) == frame.tag)
}

/// A strip that the self-test addon drew. `selftest collect` needs it later.
pub fn is_test_strip(bytes: &[u8]) -> bool {
    test_key().is_ok_and(|key| tag_checks(&key, bytes))
}

/// The frame of a test strip in the image, if its tag checks under the test key.
pub fn test_frame(image: &Image) -> Result<Option<Frame>> {
    let key = test_key()?;
    let Some(bytes) = read_with(image, |bytes| tag_checks(&key, bytes)) else {
        return Ok(None);
    };
    if !tag_checks(&key, &bytes) {
        return Ok(None);
    }
    Ok(decode_frame(&bytes).ok())
}

/// The smallest top-left corner of `image` that still holds its test strip. The rest of
/// a screenshot shows the screen of the player, with the character name, so a vector
/// keeps only this corner.
pub fn strip_corner(image: &Image) -> Result<Image> {
    let (width, height) = image.size();
    let holds = |w, h| -> Result<bool> { Ok(test_frame(&image.crop(0, 0, w, h))?.is_some()) };
    if !holds(width, height)? {
        bail!("the screenshot holds no test strip");
    }
    let height = least(height, |h| holds(width, h))?;
    let width = least(width, |w| holds(w, height))?;
    Ok(image.crop(0, 0, width, height))
}

/// The least `n` in `1..=max` where `holds(n)`, for a `holds` that stays true above it.
fn least(max: usize, holds: impl Fn(usize) -> Result<bool>) -> Result<usize> {
    let (mut low, mut high) = (1, max);
    while low < high {
        let mid = low + (high - low) / 2;
        if holds(mid)? {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    Ok(high)
}

/// The signed frame that the self-test drew for `shot`.
pub fn frame_of(shot: &Shot) -> Result<Vec<u8>> {
    let payload = from_hex(&shot.payload).context("the payload of a shot is not hex")?;
    let mut wire = encode_frame(shot.time, shot.frame_id, &payload, [0; 8])
        .context("the payload of a shot is too long")?;
    let signed = wire.len() - TAG_LEN;
    let tag = test_key()?.tag(&wire[..signed]);
    wire[signed..].copy_from_slice(&tag);
    Ok(wire)
}

/// A PNG that is a plain file under the size limit. A link is refused.
pub fn read_png(path: &Path) -> Result<Image> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_PNG {
        bail!(
            "{} is not a plain file under the size limit",
            path.display()
        );
    }
    Image::from_png(&fs::read(path)?)
}

/// The shot that drew this frame. The time, the id, and the payload must all match.
pub fn shot_of<'a>(frame: &Frame, shots: &'a [Shot]) -> Option<&'a Shot> {
    shots.iter().find(|shot| {
        shot.frame_id == frame.frame_id
            && shot.time == frame.time
            && from_hex(&shot.payload).as_deref() == Some(frame.payload.as_slice())
    })
}

/// Decodes one committed vector with the real reader, and compares it with its manifest entry.
pub fn check(dir: &Path, vector: &Vector) -> Result<()> {
    let image = read_png(&dir.join(&vector.file))?;
    if image.size() != (vector.width, vector.height) {
        bail!("{} has size {:?}", vector.file, image.size());
    }
    let frame = test_frame(&image)?
        .with_context(|| format!("{} holds no strip with a good test tag", vector.file))?;
    if shot_of(&frame, std::slice::from_ref(&vector.shot)).is_none() {
        bail!(
            "{} holds another frame than its manifest entry",
            vector.file
        );
    }
    Ok(())
}

pub fn read_manifest(dir: &Path) -> Result<Manifest> {
    let text = fs::read_to_string(dir.join(MANIFEST))
        .with_context(|| format!("cannot read {}", dir.join(MANIFEST).display()))?;
    let manifest: Manifest = serde_json::from_str(&text)?;
    if manifest.key != hex(TEST_KEY) {
        bail!("the manifest in {} names another key", dir.display());
    }
    Ok(manifest)
}

/// Checks every vector of the manifest in `dir`.
pub fn check_all(dir: &Path) -> Result<usize> {
    let manifest = read_manifest(dir)?;
    for vector in &manifest.vectors {
        check(dir, vector)?;
    }
    Ok(manifest.vectors.len())
}

#[cfg(test)]
mod tests {
    use protocol::frame::encode_frame;

    use super::*;

    fn shot(frame_id: u16, time: u32, payload: &[u8]) -> Shot {
        Shot {
            kind: "golden".into(),
            name: "n".into(),
            frame_id,
            time,
            payload: hex(payload),
            unix: None,
            ui_parent_scale: None,
            strip_effective_scale: None,
            mode: None,
        }
    }

    #[test]
    fn the_frame_of_a_shot_is_a_test_strip_that_matches_the_shot() {
        let line = shot(9, 1_790_211_079, b"a line");

        let wire = frame_of(&line).unwrap();

        assert!(is_test_strip(&wire));
        let Ok(frame) = decode_frame(&wire) else {
            panic!("the frame decodes");
        };
        assert_eq!(shot_of(&frame, std::slice::from_ref(&line)), Some(&line));
    }

    #[test]
    fn a_frame_matches_a_shot_only_by_time_id_and_payload_together() {
        let wire = encode_frame(5, 3, b"abc", [0; 8]).unwrap();
        let Ok(frame) = decode_frame(&wire) else {
            panic!("the frame decodes");
        };
        let shots = [
            shot(3, 5, b"abd"),
            shot(4, 5, b"abc"),
            shot(3, 6, b"abc"),
            shot(3, 5, b"abc"),
        ];
        assert_eq!(shot_of(&frame, &shots), Some(&shots[3]));
        assert_eq!(shot_of(&frame, &shots[..3]), None);
    }

    #[test]
    fn least_finds_the_first_size_that_holds() {
        assert_eq!(least(100, |n| Ok(n >= 37)).unwrap(), 37);
        assert_eq!(least(100, |_| Ok(true)).unwrap(), 1);
        assert_eq!(least(1, |_| Ok(true)).unwrap(), 1);
    }

    #[test]
    fn a_folder_or_a_missing_file_is_not_a_png() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_png(&dir.path().join("none.png")).is_err());
        assert!(read_png(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_read_as_a_png() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.png");
        fs::write(&target, b"not read").unwrap();
        let link = dir.path().join("link.png");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = read_png(&link).err().unwrap();
        assert!(error.to_string().contains("not a plain file"));
    }
}
