//! The lines of a batch from the Timeways addon (SPEC.md 9.8). The addon is untrusted.
//! Three types have a fixed shape. Any other type is a game event: a checked JSON object
//! that goes on to the story program, which checks its fields for itself. So a new event
//! of Timeways needs no change here. No I/O here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::app_protocol::{BadLine, RequestId, is_short, with_newline};

pub const MAX_ADDON_LINE: usize = 4096;
const MAX_TYPE: usize = 32;
/// A flat object has depth 1.
const MAX_DEPTH: usize = 4;
/// All the keys of a line, at every depth.
const MAX_KEYS: usize = 64;
const MAX_NAME: usize = 128;
const MAX_REALM: usize = 64;
const MAX_CHARACTER: usize = 48;
const MAX_QUESTION: usize = 1024;
const MAX_NPC: usize = 64;
const MAX_TALK: usize = 255;

/// The lines with a fixed shape: the character, and the two lines that get a reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Known {
    /// The story program makes file names from these, as safe ids (9.7, decision 19).
    CharacterEntered { realm: String, name: String },
    LoreAsked {
        at: u64,
        question: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    JournalAsked {
        #[serde(default)]
        page: u32,
    },
    /// The player talks to an NPC.
    TalkAsked { at: u64, npc: String, text: String },
}

const KNOWN: [&str; 4] = [
    "character_entered",
    "lore_asked",
    "journal_asked",
    "talk_asked",
];

#[derive(Clone, Debug, PartialEq)]
pub enum AddonLine {
    Known(Known),
    /// Any other type: a game event with no reply.
    Event(Map<String, Value>),
}

impl AddonLine {
    /// The character line and a game event give no output.
    pub fn wants_reply(&self) -> bool {
        matches!(
            self,
            AddonLine::Known(
                Known::LoreAsked { .. } | Known::JournalAsked { .. } | Known::TalkAsked { .. }
            )
        )
    }

    fn is_character(&self) -> bool {
        matches!(self, AddonLine::Known(Known::CharacterEntered { .. }))
    }
}

/// A character line over its limits is `Character`: it refuses the whole batch.
pub fn read_addon_line(line: &str) -> Result<AddonLine, BadLine> {
    if line.len() > MAX_ADDON_LINE {
        return Err(BadLine::TooLong);
    }
    let value: Value = serde_json::from_str(line).map_err(|_| BadLine::Shape)?;
    let kind = type_of(&value).ok_or(BadLine::Shape)?;
    if depth(&value) > MAX_DEPTH || key_count(&value) > MAX_KEYS {
        return Err(BadLine::Shape);
    }
    if !strings_are_clean(&value) && kind == "character_entered" {
        return Err(BadLine::Character);
    }
    if !strings_are_clean(&value) {
        return Err(BadLine::Text);
    }
    if KNOWN.contains(&kind.as_str()) {
        return read_known(line).map(AddonLine::Known);
    }
    match value {
        Value::Object(map) => Ok(AddonLine::Event(map)),
        _ => Err(BadLine::Shape),
    }
}

/// The `type` of an object with no `id`: the bridge adds the id, never the addon.
fn type_of(value: &Value) -> Option<String> {
    let map = value.as_object()?;
    let kind = map.get("type")?.as_str()?;
    let name_ok = (1..=MAX_TYPE).contains(&kind.len())
        && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    (name_ok && !map.contains_key("id")).then(|| kind.to_owned())
}

