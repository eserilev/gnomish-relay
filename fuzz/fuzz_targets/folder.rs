//! S5 on the compiled code: an accepted folder is clean and inside a root.
//! The input is `request NUL base NUL root NUL root ...`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::folder::resolve_folder;

fn parts(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .collect()
}

fn is_clean(path: &[u8]) -> bool {
    let parts = parts(path);
    let joined: Vec<u8> = parts.iter().flat_map(|p| [&b"/"[..], p].concat()).collect();
    !parts.is_empty() && joined == path && parts.iter().all(|&p| p != b"." && p != b"..")
}

fuzz_target!(|data: &[u8]| {
    let mut fields = data.split(|&b| b == 0);
    let request = fields.next().unwrap_or_default();
    let base = fields.next().unwrap_or_default();
    let roots: Vec<Vec<u8>> = fields.map(<[u8]>::to_vec).collect();
    let Some(folder) = resolve_folder(&roots, base, request) else {
        return;
    };
    assert!(is_clean(&folder));
    assert!(
        roots
            .iter()
            .any(|root| parts(&folder).starts_with(&parts(root)))
    );
});
