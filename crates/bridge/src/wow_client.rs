//! The WoW clients that Gnomish Relay supports (SPEC.md 7.9).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WowClient {
    Forever,
    Anniversary,
}

impl WowClient {
    /// In search order: on a tie, the first one wins.
    pub const ALL: [WowClient; 2] = [WowClient::Forever, WowClient::Anniversary];

    /// The folder of the client inside `World of Warcraft`.
    pub fn folder(self) -> &'static str {
        match self {
            WowClient::Forever => "_classic_beta_",
            WowClient::Anniversary => "_anniversary_",
        }
    }

    /// The `## Interface` number of the client: major * 10000 + minor * 100 + patch.
    pub fn interface(self) -> u32 {
        match self {
            WowClient::Forever => 16001,
            WowClient::Anniversary => 20506,
        }
    }

    /// The name of the client in fixture files, such as `forever-1.60.1.70124.json`.
    pub fn name(self) -> &'static str {
        match self {
            WowClient::Forever => "forever",
            WowClient::Anniversary => "anniversary",
        }
    }

    pub fn of_folder_name(name: &str) -> Option<WowClient> {
        WowClient::ALL.into_iter().find(|c| c.folder() == name)
    }

    /// The client of a build, from the major and minor version of its interface number.
    pub fn of_interface(interface: u32) -> Option<WowClient> {
        WowClient::ALL
            .into_iter()
            .find(|c| c.interface() / 100 == interface / 100)
    }
}

/// The `## Interface` of every addon that the desktop app writes. It lists every client,
/// so one file loads in each.
pub const TOC_INTERFACE: &str = "16001, 20506";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toc_interface_lists_every_client_in_order() {
        let numbers: Vec<String> = WowClient::ALL
            .iter()
            .map(|c| c.interface().to_string())
            .collect();
        assert_eq!(TOC_INTERFACE, numbers.join(", "));
    }

    #[test]
    fn a_folder_name_gives_its_client() {
        assert_eq!(
            WowClient::of_folder_name("_anniversary_"),
            Some(WowClient::Anniversary)
        );
        assert_eq!(
            WowClient::of_folder_name("_classic_beta_"),
            Some(WowClient::Forever)
        );
        assert_eq!(WowClient::of_folder_name("_retail_"), None);
    }

    #[test]
    fn a_patch_of_a_client_still_gives_that_client() {
        assert_eq!(WowClient::of_interface(20507), Some(WowClient::Anniversary));
        assert_eq!(WowClient::of_interface(16001), Some(WowClient::Forever));
        assert_eq!(WowClient::of_interface(120_001), None);
    }

    #[test]
    fn each_client_has_its_own_folder_and_name() {
        let [forever, anniversary] = WowClient::ALL;
        assert_ne!(forever.folder(), anniversary.folder());
        assert_ne!(forever.name(), anniversary.name());
    }
}
