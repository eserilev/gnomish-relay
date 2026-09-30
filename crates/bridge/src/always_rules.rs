//! The "Always allow" rules from the game (SPEC.md 6.6.5): `rules.json` in the data
//! folder. The data folder is a `deny` path, so no agent reads or writes this file.
//!
//! Each call reads the file again, so `gnomish-relay rules remove` works while the
//! bridge runs.

use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use protocol::always::is_plain_word;
use serde::{Deserialize, Serialize};

use crate::fs_safe::{make_private_dir, write_private};
use crate::run::log;

pub const FILE: &str = "rules.json";
/// The bridge never drops a rule by itself, so a full folder gets no more rules.
pub const MAX_PER_FOLDER: usize = 64;
/// A rule ends this many days after its last use.
pub const DAYS_KEPT: u32 = 30;
/// The popup never cuts the rule line (SPEC.md 6.4).
pub const MAX_LINE: usize = 48;
const DAY: u32 = 86_400;
/// The mark of a folder that the rule line cut from the left.
const LINE_CUT: &str = "...";
/// 64 rules in each of many folders fit in far less.
const MAX_FILE: u64 = 1024 * 1024;

/// A rule of a root of `allowed_roots` or of the home folder covers only that folder.
/// Else one click there gives a global rule from the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The folder and every folder inside it.
    Tree,
    Exact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// 4 hex digits, for `gnomish-relay rules remove` and the Settings tab.
    pub id: String,
    /// Resolved, as the chat folder is.
    pub folder: PathBuf,
    pub scope: Scope,
    /// The first words of a command: `cargo test` covers `cargo test -q`.
    pub words: Vec<String>,
    /// Unix seconds.
    pub added: u32,
    /// Days since 1970. The bridge writes it at most once a day.
    pub used_day: u32,
}

impl Rule {
    pub fn applies_to(&self, chat: &Path) -> bool {
        match self.scope {
            Scope::Exact => chat == self.folder,
            Scope::Tree => chat.starts_with(&self.folder),
        }
    }

    /// `cargo test *`, as the popup and the Settings tab show it.
    pub fn pattern(&self) -> String {
        pattern_of(&self.words)
    }

    pub fn days_unused(&self, now: u32) -> u32 {
        day_of(now).saturating_sub(self.used_day)
    }

    fn is_expired(&self, now: u32) -> bool {
        self.days_unused(now) > DAYS_KEPT
    }
}

pub fn pattern_of(words: &[String]) -> String {
    format!("{} *", words.join(" "))
}

pub fn day_of(now: u32) -> u32 {
    now / DAY
}

fn is_id(id: &str) -> bool {
    id.len() == 4
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// An absolute path with no `.` or `..`, as `canonicalize` gives it.
fn is_clean_folder(folder: &Path) -> bool {
    folder.is_absolute()
        && folder
            .components()
            .all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
}

/// The words that `propose` of `protocol` makes: 1 or 2 plain words, and no `/` in the
/// first. A row that the file changed into anything else never loads.
pub fn is_rule(words: &[String]) -> bool {
    let plain = words.iter().all(|w| is_plain_word(w.as_bytes()));
    (1..=2).contains(&words.len()) && plain && !words[0].contains('/')
}

fn is_good(rule: &Rule) -> bool {
    is_id(&rule.id) && is_clean_folder(&rule.folder) && is_rule(&rule.words)
}

#[derive(Serialize, Deserialize, Default)]
struct FileRows {
    rules: Vec<serde_json::Value>,
}

fn row_of(value: serde_json::Value) -> Option<Rule> {
    let rule: Rule = serde_json::from_value(value).ok()?;
    is_good(&rule).then_some(rule)
}

fn read_rows(path: &Path) -> Result<Option<FileRows>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    if !meta.is_file() || meta.len() > MAX_FILE {
        anyhow::bail!("{} is not a rules file", path.display());
    }
    let text = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&text)?))
}

/// The rules that still count. A bad row is dropped with a log line, so a broken file
/// never widens a rule. A missing or broken file is no rules.
pub fn parse_rules(text: &str, now: u32) -> Vec<Rule> {
    let Ok(file) = serde_json::from_str::<FileRows>(text) else {
        log("rules.json is damaged: no Always rules apply");
        return Vec::new();
    };
    good_rows(file, now)
}

