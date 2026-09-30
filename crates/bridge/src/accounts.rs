//! The account folder of each token (SPEC.md 7.6). A new token in a folder that held
//! another one is a wipe. A token in another folder is another account. No I/O here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The token that the saved variables of each account folder held last, by folder name.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Accounts(BTreeMap<String, String>);

impl Accounts {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the token that the folder held before, when it now holds another one.
    pub fn replaced_token(&mut self, account: &str, token: &str) -> Option<String> {
        let old = self.0.insert(account.to_owned(), token.to_owned())?;
        (old != token).then_some(old)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_token_of_a_folder_replaces_nothing() {
        let mut accounts = Accounts::default();

        assert_eq!(accounts.replaced_token("ACCOUNT1", "one"), None);
    }

    #[test]
    fn the_same_token_again_replaces_nothing() {
        let mut accounts = Accounts::default();
        accounts.replaced_token("ACCOUNT1", "one");

        assert_eq!(accounts.replaced_token("ACCOUNT1", "one"), None);
    }

    #[test]
    fn a_new_token_in_the_same_folder_replaces_the_old_one() {
        let mut accounts = Accounts::default();
        accounts.replaced_token("ACCOUNT1", "one");

        assert_eq!(
            accounts.replaced_token("ACCOUNT1", "two"),
            Some("one".to_owned())
        );
        assert_eq!(accounts.replaced_token("ACCOUNT1", "two"), None);
    }

    #[test]
    fn a_token_in_another_folder_replaces_nothing() {
        let mut accounts = Accounts::default();
        accounts.replaced_token("ACCOUNT1", "one");

        assert_eq!(accounts.replaced_token("ACCOUNT2", "two"), None);
        assert_eq!(accounts.replaced_token("ACCOUNT1", "one"), None);
    }
}