fn depth(value: &Value) -> usize {
    match value {
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn key_count(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len() + map.values().map(key_count).sum::<usize>(),
        Value::Array(items) => items.iter().map(key_count).sum(),
        _ => 0,
    }
}

fn strings_are_clean(value: &Value) -> bool {
    let clean = |text: &str| !text.chars().any(char::is_control);
    match value {
        Value::String(text) => clean(text),
        Value::Object(map) => map.iter().all(|(k, v)| clean(k) && strings_are_clean(v)),
        Value::Array(items) => items.iter().all(strings_are_clean),
        _ => true,
    }
}

/// From the raw line, so a key given twice is an error, as for the story program.
fn read_known(line: &str) -> Result<Known, BadLine> {
    let known: Known = serde_json::from_str(line).map_err(|_| BadLine::Shape)?;
    match &known {
        Known::CharacterEntered { realm, name }
            if !(is_short(realm, MAX_REALM) && is_short(name, MAX_CHARACTER)) =>
        {
            Err(BadLine::Character)
        }
        Known::LoreAsked {
            question, target, ..
        } if !(is_short(question, MAX_QUESTION)
            && target.as_deref().is_none_or(|t| is_short(t, MAX_NAME))) =>
        {
            Err(BadLine::Text)
        }
        Known::TalkAsked { npc, text, .. }
            if npc.is_empty() || !(is_short(npc, MAX_NPC) && is_short(text, MAX_TALK)) =>
        {
            Err(BadLine::Text)
        }
        _ => Ok(known),
    }
}

/// Made from the checked value, never from the raw bytes of the addon.
pub fn forwarded_line(id: RequestId, line: &AddonLine) -> String {
    let mut map = match line {
        AddonLine::Known(known) => match serde_json::to_value(known) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        AddonLine::Event(map) => map.clone(),
    };
    map.insert("id".into(), id.0.into());
    with_newline(serde_json::to_string(&map))
}

/// The good lines of a batch, and the bad lines that were dropped.
#[derive(Debug, PartialEq)]
pub struct Batch {
    pub lines: Vec<AddonLine>,
    pub dropped: Vec<BadLine>,
}

/// Why a whole batch was refused.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refused {
    /// A character line that is not first, or a line with a reply that is not last.
    Order,
    /// A character line with a long realm or name, or with a control character.
    Character,
}

/// A batch is an optional character line, then game events, then at most one line with
/// a reply. A bad line is dropped. A bad character line or a wrong order refuses it all.
pub fn read_batch(text: &str) -> Result<Batch, Refused> {
    let mut batch = Batch {
        lines: Vec::new(),
        dropped: Vec::new(),
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match read_addon_line(line) {
            Ok(line) => batch.lines.push(line),
            Err(BadLine::Character) => return Err(Refused::Character),
            Err(bad) => batch.dropped.push(bad),
        }
    }
    if !is_in_order(&batch.lines) {
        return Err(Refused::Order);
    }
    Ok(batch)
}