fn good_rows(file: FileRows, now: u32) -> Vec<Rule> {
    let mut rules = Vec::new();
    for value in file.rules {
        match row_of(value) {
            Some(rule) if !rule.is_expired(now) => rules.push(rule),
            Some(_) => {}
            None => log("rules.json: dropped a bad row"),
        }
    }
    rules
}

pub fn load(dir: &Path, now: u32) -> Vec<Rule> {
    match read_rows(&dir.join(FILE)) {
        Ok(Some(file)) => good_rows(file, now),
        Ok(None) => Vec::new(),
        Err(e) => {
            log(&format!("rules.json: {e:#}. No Always rules apply."));
            Vec::new()
        }
    }
}

pub fn save(dir: &Path, rules: &[Rule]) -> Result<()> {
    let file = FileRows {
        rules: rules
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()?,
    };
    make_private_dir(dir)?;
    write_private(dir, FILE, &serde_json::to_string_pretty(&file)?)
}

/// The words of each rule for the chat, as the classifier takes them.
pub fn words_for(rules: &[Rule], chat: &Path) -> Vec<Vec<Vec<u8>>> {
    rules
        .iter()
        .filter(|r| r.applies_to(chat))
        .map(|r| r.words.iter().map(|w| w.as_bytes().to_vec()).collect())
        .collect()
}

pub fn count_in(rules: &[Rule], folder: &Path) -> usize {
    rules.iter().filter(|r| r.folder == folder).count()
}

fn new_id(rules: &[Rule]) -> Result<String> {
    loop {
        let mut bytes = [0u8; 2];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no random bytes: {e}"))?;
        let id = bytes.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
        if !rules.iter().any(|r| r.id == id) {
            return Ok(id);
        }
    }
}

/// Adds each rule that the folder does not have yet. A rule that exists gives no second row.
pub fn add(
    rules: &mut Vec<Rule>,
    folder: &Path,
    scope: Scope,
    new: &[Vec<String>],
    now: u32,
) -> Result<()> {
    for words in new {
        let same = |r: &Rule| r.folder == folder && r.scope == scope && &r.words == words;
        if rules.iter().any(same) {
            continue;
        }
        let rule = Rule {
            id: new_id(rules)?,
            folder: folder.to_owned(),
            scope,
            words: words.clone(),
            added: now,
            used_day: day_of(now),
        };
        rules.push(rule);
    }
    Ok(())
}

/// Marks each rule of the chat that covers one of `commands` as used today. Returns
/// whether a day changed, so a command writes the file at most once a day.
pub fn mark_used(rules: &mut [Rule], chat: &Path, commands: &[Vec<Vec<u8>>], now: u32) -> bool {
    let today = day_of(now);
    let mut changed = false;
    for rule in rules.iter_mut().filter(|r| r.applies_to(chat)) {
        let covers = commands
            .iter()
            .any(|words| starts_with_rule(words, &rule.words));
        if covers && rule.used_day < today {
            rule.used_day = today;
            changed = true;
        }
    }
    changed
}

fn starts_with_rule(words: &[Vec<u8>], rule: &[String]) -> bool {
    rule.len() <= words.len() && rule.iter().zip(words).all(|(r, w)| r.as_bytes() == w)
}

/// The folder as the popup shows it: its path from its allowed root, `~/` for the home
/// folder, and only printable ASCII.
pub fn shown_folder(folder: &Path, roots: &[PathBuf], home: &Path) -> String {
    let base = roots
        .iter()
        .filter_map(|root| root.parent().filter(|_| folder.starts_with(root)))
        .max_by_key(|parent| parent.components().count());
    let text = match (base, folder.strip_prefix(home)) {
        (Some(parent), _) => rel_text(folder.strip_prefix(parent).unwrap_or(folder)),
        (None, Ok(rest)) => format!("~/{}", rel_text(rest)),
        (None, Err(_)) => folder.to_string_lossy().into_owned(),
    };
    printable(&text)
}

