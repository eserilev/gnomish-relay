//! `allowed_roots` of the running bridge, resolved. A click on the desktop adds a root
//! while the bridge runs (SPEC.md 9.12), so the gate of every agent, the folder walk,
//! and the relay lane share one list.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use crate::folder_path::{is_inside_folder, path_bytes};

#[derive(Clone, Debug, Default)]
pub struct Roots(Arc<RwLock<Vec<PathBuf>>>);

impl Roots {
    pub fn new(roots: Vec<PathBuf>) -> Roots {
        Roots(Arc::new(RwLock::new(roots)))
    }

    pub fn list(&self) -> Vec<PathBuf> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A root that is already in the list stays once.
    pub fn add(&self, root: PathBuf) {
        let mut roots = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }

    /// `folder` is resolved. Paths compare by parts, so `/a/b2` is not inside `/a/b`.
    pub fn hold(&self, folder: &Path) -> bool {
        let folder = path_bytes(folder);
        self.list()
            .iter()
            .any(|root| is_inside_folder(&folder, &path_bytes(root)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_added_through_one_holder_shows_in_every_copy() {
        let roots = Roots::new(vec![PathBuf::from("/home/x/Code")]);
        let copy = roots.clone();

        roots.add(PathBuf::from("/home/x/lighthouse"));
        roots.add(PathBuf::from("/home/x/lighthouse"));

        assert_eq!(
            copy.list(),
            [
                PathBuf::from("/home/x/Code"),
                PathBuf::from("/home/x/lighthouse")
            ]
        );
    }

    #[test]
    fn a_folder_is_held_only_inside_a_root_by_its_parts() {
        let roots = Roots::new(vec![PathBuf::from("/home/x/Code")]);

        assert!(roots.hold(Path::new("/home/x/Code")));
        assert!(roots.hold(Path::new("/home/x/Code/app")));
        assert!(!roots.hold(Path::new("/home/x/Code2")));
        assert!(!roots.hold(Path::new("/home/x")));
    }
}
