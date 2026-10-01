//! The reply to a folder list: the folder tree for the browser of the game (SPEC.md 9.9).
//!
//! The first line is the default folder as the player reads it. Each other line is a
//! folder, breadth first: `parent \t name \t mark`. A root has parent 0 and its whole
//! path as its name. A line `?` lists the folders whose subfolders are not all in the
//! reply. A last line `+` says that the tree is cut.

use protocol::lua::lua_string;
use protocol::slot::MAX_TEXT;

use crate::folder_path::{
    folder_request, is_inside_folder, native_folder, path_bytes, path_parts, relative_folder,
};
use crate::folder_walk::{Folder, Snapshot};
use crate::new_folder::is_folder_name;
use crate::relay::{Folders, in_roots};

/// The game sends a listed folder back in each message of its chat.
pub const MAX_FOLDER: usize = 255;
pub const CUT: &str = "+";
pub const NOT_WALKED: &str = "?";
/// The two quotes of the Lua literal, and the cut line after a newline.
const RESERVED: usize = 2 + 4 + CUT.len();
/// The room for the `?` line. A longer one becomes one range.
const NOT_WALKED_ROOM: usize = 1024;

/// How the reply writes paths. `~/` only when the default folder and every root are in
/// the home folder, so the addon can compare the parts of any two paths.
struct Names {
    home: Option<Vec<u8>>,
}

impl Names {
    fn new(folders: &Folders, snapshot: &Snapshot) -> Names {
        let home = snapshot.home.as_deref().map(path_bytes);
        let all_inside = |home: &Vec<u8>| {
            let mut paths = folders.roots.iter().chain([&folders.base]);
            paths.all(|p| is_inside_folder(&native(p), home))
        };
        Names {
            home: home.filter(all_inside),
        }
    }

    fn show(&self, resolved: &[u8]) -> Option<String> {
        let native = native(resolved);
        let Some(home) = &self.home else {
            return String::from_utf8(native).ok();
        };
        let rest = &path_parts(&native)[path_parts(home).len()..];
        let mut shown = vec![b"~".as_slice()];
        shown.extend(rest);
        String::from_utf8(shown.join(&b'/')).ok()
    }
}

fn native(resolved: &[u8]) -> Vec<u8> {
    native_folder(resolved.to_vec(), cfg!(windows))
}

/// A folder that the game can send back as it came, and that keeps a line whole.
fn fits_the_game(base: &[u8], resolved: &[u8]) -> bool {
    let Ok(folder) = String::from_utf8(relative_folder(base, resolved)) else {
        return false;
    };
    let whole = folder.len() <= MAX_FOLDER && !folder.chars().any(char::is_control);
    whole && folder_request(folder.as_bytes(), cfg!(windows)).is_some()
}

/// The line of one folder, or `None` for a folder that the game cannot use.
fn line(
    folders: &Folders,
    names: &Names,
    folder: &Folder,
    parent: Option<usize>,
) -> Option<String> {
    let resolved = in_roots(folders, &folder.path)?;
    if !fits_the_game(&folders.base, &resolved) {
        return None;
    }
    let mark = if folder.repo { "g" } else { "" };
    let (parent, name) = match (folder.parent, parent) {
        (None, _) => (0, names.show(&resolved)?),
        (Some(_), Some(parent)) => (parent, last_name(&resolved)?),
        (Some(_), None) => return None,
    };
    if name.chars().any(char::is_control) {
        return None;
    }
    Some(format!("{parent}\t{name}\t{mark}"))
}

fn last_name(resolved: &[u8]) -> Option<String> {
    let name = String::from_utf8(path_parts(resolved).last()?.to_vec()).ok()?;
    let sendable = !cfg!(windows) || !name.contains(':');
    (is_folder_name(&name) && sendable).then_some(name)
}

/// The size of a line in the slot file, with the newline before it.
fn cost(line: &str) -> usize {
    lua_string(format!("\n{line}").as_bytes()).len() - 2
}

fn range(first: usize, last: usize) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}

/// `numbers` is sorted, with no number twice: `1,4-6,9`.
fn ranges(numbers: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut first = numbers[0];
    let mut last = first;
    for &n in &numbers[1..] {
        if n != last + 1 {
            parts.push(range(first, last));
            first = n;
        }
        last = n;
    }
    parts.push(range(first, last));
    parts.join(",")
}

