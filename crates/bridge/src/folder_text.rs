//! The text of a chat folder that the game sends (SPEC.md 9.9): the home form `~/...`,
//! or the old form, which is relative to `default_cwd`.

use std::path::Path;

use protocol::folder::resolve_folder;

use crate::folder_path::{folder_request, native_folder, path_parts, relative_folder};

/// A new folder does not exist yet, so the step back to the home folder never finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wanted {
    Existing,
    New,
}

/// The base and the request that the resolver gets for a folder text of the game.
/// `base` and `home` are in the form of the resolver. A home form needs `home`.
pub fn to_resolve(
    raw: &[u8],
    base: &[u8],
    home: Option<&[u8]>,
    wanted: Wanted,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let request = folder_request(raw, cfg!(windows))?;
    if let Some(rest) = home_rest(&request) {
        return Some((home?.to_vec(), rest.to_vec()));
    }
    let Some(home) = home else {
        return Some((base.to_vec(), request));
    };
    if wanted == Wanted::Existing && saved_before_base_changed(base, home, &request) {
        return Some((home.to_vec(), request));
    }
    Some((base.to_vec(), request))
}

/// The rest after `~` of a home form.
fn home_rest(request: &[u8]) -> Option<&[u8]> {
    match request {
        [b'~'] => Some(&[]),
        [b'~', b'/', rest @ ..] => Some(rest),
        _ => None,
    }
}

/// An old text that names no folder from `base` but one from the home folder: setup
/// wrote `default_cwd = "~"`, and the user changed it later. From the home folder, only
/// plain names lead to a folder where a chat can work.
fn saved_before_base_changed(base: &[u8], home: &[u8], request: &[u8]) -> bool {
    has_only_names(request) && !is_folder(base, request) && is_folder(home, request)
}

fn has_only_names(request: &[u8]) -> bool {
    let parts = path_parts(request);
    let plain = |part: &&[u8]| *part != b"." && *part != b"..";
    !request.starts_with(b"/") && !parts.is_empty() && parts.iter().all(plain)
}

fn is_folder(base: &[u8], request: &[u8]) -> bool {
    let anywhere = [b"/".to_vec()];
    let Some(path) = resolve_folder(&anywhere, base, request) else {
        return false;
    };
    let native = native_folder(path, cfg!(windows));
    std::str::from_utf8(&native).is_ok_and(|path| Path::new(path).is_dir())
}

/// The text that the addon sends for a folder, so the tree lists only folders whose text
/// fits. On Windows, a folder on another drive than the home folder keeps the old form.
pub fn game_text(base: &[u8], home: Option<&[u8]>, resolved: &[u8]) -> Vec<u8> {
    let Some(home) = home else {
        return relative_folder(base, resolved);
    };
    let rest = relative_folder(home, resolved);
    if rest.contains(&b':') {
        return relative_folder(base, resolved);
    }
    if rest.is_empty() {
        return b"~".to_vec();
    }
    [b"~/".as_slice(), &rest].concat()
}

#[cfg(test)]
mod tests {
    use crate::folder_path::path_bytes;

    use super::*;

    fn resolved(raw: &str, base: &str, home: Option<&str>, wanted: Wanted) -> Option<String> {
        let (base, request) = to_resolve(
            raw.as_bytes(),
            base.as_bytes(),
            home.map(str::as_bytes),
            wanted,
        )?;
        let roots = [b"/".to_vec()];
        let path = resolve_folder(&roots, &base, &request)?;
        Some(String::from_utf8(path).unwrap())
    }

    #[test]
    fn a_home_form_resolves_from_the_home_folder_whatever_the_base() {
        let got = |raw| resolved(raw, "/home/x/Code", Some("/home/x"), Wanted::Existing);
        assert_eq!(got("~/Code/app").as_deref(), Some("/home/x/Code/app"));
        assert_eq!(got("~/../../srv/x").as_deref(), Some("/srv/x"));
        assert_eq!(got("~").as_deref(), Some("/home/x"));
    }

    #[test]
    fn a_home_form_needs_a_home_folder() {
        assert_eq!(resolved("~/a", "/home/x", None, Wanted::Existing), None);
    }

    #[test]
    fn a_folder_named_like_a_user_is_an_old_text() {
        let got = resolved("~x/a", "/home/x/Code", Some("/home/x"), Wanted::Existing);
        assert_eq!(got.as_deref(), Some("/home/x/Code/~x/a"));
    }

    /// The form of the resolver: `/C:/Users/x` on Windows.
    fn resolver_form(path: &Path) -> String {
        String::from_utf8(path_bytes(path)).unwrap()
    }

    #[test]
    fn an_old_text_resolves_from_the_base_when_its_folder_is_there() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Code/app")).unwrap();
        std::fs::create_dir_all(tmp.path().join("app")).unwrap();
        let home = &resolver_form(tmp.path());
        let base = format!("{home}/Code");

        let got = resolved("app", &base, Some(home), Wanted::Existing);

        assert_eq!(got, Some(format!("{base}/app")));
    }

    #[test]
    fn an_old_text_of_the_home_base_resolves_from_the_home_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Documents/Code/Personal/sandcastle")).unwrap();
        let home = &resolver_form(tmp.path());
        let base = format!("{home}/Documents/Code");

        let got = resolved(
            "Documents/Code/Personal/sandcastle",
            &base,
            Some(home),
            Wanted::Existing,
        );

        assert_eq!(got, Some(format!("{base}/Personal/sandcastle")));
    }

    #[test]
    fn an_old_text_with_a_step_up_never_resolves_from_the_home_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join("app")).unwrap();
        let home = &resolver_form(&home);
        let base = format!("{home}/Code");

        let got = resolved("../home/app", &base, Some(home), Wanted::Existing);

        assert_eq!(got, Some(format!("{home}/home/app")));
    }

    #[test]
    fn a_new_folder_always_resolves_from_the_base() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Documents")).unwrap();
        let home = &resolver_form(tmp.path());
        let base = format!("{home}/Code");

        let got = resolved("Documents", &base, Some(home), Wanted::New);

        assert_eq!(got, Some(format!("{base}/Documents")));
    }

    #[test]
    fn an_old_text_that_names_no_folder_resolves_from_the_base() {
        let got = resolved("gone", "/nowhere/Code", Some("/nowhere"), Wanted::Existing);
        assert_eq!(got.as_deref(), Some("/nowhere/Code/gone"));
    }

    #[test]
    fn the_game_text_is_the_home_form_with_a_step_up_outside_the_home_folder() {
        let text = |resolved: &str| {
            let home = Some(b"/home/x".as_slice());
            String::from_utf8(game_text(b"/home/x/Code", home, resolved.as_bytes())).unwrap()
        };
        assert_eq!(text("/home/x/Code/app"), "~/Code/app");
        assert_eq!(text("/home/x"), "~");
        assert_eq!(text("/srv/code"), "~/../../srv/code");
    }

    #[test]
    fn the_game_text_keeps_the_old_form_for_another_drive_or_no_home_folder() {
        let home = Some(b"/C:/Users/x".as_slice());
        assert_eq!(game_text(b"/D:/Code", home, b"/D:/Code/app"), b"app");
        assert_eq!(
            game_text(b"/C:/Users/x", home, b"/C:/Code"),
            b"~/../../Code"
        );
        assert_eq!(
            game_text(b"/home/x/Code", None, b"/home/x/Code/app"),
            b"app"
        );
    }
}
