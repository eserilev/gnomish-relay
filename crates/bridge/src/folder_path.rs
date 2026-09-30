//! Paths as bytes in the form of the proved folder resolver (S5): parts split at `/`.

use std::path::{Path, PathBuf};

/// The proved resolver (S5) knows only `/`. Windows also splits at `\`, and
/// `canonicalize` there adds a `\\?\` prefix.
fn portable(path: &str, windows: bool) -> Vec<u8> {
    if !windows {
        return path.as_bytes().to_vec();
    }
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.replace('\\', "/").into_bytes()
}

/// A path in the form that the folder check takes, on every OS.
pub fn path_bytes(path: &Path) -> Vec<u8> {
    portable(&path.to_string_lossy(), cfg!(windows))
}

/// `canonicalize` with no `\\?\` before a drive on Windows, because git and many
/// other programs refuse that prefix.
pub fn real_path(path: &Path) -> std::io::Result<PathBuf> {
    let real = path.canonicalize()?;
    Ok(match real.to_str() {
        Some(text) => PathBuf::from(without_verbatim(text)),
        None => real,
    })
}

/// A network path keeps its prefix, because it has no other form with the same meaning.
fn without_verbatim(path: &str) -> &str {
    match path.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest,
        _ => path,
    }
}

/// The resolver starts its result with `/`. On Windows the drive comes first.
pub fn native_folder(resolved: Vec<u8>, windows: bool) -> Vec<u8> {
    match resolved.as_slice() {
        [b'/', _, b':', ..] if windows => resolved[1..].to_vec(),
        _ => resolved,
    }
}

/// The parts of a path in the form of `path_bytes`.
pub fn path_parts(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .collect()
}

/// Both paths in the form of `path_bytes`, so a `\\?\` prefix never makes a difference.
pub fn is_inside_folder(path: &[u8], folder: &[u8]) -> bool {
    path_parts(path).starts_with(&path_parts(folder))
}

/// The path from `base` to `target`, both resolved. The game sends it back, and it
/// resolves to `target` again. An absolute path can hold a drive, which the game
/// cannot send on Windows.
pub fn relative_folder(base: &[u8], target: &[u8]) -> Vec<u8> {
    let parts = |path: &'_ [u8]| -> Vec<Vec<u8>> {
        path.split(|&b| b == b'/')
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    };
    let (base, target) = (parts(base), parts(target));
    let same = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut out: Vec<Vec<u8>> = vec![b"..".to_vec(); base.len() - same];
    out.extend(target[same..].iter().cloned());
    out.join(&b'/')
}

/// A folder from the game, in the form that the resolver checks. On Windows a `\`
/// becomes `/`, so each `..` counts. A `:` never passes there: it starts a drive
/// or names a stream.
pub fn folder_request(raw: &[u8], windows: bool) -> Option<Vec<u8>> {
    if !windows {
        return Some(raw.to_vec());
    }
    if raw.contains(&b':') {
        return None;
    }
    Some(
        raw.iter()
            .map(|&b| if b == b'\\' { b'/' } else { b })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_folder_goes_down_from_the_base_or_up_and_over() {
        let rel = |base: &str, target: &str| {
            String::from_utf8(relative_folder(base.as_bytes(), target.as_bytes())).unwrap()
        };
        assert_eq!(rel("/home/x/Code", "/home/x/Code/app/src"), "app/src");
        assert_eq!(rel("/home/x/Code", "/home/x/Code"), "");
        assert_eq!(rel("/home/x/Code/a", "/home/x/Work/b"), "../../Work/b");
        assert_eq!(rel("/C:/Users/x", "/C:/Users/x/repo"), "repo");
    }

    #[test]
    fn a_relative_folder_resolves_back_to_its_target() {
        let roots = [b"/home/x".to_vec()];
        let base = b"/home/x/Code/a";
        let target = b"/home/x/Work/b";
        let rel = relative_folder(base, target);
        assert_eq!(
            protocol::folder::resolve_folder(&roots, base, &rel).as_deref(),
            Some(target.as_slice())
        );
    }

    #[test]
    fn a_windows_request_splits_at_backslashes_and_never_names_a_drive() {
        assert_eq!(folder_request(br"..\..\x", true).unwrap(), b"../../x");
        assert_eq!(folder_request(br"sub\dir", true).unwrap(), b"sub/dir");
        assert_eq!(folder_request(br"C:\Windows", true), None);
        assert_eq!(folder_request(b"file.txt:stream", true), None);
        assert_eq!(folder_request(br"a\b", false).unwrap(), br"a\b");
    }

    #[test]
    fn a_windows_folder_starts_with_its_drive() {
        assert_eq!(native_folder(b"/C:/Code/x".to_vec(), true), b"C:/Code/x");
        assert_eq!(native_folder(b"/home/x".to_vec(), true), b"/home/x");
        assert_eq!(native_folder(b"/C:/x".to_vec(), false), b"/C:/x");
    }

    #[test]
    fn a_verbatim_prefix_goes_only_before_a_drive() {
        assert_eq!(without_verbatim(r"\\?\C:\Code\app"), r"C:\Code\app");
        assert_eq!(
            without_verbatim(r"\\?\UNC\server\share"),
            r"\\?\UNC\server\share"
        );
        assert_eq!(without_verbatim("/home/x/Code"), "/home/x/Code");
    }

    #[test]
    fn a_real_path_resolves_dots_and_has_no_verbatim_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("a")).unwrap();

        let real = real_path(&tmp.path().join("a/..")).unwrap();

        assert_eq!(real, real_path(tmp.path()).unwrap());
        assert!(real.is_absolute());
        assert!(!real.to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn a_windows_root_loses_its_prefix_and_uses_slashes() {
        assert_eq!(portable(r"\\?\C:\Users\x\Code", true), b"C:/Users/x/Code");
        assert_eq!(portable(r"/home/x\y", false), br"/home/x\y");
    }
}