fn rel_text(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// A folder name can hold any byte. The line stays printable ASCII.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// `cargo test *, tail * in Personal/app`, at most `MAX_LINE` bytes. The folder is cut
/// from the left. `None` when the patterns leave too little room for a folder.
pub fn rule_line(new: &[Vec<String>], folder: &str) -> Option<String> {
    let patterns: Vec<String> = new.iter().map(|w| pattern_of(w)).collect();
    let head = format!("{} in ", patterns.join(", "));
    let room = MAX_LINE.checked_sub(head.len())?;
    if folder.len() <= room {
        return Some(format!("{head}{folder}"));
    }
    if room < LINE_CUT.len() + 4 {
        return None;
    }
    let tail = &folder[folder.len() - (room - LINE_CUT.len())..];
    Some(format!("{head}{LINE_CUT}{tail}"))
}

/// A rule as the settings list shows it (SPEC.md 13.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleLine {
    pub id: String,
    pub folder: String,
    pub pattern: String,
    /// Days since its last use.
    pub days: u32,
}

/// The rules for the Settings tab, with what the folders show.
#[derive(Clone, Debug, Default)]
pub struct RuleList {
    pub store: AlwaysRules,
    pub roots: Vec<PathBuf>,
    pub home: PathBuf,
}

impl RuleList {
    pub fn lines(&self, now: u32) -> Vec<RuleLine> {
        self.store
            .list(now)
            .iter()
            .map(|r| RuleLine {
                id: r.id.clone(),
                folder: shown_folder(&r.folder, &self.roots, &self.home),
                pattern: r.pattern(),
                days: r.days_unused(now),
            })
            .collect()
    }
}

/// The rules file with a lock, so two runs never lose each other's rule.
#[derive(Clone, Debug)]
pub struct AlwaysRules {
    dir: Option<PathBuf>,
    lock: Arc<Mutex<()>>,
}

impl Default for AlwaysRules {
    fn default() -> AlwaysRules {
        AlwaysRules::none()
    }
}

impl AlwaysRules {
    pub fn new(data_dir: &Path) -> AlwaysRules {
        AlwaysRules {
            dir: Some(data_dir.to_owned()),
            lock: Arc::default(),
        }
    }

    /// No rules and no file, for a gate that never offers "Always allow".
    pub fn none() -> AlwaysRules {
        AlwaysRules {
            dir: None,
            lock: Arc::default(),
        }
    }

    pub fn is_on(&self) -> bool {
        self.dir.is_some()
    }

    pub fn list(&self, now: u32) -> Vec<Rule> {
        self.dir.as_ref().map_or_else(Vec::new, |d| load(d, now))
    }

    /// Reads, changes, and writes the file under the lock. `change` returns whether
    /// to write.
    fn update<T>(&self, now: u32, change: impl FnOnce(&mut Vec<Rule>) -> (bool, T)) -> Result<T> {
        let Some(dir) = &self.dir else {
            anyhow::bail!("no rules file");
        };
        let _held = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rules = load(dir, now);
        let (write, out) = change(&mut rules);
        if write {
            save(dir, &rules)?;
        }
        Ok(out)
    }

    pub fn grant(&self, folder: &Path, scope: Scope, new: &[Vec<String>], now: u32) -> Result<()> {
        self.update(now, |rules| (true, add(rules, folder, scope, new, now)))?
    }

    /// Returns whether a rule had this id.
    pub fn remove(&self, id: &str, now: u32) -> Result<bool> {
        self.update(now, |rules| {
            let before = rules.len();
            rules.retain(|r| r.id != id);
            let removed = rules.len() < before;
            (removed, removed)
        })
    }