fn is_in_order(lines: &[AddonLine]) -> bool {
    let last = lines.len().saturating_sub(1);
    let reply_before_last = lines[..last].iter().any(AddonLine::wants_reply);
    let character_after_first = lines.iter().skip(1).any(AddonLine::is_character);
    !reply_before_last && !character_after_first
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHARACTER: &str = r#"{"type":"character_entered","realm":"Stormrage","name":"Anduin"}"#;
    const EVENT: &str = r#"{"type":"npc_met","at":2,"name":"Hogger"}"#;
    const QUESTION: &str = r#"{"type":"lore_asked","at":3,"question":"why?"}"#;
    const JOURNAL: &str = r#"{"type":"journal_asked"}"#;

    fn batch(lines: &[&str]) -> Result<Batch, Refused> {
        read_batch(&lines.join("\n"))
    }

    fn character(realm: &str, name: &str) -> String {
        serde_json::json!({ "type": "character_entered", "realm": realm, "name": name }).to_string()
    }

    fn forwarded(line: &str) -> Value {
        let line = read_addon_line(line).unwrap();
        serde_json::from_str(&forwarded_line(RequestId(7), &line)).unwrap()
    }

    #[test]
    fn a_new_event_passes_through_as_checked_json_with_the_id() {
        let line = r#"{ "type": "npc_defeated", "at": 5, "name": "Hogger" }"#;

        assert_eq!(
            forwarded(line),
            serde_json::json!({ "type": "npc_defeated", "at": 5, "name": "Hogger", "id": 7 })
        );
        assert!(!read_addon_line(line).unwrap().wants_reply());
    }

    #[test]
    fn the_events_of_today_pass_through() {
        let lines = [
            r#"{"type":"zone_entered","at":1,"zone":"Elwynn Forest","subzone":"Goldshire"}"#,
            r#"{"type":"npc_met","at":2,"name":"Marshal Dughan"}"#,
            r#"{"type":"level_reached","at":3,"level":12}"#,
            r#"{"type":"died","at":4,"place":{"zone":"Duskwood","near":["Darkshire"]}}"#,
        ];
        for line in lines {
            assert!(
                matches!(read_addon_line(line), Ok(AddonLine::Event(_))),
                "{line}"
            );
        }
    }

    #[test]
    fn the_known_lines_read_with_their_shapes() {
        assert_eq!(
            read_addon_line(r#"{"type":"journal_asked"}"#),
            Ok(AddonLine::Known(Known::JournalAsked { page: 0 }))
        );
        assert_eq!(
            forwarded(r#"{"type":"lore_asked","at":4,"question":"why?","target":"Hogger"}"#),
            serde_json::json!({
                "type": "lore_asked", "at": 4, "question": "why?", "target": "Hogger", "id": 7
            })
        );
        assert!(read_addon_line(CHARACTER).is_ok());
    }

    #[test]
    fn a_line_that_is_not_an_object_with_a_good_type_is_refused() {
        let bad = [
            "not json",
            "[1,2]",
            r#""npc_met""#,
            r#"{"at":1}"#,
            r#"{"type":7}"#,
            r#"{"type":""}"#,
            r#"{"type":"NPC_met"}"#,
            r#"{"type":"npc-met"}"#,
            &format!(r#"{{"type":"{}"}}"#, "e".repeat(33)),
        ];
        for line in bad {
            assert_eq!(read_addon_line(line), Err(BadLine::Shape), "{line}");
        }
        let longest = format!(r#"{{"type":"{}"}}"#, "e".repeat(32));
        assert!(read_addon_line(&longest).is_ok());
    }

    #[test]
    fn a_line_with_an_id_of_the_addon_is_refused() {
        assert_eq!(
            read_addon_line(r#"{"type":"npc_met","at":2,"name":"x","id":9}"#),
            Err(BadLine::Shape)
        );
        assert_eq!(
            read_addon_line(r#"{"type":"lore_asked","at":2,"question":"x","id":9}"#),
            Err(BadLine::Shape)
        );
    }

    #[test]
    fn a_line_nested_four_deep_passes_and_five_deep_is_refused() {
        let four = r#"{"type":"e","a":{"b":{"c":[1]}}}"#;
        let five = r#"{"type":"e","a":{"b":{"c":[[1]]}}}"#;
        assert!(read_addon_line(four).is_ok());
        assert_eq!(read_addon_line(five), Err(BadLine::Shape));
    }

    #[test]
    fn a_line_with_64_keys_passes_and_65_is_refused() {
        let line = |keys: usize| {
            let mut map = Map::new();
            map.insert("type".into(), "e".into());
            for n in 1..keys {
                map.insert(format!("k{n}"), n.into());
            }
            Value::Object(map).to_string()
        };
        assert!(read_addon_line(&line(64)).is_ok());
        assert_eq!(read_addon_line(&line(65)), Err(BadLine::Shape));
    }

    #[test]
    fn a_control_character_in_any_string_or_key_is_refused() {
        let bad = [
            r#"{"type":"npc_met","name":"a\nb"}"#,
            r#"{"type":"e","list":["ok","a\u0007b"]}"#,
            r#"{"type":"e","a\tb":1}"#,
            r#"{"type":"lore_asked","at":1,"question":"a\u0000b"}"#,
        ];
        for line in bad {
            assert_eq!(read_addon_line(line), Err(BadLine::Text), "{line}");
        }
    }

    #[test]
    fn a_line_over_the_size_limit_is_refused() {
        let long = format!(r#"{{"type":"e","x":"{}"}}"#, "x".repeat(MAX_ADDON_LINE));
        assert_eq!(read_addon_line(&long), Err(BadLine::TooLong));
    }

    #[test]
    fn a_known_line_keeps_its_exact_shape() {
        let bad = [
            r#"{"type":"lore_asked","at":1}"#,
            r#"{"type":"lore_asked","at":1,"question":"x","mood":"sad"}"#,
            r#"{"type":"journal_asked","page":-1}"#,
            r#"{"type":"journal_asked","page":1,"page":2}"#,
            r#"{"type":"character_entered","realm":"x"}"#,
        ];
        for line in bad {
            assert_eq!(read_addon_line(line), Err(BadLine::Shape), "{line}");
        }
        let long = format!(
            r#"{{"type":"lore_asked","at":1,"question":"{}"}}"#,
            "q".repeat(MAX_QUESTION + 1)
        );
        assert_eq!(read_addon_line(&long), Err(BadLine::Text));
    }

    fn talk(npc: &str, text: &str) -> String {
        serde_json::json!({ "type": "talk_asked", "at": 1, "npc": npc, "text": text }).to_string()
    }

    #[test]
    fn talk_asked_is_a_line_with_a_reply_and_keeps_its_shape() {
        let line = read_addon_line(&talk("Marshal Dughan", "Any work?")).unwrap();
        assert!(line.wants_reply());
        assert_eq!(
            forwarded(&talk("Marshal Dughan", "Any work?")),
            serde_json::json!({
                "type": "talk_asked", "at": 1, "npc": "Marshal Dughan", "text": "Any work?", "id": 7
            })
        );
        let extra = r#"{"type":"talk_asked","at":1,"npc":"x","text":"y","mood":"sad"}"#;
        assert_eq!(read_addon_line(extra), Err(BadLine::Shape));
    }

    #[test]
    fn a_talk_npc_of_1_to_64_bytes_and_a_text_of_255_bytes_pass_and_more_is_refused() {
        assert!(read_addon_line(&talk("n", "")).is_ok());
        assert!(read_addon_line(&talk(&"é".repeat(32), &"t".repeat(255))).is_ok());
        for bad in [
            talk("", "hi"),
            talk(&"n".repeat(65), "hi"),
            talk("n", &"t".repeat(256)),
        ] {
            assert_eq!(read_addon_line(&bad), Err(BadLine::Text), "{bad}");
        }
    }

    #[test]
    fn talk_asked_is_last_in_its_batch_like_the_other_reply_lines() {
        let talk = talk("Marshal Dughan", "Any work?");
        assert!(batch(&[CHARACTER, EVENT, &talk]).is_ok());
        assert_eq!(batch(&[&talk, EVENT]), Err(Refused::Order));
        assert_eq!(batch(&[&talk, QUESTION]), Err(Refused::Order));
    }

    #[test]
    fn a_batch_drops_its_bad_lines_and_skips_blank_ones() {
        let read = read_batch(&format!("{EVENT}\n\n  \nbad\n{QUESTION}")).unwrap();
        assert_eq!(read.lines.len(), 2);
        assert_eq!(read.dropped, [BadLine::Shape]);
    }

    #[test]
    fn a_batch_has_the_character_first_then_events_then_one_reply_line_last() {
        let good: [&[&str]; 6] = [
            &[],
            &[CHARACTER],
            &[CHARACTER, EVENT, EVENT, QUESTION],
            &[EVENT, JOURNAL],
            &[CHARACTER, JOURNAL],
            &[QUESTION],
        ];
        for lines in good {
            assert!(batch(lines).is_ok(), "{lines:?}");
        }
    }

    #[test]
    fn a_batch_out_of_order_is_refused() {
        let bad: [&[&str]; 5] = [
            &[EVENT, CHARACTER],
            &[CHARACTER, CHARACTER],
            &[QUESTION, EVENT],
            &[JOURNAL, QUESTION],
            &[CHARACTER, QUESTION, JOURNAL],
        ];
        for lines in bad {
            assert_eq!(batch(lines), Err(Refused::Order), "{lines:?}");
        }
    }

    #[test]
    fn a_realm_of_64_bytes_and_a_name_of_48_bytes_pass_and_one_more_refuses_the_batch() {
        let longest = character(&"r".repeat(64), &"é".repeat(24));
        assert!(batch(&[&longest, EVENT]).is_ok());

        let realm = character(&"r".repeat(65), "Anduin");
        let name = character("Stormrage", &format!("{}n", "é".repeat(24)));
        assert_eq!(batch(&[&realm, EVENT]), Err(Refused::Character));
        assert_eq!(batch(&[&name, EVENT]), Err(Refused::Character));
    }

    #[test]
    fn a_character_line_with_a_control_character_refuses_the_batch() {
        let control = character("Storm\trage", "Anduin");
        assert_eq!(read_addon_line(&control), Err(BadLine::Character));
        assert_eq!(batch(&[&control, EVENT]), Err(Refused::Character));
    }
}
