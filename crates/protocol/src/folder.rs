//! The folder policy: a chat can only work inside `allowed_roots`. See `SPEC.md` 6.2.
//!
//! This is path text only. The bridge also resolves symbolic links and checks again.
//! Paths use `/`. `~` is expanded by the bridge before it calls this.

/// `roots` and `base` are absolute and normalized. `request` is absolute or relative
/// to `base`. Returns the normalized folder, or `None` if it is outside every root.
#[must_use]
pub fn resolve_folder(roots: &[Vec<u8>], base: &[u8], request: &[u8]) -> Option<Vec<u8>> {
    todo!()
}