    pub fn mark_used(&self, chat: &Path, commands: &[Vec<Vec<u8>>], now: u32) {
        if !self.is_on() {
            return;
        }
        let marked = self.update(now, |rules| (mark_used(rules, chat, commands, now), ()));
        if let Err(e) = marked {
            log(&format!("rules.json: {e:#}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u32 = 1_790_000_000;

    fn words(w: &[&str]) -> Vec<String> {
        w.iter().map(|s| (*s).to_owned()).collect()
    }

    /// `/h/app` as an absolute path of this OS: `C:\h\app` on Windows.
    fn h(path: &str) -> PathBuf {
        let base = if cfg!(windows) { r"C:\h" } else { "/h" };
        let rest = path.strip_prefix("/h").unwrap_or(path);
        PathBuf::from(base).join(rest.trim_start_matches('/'))
    }

    fn json(path: &Path) -> String {
        serde_json::to_string(path).unwrap()
    }

    fn rule(id: &str, folder: &str, scope: Scope, w: &[&str]) -> Rule {
        Rule {
            id: id.into(),
            folder: h(folder),
            scope,
            words: words(w),
            added: NOW,
            used_day: day_of(NOW),
        }
    }

    #[test]
    fn a_tree_rule_covers_the_folders_inside_and_an_exact_rule_only_its_own() {
        let tree = rule("a1b2", "/h/Code/app", Scope::Tree, &["make"]);
        let exact = rule("c3d4", "/h/Code", Scope::Exact, &["make"]);
        assert!(tree.applies_to(&h("/h/Code/app/src")));
        assert!(!tree.applies_to(&h("/h/Code/lib")));
        assert!(exact.applies_to(&h("/h/Code")));
        assert!(!exact.applies_to(&h("/h/Code/app")));
    }

    #[test]
    fn a_rule_ends_thirty_days_after_its_last_use() {
        let mut r = rule("a1b2", "/h/app", Scope::Tree, &["make"]);
        r.used_day = day_of(NOW) - DAYS_KEPT;
        assert!(!r.is_expired(NOW));
        r.used_day -= 1;
        assert!(r.is_expired(NOW));
    }

    #[test]
    fn a_bad_row_is_dropped_and_the_good_rows_stay() {
        let app = json(&h("/h/app"));
        let up = json(&h("/h").join("..").join("app"));
        let text = format!(
            r#"{{"rules": [
                {{"id": "a1b2", "folder": {app}, "scope": "tree", "words": ["cargo", "test"], "added": {NOW}, "used_day": {day}}},
                {{"id": "zzzz", "folder": {app}, "scope": "tree", "words": ["make"], "added": 0, "used_day": {day}}},
                {{"id": "c3d4", "folder": "rel/app", "scope": "tree", "words": ["make"], "added": 0, "used_day": {day}}},
                {{"id": "c3d5", "folder": {up}, "scope": "tree", "words": ["make"], "added": 0, "used_day": {day}}},
                {{"id": "e5f6", "folder": {app}, "scope": "tree", "words": ["rm -rf"], "added": 0, "used_day": {day}}},
                {{"id": "e5f7", "folder": {app}, "scope": "tree", "words": ["a", "b", "c"], "added": 0, "used_day": {day}}},
                {{"id": "e5f8", "folder": {app}, "scope": "tree", "words": ["./x.sh"], "added": 0, "used_day": {day}}},
                {{"id": "e5f9", "folder": {app}, "scope": "tree", "words": ["make"], "added": 0, "used_day": {day}, "extra": 1}},
                {{"id": "e5fa", "folder": {app}, "scope": "all", "words": ["make"], "added": 0, "used_day": {day}}}
            ]}}"#,
            day = day_of(NOW)
        );
        let rules = parse_rules(&text, NOW);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "a1b2");
    }

    #[test]
    fn a_damaged_file_is_no_rules() {
        assert!(parse_rules("{", NOW).is_empty());
        assert!(parse_rules("[]", NOW).is_empty());
    }

