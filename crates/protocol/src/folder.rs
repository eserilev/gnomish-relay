//! The folder policy: a chat can only work inside `allowed_roots`. See `SPEC.md` 6.2.
//!
//! This is path text only. The bridge also resolves symbolic links and checks again.
//! Paths use `/`. `~` is expanded by the bridge before it calls this.
//! Paths are compared by parts, never as text, so `/home/x/Code2` is not inside `/home/x/Code`.

use crate::ascii::{bytes_equal, copy_bytes, push_bytes};

const SLASH: u8 = b'/';

/// The parts of the folder so far are `stack[..depth]`. Aeneas has no `Vec::pop`,
/// so a `..` only lowers `depth`, and the next part overwrites the stale slot.
struct Walk {
    ok: bool,
    stack: Vec<Vec<u8>>,
    depth: usize,
}

fn end_part(mut parts: Vec<Vec<u8>>, cur: Vec<u8>) -> Vec<Vec<u8>> {
    if cur.len() > 0 {
        parts.push(cur);
    }
    parts
}

fn take_byte(parts: Vec<Vec<u8>>, mut cur: Vec<u8>, b: u8) -> (Vec<Vec<u8>>, Vec<u8>) {
    if b == SLASH {
        (end_part(parts, cur), Vec::new())
    } else {
        cur.push(b);
        (parts, cur)
    }
}

/// `/a//b/` has the parts `a` and `b`.
fn split_parts(path: &[u8]) -> Vec<Vec<u8>> {
    let mut parts = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < path.len() {
        let (p, c) = take_byte(parts, cur, path[i]);
        parts = p;
        cur = c;
        i += 1;
    }
    end_part(parts, cur)
}

fn is_dot(part: &[u8]) -> bool {
    part.len() == 1 && part[0] == b'.'
}

fn is_dot_dot(part: &[u8]) -> bool {
    part.len() == 2 && part[0] == b'.' && part[1] == b'.'
}

fn put(mut stack: Vec<Vec<u8>>, depth: usize, part: Vec<u8>) -> Vec<Vec<u8>> {
    if depth < stack.len() {
        stack[depth] = part;
    } else {
        stack.push(part);
    }
    stack
}

/// A `..` above `/` fails the walk.
fn apply_part(walk: Walk, part: &[u8]) -> Walk {
    if is_dot(part) {
        walk
    } else if is_dot_dot(part) {
        if walk.depth == 0 {
            Walk {
                ok: false,
                stack: walk.stack,
                depth: 0,
            }
        } else {
            Walk {
                ok: walk.ok,
                stack: walk.stack,
                depth: walk.depth - 1,
            }
        }
    } else {
        Walk {
            ok: walk.ok,
            stack: put(walk.stack, walk.depth, copy_bytes(part)),
            depth: walk.depth + 1,
        }
    }
}

fn apply_all(mut walk: Walk, parts: &[Vec<u8>]) -> Walk {
    let mut i = 0;
    while walk.ok && i < parts.len() {
        walk = apply_part(walk, &parts[i]);
        i += 1;
    }
    walk
}

fn start(base: &[u8], request: &[u8]) -> Walk {
    let walk = Walk {
        ok: true,
        stack: Vec::new(),
        depth: 0,
    };
    if request.len() > 0 && request[0] == SLASH {
        walk
    } else {
        apply_all(walk, &split_parts(base))
    }
}

fn is_prefix(root: &[Vec<u8>], stack: &[Vec<u8>], depth: usize) -> bool {
    let mut same = root.len() <= depth;
    let mut j = 0;
    while same && j < root.len() {
        same = bytes_equal(&root[j], &stack[j]);
        j += 1;
    }
    same
}

fn inside_any(roots: &[Vec<u8>], stack: &[Vec<u8>], depth: usize) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < roots.len() {
        found = is_prefix(&split_parts(&roots[i]), stack, depth);
        i += 1;
    }
    found
}

fn join(stack: &[Vec<u8>], depth: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut j = 0;
    while j < depth {
        out.push(SLASH);
        push_bytes(&mut out, &stack[j]);
        j += 1;
    }
    out
}

/// `request` is absolute or relative to `base`. Returns the normalized folder, or
/// `None` if it is outside every root.
#[must_use]
pub fn resolve_folder(roots: &[Vec<u8>], base: &[u8], request: &[u8]) -> Option<Vec<u8>> {
    let walk = apply_all(start(base, request), &split_parts(request));
    if !walk.ok || walk.depth == 0 || !inside_any(roots, &walk.stack, walk.depth) {
        return None;
    }
    Some(join(&walk.stack, walk.depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(request: &str) -> Option<String> {
        let roots = [b"/home/x/Code".to_vec()];
        resolve_folder(&roots, b"/home/x/Code/app", request.as_bytes())
            .map(|p| String::from_utf8(p).unwrap())
    }

    #[test]
    fn a_plain_subfolder_is_joined_to_the_base() {
        assert_eq!(
            resolve("src/ui").as_deref(),
            Some("/home/x/Code/app/src/ui")
        );
    }

    #[test]
    fn an_empty_request_is_the_base() {
        assert_eq!(resolve("").as_deref(), Some("/home/x/Code/app"));
    }

    #[test]
    fn dot_dot_can_move_inside_the_root() {
        assert_eq!(resolve("../lib").as_deref(), Some("/home/x/Code/lib"));
    }

    #[test]
    fn dot_dot_cannot_leave_the_root() {
        assert_eq!(resolve("../../.ssh"), None);
    }

    #[test]
    fn dot_dot_above_slash_is_refused() {
        assert_eq!(resolve("/../../../home/x/Code"), None);
    }

    #[test]
    fn an_absolute_request_ignores_the_base() {
        assert_eq!(
            resolve("/home/x/Code/other").as_deref(),
            Some("/home/x/Code/other")
        );
        assert_eq!(resolve("/etc"), None);
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_outside() {
        assert_eq!(resolve("/home/x/Code2"), None);
    }

    #[test]
    fn dots_and_repeated_slashes_are_cleaned() {
        assert_eq!(
            resolve("/home//x/./Code/app/").as_deref(),
            Some("/home/x/Code/app")
        );
    }

    #[test]
    fn the_root_itself_is_allowed_but_slash_is_not() {
        assert_eq!(resolve("/home/x/Code").as_deref(), Some("/home/x/Code"));
        assert_eq!(resolve("/"), None);
    }

    #[test]
    fn a_root_with_extra_slashes_still_matches() {
        let roots = [b"//home/x//Code/".to_vec()];
        let got = resolve_folder(&roots, b"/home/x/Code", b"a");
        assert_eq!(got, Some(b"/home/x/Code/a".to_vec()));
    }

    #[test]
    fn no_roots_means_nothing_is_allowed() {
        assert_eq!(resolve_folder(&[], b"/home/x", b"a"), None);
    }
}