/// The `?` line, or `None` when the reply holds every folder in full. `last` is the
/// number of the last folder line.
fn not_walked_line(numbers: &[usize], last: usize) -> Option<String> {
    let first = *numbers.first()?;
    let line = format!("{NOT_WALKED}{}", ranges(numbers));
    if cost(&line) <= NOT_WALKED_ROOM {
        return Some(line);
    }
    Some(format!("{NOT_WALKED}{}", range(first, last)))
}

/// The line number of the parent of `folder`, when the parent has a line.
fn parent_line(numbers: &[Option<usize>], folder: &Folder) -> Option<usize> {
    folder
        .parent
        .and_then(|p| numbers.get(p).copied().flatten())
}

/// Keeps a breadth-first start of the tree that fits in one reply record (S12), so
/// shallow folders always come. A folder that is left out leaves out its subfolders.
pub fn folder_reply(folders: &Folders, snapshot: &Snapshot) -> String {
    let names = Names::new(folders, snapshot);
    let first = names.show(&folders.base).unwrap_or_default();
    let mut lines = vec![first.clone()];
    let mut size = RESERVED + NOT_WALKED_ROOM + cost(&first) - 4;
    let mut numbers: Vec<Option<usize>> = Vec::new();
    let mut not_walked = Vec::new();
    let mut cut_at = None;
    for (at, folder) in snapshot.folders.iter().enumerate() {
        let parent = parent_line(&numbers, folder);
        let Some(line) = line(folders, &names, folder, parent) else {
            numbers.push(None);
            continue;
        };
        if size + cost(&line) > MAX_TEXT {
            cut_at = Some(at);
            break;
        }
        size += cost(&line);
        lines.push(line);
        numbers.push(Some(lines.len() - 1));
        if !folder.walked {
            not_walked.push(lines.len() - 1);
        }
    }
    // The size cut leaves out subfolders, so their parents are not walked in full.
    let left_out = cut_at.map_or(&[][..], |at| &snapshot.folders[at..]);
    not_walked.extend(left_out.iter().filter_map(|f| parent_line(&numbers, f)));
    not_walked.sort_unstable();
    not_walked.dedup();
    lines.extend(not_walked_line(&not_walked, lines.len() - 1));
    if cut_at.is_some() || !snapshot.complete {
        lines.push(CUT.into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folders(roots: &[&str], base: &str) -> Folders {
        Folders {
            roots: roots.iter().map(|r| r.as_bytes().to_vec()).collect(),
            base: base.as_bytes().to_vec(),
        }
    }

    fn code() -> Folders {
        folders(&["/home/x/Code"], "/home/x/Code")
    }

    fn folder(path: &str, parent: Option<usize>) -> Folder {
        Folder {
            path: path.into(),
            parent,
            repo: false,
            walked: true,
        }
    }

    fn snapshot(found: Vec<Folder>, home: Option<&str>) -> Snapshot {
        Snapshot {
            folders: found,
            complete: true,
            home: home.map(PathBuf::from),
        }
    }

    #[test]
    fn the_first_line_is_the_default_folder_and_a_root_carries_its_whole_path() {
        let found = snapshot(
            vec![
                folder("/home/x/Code", None),
                folder("/home/x/Code/app", Some(0)),
                folder("/home/x/Code/app/src", Some(1)),
            ],
            None,
        );
        assert_eq!(
            folder_reply(&code(), &found),
            "/home/x/Code\n0\t/home/x/Code\t\n1\tapp\t\n2\tsrc\t"
        );
    }

    #[test]
    fn a_repository_carries_the_g_mark() {
        let mut app = folder("/home/x/Code/app", Some(0));
        app.repo = true;
        let found = snapshot(vec![folder("/home/x/Code", None), app], None);
        assert!(folder_reply(&code(), &found).ends_with("\n1\tapp\tg"));
    }

    #[test]
    fn paths_in_the_home_folder_start_with_a_tilde() {
        let found = snapshot(vec![folder("/home/x/Code", None)], Some("/home/x"));
        assert_eq!(folder_reply(&code(), &found), "~/Code\n0\t~/Code\t");
    }

    #[test]
    fn a_root_outside_the_home_folder_keeps_every_path_whole() {
        let policy = folders(&["/home/x/Code", "/srv/work"], "/home/x/Code");
        let found = snapshot(
            vec![folder("/home/x/Code", None), folder("/srv/work", None)],
            Some("/home/x"),
        );
        assert_eq!(
            folder_reply(&policy, &found),
            "/home/x/Code\n0\t/home/x/Code\t\n0\t/srv/work\t"
        );
    }

    #[test]
    fn a_folder_outside_the_roots_is_left_out_with_its_subfolders() {
        let found = snapshot(
            vec![
                folder("/home/x/Other", None),
                folder("/home/x/Other/app", Some(0)),
                folder("/home/x/Code", None),
            ],
            None,
        );
        assert_eq!(
            folder_reply(&code(), &found),
            "/home/x/Code\n0\t/home/x/Code\t"
        );
    }

    #[test]
    fn a_folder_that_would_break_a_line_or_that_the_game_cannot_send_back_is_left_out() {
        let long = format!("/home/x/Code/{}", "a".repeat(MAX_FOLDER + 1));
        let found = snapshot(
            vec![
                folder("/home/x/Code", None),
                folder("/home/x/Code/tab\there", Some(0)),
                folder("/home/x/Code/tab\there/below", Some(1)),
                folder(&long, Some(0)),
                folder("/home/x/Code/fine", Some(0)),
            ],
            None,
        );
        assert_eq!(
            folder_reply(&code(), &found),
            "/home/x/Code\n0\t/home/x/Code\t\n1\tfine\t"
        );
    }

    #[test]
    fn an_incomplete_walk_is_marked_as_cut() {
        let mut found = snapshot(vec![folder("/home/x/Code", None)], None);
        found.complete = false;
        assert!(folder_reply(&code(), &found).ends_with("\n+"));
    }

    fn big_tree(name: &str, count: usize) -> Snapshot {
        let mut found = vec![folder("/home/x/Code", None)];
        for i in 0..count {
            found.push(folder(&format!("/home/x/Code/{name}{i}"), Some(0)));
        }
        snapshot(found, None)
    }

    #[test]
    fn a_tree_too_big_for_one_record_is_cut_breadth_first_and_marked() {
        let found = big_tree("folder-with-a-long-name-", 3000);

        let text = folder_reply(&code(), &found);

        assert!(lua_string(text.as_bytes()).len() <= MAX_TEXT);
        assert!(text.ends_with("\n+"));
        let kept: Vec<&str> = text.lines().skip(2).collect();
        assert!(kept.len() > 100);
        assert_eq!(kept[kept.len() - 2], "?1", "the root misses subfolders");
        for (i, line) in kept.iter().take(kept.len() - 2).enumerate() {
            assert_eq!(*line, format!("1\tfolder-with-a-long-name-{i}\t"));
        }
    }

    #[test]
    fn a_name_whose_bytes_escape_counts_four_times_toward_the_limit() {
        let found = big_tree("\u{e9}\u{e9}\u{e9}\u{e9}", 3000);
        let text = folder_reply(&code(), &found);
        assert!(lua_string(text.as_bytes()).len() <= MAX_TEXT);
        assert!(text.ends_with("\n+"));
    }

    #[test]
    fn a_tree_that_fits_has_no_cut_mark() {
        let text = folder_reply(&code(), &big_tree("f", 10));
        assert!(!text.ends_with('+'));
        assert_eq!(text.lines().count(), 12);
    }

    fn deep(path: &str, parent: usize) -> Folder {
        let mut f = folder(path, Some(parent));
        f.walked = false;
        f
    }

    #[test]
    fn the_folders_that_are_not_walked_come_as_ranges_of_line_numbers() {
        let found = snapshot(
            vec![
                folder("/home/x/Code", None),
                deep("/home/x/Code/a", 0),
                deep("/home/x/Code/b", 0),
                deep("/home/x/Code/c", 0),
                folder("/home/x/Code/d", Some(0)),
                deep("/home/x/Code/e", 0),
            ],
            None,
        );

        let text = folder_reply(&code(), &found);

        assert!(text.ends_with("\n1\te\t\n?2-4,6"), "{text}");
    }

    #[test]
    fn a_folder_that_is_left_out_does_not_count_as_not_walked() {
        let found = snapshot(
            vec![
                folder("/home/x/Code", None),
                deep("/home/x/Code/tab\there", 0),
            ],
            None,
        );
        assert_eq!(
            folder_reply(&code(), &found),
            "/home/x/Code\n0\t/home/x/Code\t"
        );
    }

    #[test]
    fn a_long_not_walked_line_becomes_one_range_to_the_last_folder() {
        let mut found = vec![folder("/home/x/Code", None)];
        for i in 0..600 {
            let f = format!("/home/x/Code/f{i}");
            found.push(if i % 2 == 0 {
                deep(&f, 0)
            } else {
                folder(&f, Some(0))
            });
        }

        let text = folder_reply(&code(), &snapshot(found, None));

        assert!(text.ends_with("\n?2-601"), "{}", &text[text.len() - 40..]);
        assert!(lua_string(text.as_bytes()).len() <= MAX_TEXT);
    }
}