    #[test]
    fn a_missing_file_is_no_rules_and_a_save_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path(), NOW).is_empty());
        let rules = vec![rule("a1b2", "/h/app", Scope::Tree, &["cargo", "test"])];
        save(dir.path(), &rules).unwrap();
        assert_eq!(load(dir.path(), NOW), rules);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn an_expired_rule_does_not_load() {
        let dir = tempfile::tempdir().unwrap();
        let mut old = rule("a1b2", "/h/app", Scope::Tree, &["make"]);
        old.used_day = day_of(NOW) - DAYS_KEPT - 1;
        save(dir.path(), &[old]).unwrap();
        assert!(load(dir.path(), NOW).is_empty());
    }

    #[test]
    fn a_link_in_place_of_the_file_is_no_rules() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("other.json");
        save(
            dir.path(),
            &[rule("a1b2", "/h/app", Scope::Tree, &["make"])],
        )
        .unwrap();
        fs::rename(dir.path().join(FILE), &other).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&other, dir.path().join(FILE)).unwrap();
            assert!(load(dir.path(), NOW).is_empty());
        }
    }

    #[test]
    fn a_rule_that_exists_gives_no_second_row() {
        let mut rules = Vec::new();
        let folder = &h("/h/app");
        add(&mut rules, folder, Scope::Tree, &[words(&["make"])], NOW).unwrap();
        add(
            &mut rules,
            folder,
            Scope::Tree,
            &[words(&["make"]), words(&["tail"])],
            NOW,
        )
        .unwrap();
        assert_eq!(rules.len(), 2);
        assert_ne!(rules[0].id, rules[1].id);
        assert!(rules.iter().all(|r| is_id(&r.id)));
        assert_eq!(count_in(&rules, folder), 2);
    }

    #[test]
    fn the_words_of_a_chat_come_from_the_rules_that_apply() {
        let rules = vec![
            rule("a1b2", "/h/app", Scope::Tree, &["cargo", "test"]),
            rule("c3d4", "/h/lib", Scope::Tree, &["make"]),
        ];
        let got = words_for(&rules, &h("/h/app/src"));
        assert_eq!(got, vec![vec![b"cargo".to_vec(), b"test".to_vec()]]);
    }

    #[test]
    fn a_use_marks_the_day_once() {
        let mut r = rule("a1b2", "/h/app", Scope::Tree, &["cargo", "test"]);
        r.used_day = day_of(NOW) - 3;
        let mut rules = vec![r];
        let used = vec![vec![b"cargo".to_vec(), b"test".to_vec(), b"-q".to_vec()]];
        let other = vec![vec![b"make".to_vec()]];
        assert!(!mark_used(&mut rules, &h("/h/app"), &other, NOW));
        assert!(mark_used(&mut rules, &h("/h/app"), &used, NOW));
        assert_eq!(rules[0].used_day, day_of(NOW));
        assert!(!mark_used(&mut rules, &h("/h/app"), &used, NOW));
    }

    #[test]
    fn the_shown_folder_is_its_path_from_its_root() {
        let roots = vec![h("/h/Documents/Code")];
        let home = &h("/h");
        let app = &h("/h/Documents/Code/Personal/app");
        assert_eq!(shown_folder(app, &roots, home), "Code/Personal/app");
        assert_eq!(shown_folder(&h("/h/Documents/Code"), &roots, home), "Code");
        assert_eq!(shown_folder(&h("/h/x"), &roots, home), "~/x");
        assert_eq!(shown_folder(Path::new("/opt/x"), &roots, home), "/opt/x");
        assert_eq!(
            shown_folder(&h("/h/Documents/Code/a\u{202e}b"), &roots, home),
            "Code/a?b"
        );
    }

    #[test]
    fn the_rule_line_fits_and_cuts_the_folder_from_the_left() {
        let one = [words(&["cargo", "test"])];
        assert_eq!(
            rule_line(&one, "Code/app").as_deref(),
            Some("cargo test * in Code/app")
        );
        let long = "Code/Personal/a-very-long-project-name/crates/bridge";
        let line = rule_line(&one, long).unwrap();
        assert_eq!(line.len(), MAX_LINE);
        assert!(line.starts_with("cargo test * in ..."), "{line}");
        assert!(line.ends_with("crates/bridge"), "{line}");
        let many = [
            words(&["cargo", "test"]),
            words(&["tail"]),
            words(&["some-long-tool-name"]),
        ];
        assert_eq!(rule_line(&many, long), None);
    }

    #[test]
    fn the_store_grants_lists_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let store = AlwaysRules::new(dir.path());
        let folder = &h("/h/app");
        store
            .grant(folder, Scope::Tree, &[words(&["make"])], NOW)
            .unwrap();
        let listed = store.list(NOW);
        assert_eq!(listed.len(), 1);
        assert!(!store.remove("ffff", NOW).unwrap());
        assert!(store.remove(&listed[0].id, NOW).unwrap());
        assert!(store.list(NOW).is_empty());
    }

    #[test]
    fn a_store_with_no_file_has_no_rules_and_grants_nothing() {
        let store = AlwaysRules::none();
        assert!(!store.is_on());
        assert!(store.list(NOW).is_empty());
        assert!(
            store
                .grant(&h("/h"), Scope::Tree, &[words(&["make"])], NOW)
                .is_err()
        );
        store.mark_used(&h("/h"), &[], NOW);
    }

    #[test]
    fn a_rule_is_one_or_two_plain_words_with_no_folder_in_the_name() {
        assert!(is_rule(&words(&["cargo", "test"])));
        assert!(!is_rule(&words(&[])));
        assert!(!is_rule(&words(&["/bin/ls"])));
        assert!(!is_rule(&words(&["ls", "-la"])));
        assert!(!is_rule(&words(&["a b"])));
    }
}
