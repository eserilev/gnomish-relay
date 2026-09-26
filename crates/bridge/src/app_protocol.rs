//! The app protocol between the bridge and the story program of Timeways (SPEC.md 9.8):
//! one JSON object on each line of stdin and stdout. The story program is untrusted, so
//! each line from it must have a fixed shape. `addon_lines` checks the lines that go to
//! it. No I/O here.

use protocol::slot::{Reply as SlotReply, Status, prepare_replies};
use protocol::wow_text::chat_safe;
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
/// The largest line from the story program. A model prompt fits.
pub const MAX_LINE: usize = 1024 * 1024;
/// The largest answer line. A reply record in the slot body is at most 32 KB (S12), and
/// the Lua escape makes some bytes longer.
pub const MAX_ANSWER_LINE: usize = 24_576;
pub const MAX_PROMPT: usize = 256 * 1024;
pub const MAX_COMPANION: usize = 1000;
const MAX_NAME: usize = 128;
const MAX_ANSWER: usize = 8 * 1024;
const MAX_PASSAGES: usize = 8;
const MAX_PASSAGE: usize = 4 * 1024;
const MAX_SOURCE: usize = 512;
const MAX_NPC: usize = 64;
/// 400 characters of at most 4 bytes each, on one line.
const MAX_TALK_ANSWER: usize = 1600;
const APP: &str = "timeways";

/// The number that ties a forwarded batch to its answer. The bridge counts up from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u64);

/// A model call of the story program. The story program counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallId(pub u64);

/// Why a line of the addon or of the story program was refused, for the log.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BadLine {
    TooLong,
    /// Not JSON, an unknown type or field, a value of the wrong type, or too deep.
    Shape,
    /// A text over its limit, or with a control character.
    Text,
    /// A character line with a text over its limit, or with a control character.
    Character,
}

/// A text from the game has no control character, so it stays on one line.
pub(crate) fn is_short(text: &str, max: usize) -> bool {
    text.len() <= max && !text.chars().any(char::is_control)
}

fn is_short_or_none(text: Option<&str>, max: usize) -> bool {
    text.is_none_or(|t| is_short(t, max))
}

/// A text for the game keeps its newlines and tabs.
fn is_printable(text: &str, max: usize) -> bool {
    text.len() <= max
        && !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

// Lines from the bridge to the story program.

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToStory {
    Hello { protocol: u32, app: &'static str },
    BatchEnd { id: RequestId },
    ModelFailed { call: CallId },
}

pub(crate) fn with_newline(json: Result<String, serde_json::Error>) -> String {
    // A value of strings and numbers always serializes.
    let mut line = json.unwrap_or_default();
    line.push('\n');
    line
}

pub fn hello_line() -> String {
    with_newline(serde_json::to_string(&ToStory::Hello {
        protocol: VERSION,
        app: APP,
    }))
}

/// After the last line of each batch.
pub fn batch_end_line(id: RequestId) -> String {
    with_newline(serde_json::to_string(&ToStory::BatchEnd { id }))
}

pub fn model_failed_line(call: CallId) -> String {
    with_newline(serde_json::to_string(&ToStory::ModelFailed { call }))
}

// Lines from the story program.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Passage {
    pub text: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub name: String,
    pub within: Option<String>,
    pub first_visit: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub name: String,
    pub place: Option<String>,
    pub first_met: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeedKind {
    Level,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deed {
    pub kind: DeedKind,
    pub from: Option<u32>,
    pub to: u32,
    pub at: u64,
    pub place: Option<String>,
}

/// What an answer says, as it goes back to the game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Body {
    /// `text` is `None` when the story program has only passages.
    LoreAnswer {
        text: Option<String>,
        passages: Vec<Passage>,
    },
    Journal {
        page: u32,
        pages: u32,
        places: Vec<Place>,
        people: Vec<Person>,
        deeds: Vec<Deed>,
    },
    /// What the NPC says. `text` is `None` when no model answered.
    TalkAnswer { npc: String, text: Option<String> },
    /// The answer to a batch of game events only.
    EventsSeen,
}

/// A checked answer. `companion` is a line of the companion of the player, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub body: Body,
    pub companion: Option<String>,
}

impl Answer {
    pub fn is_events_seen(&self) -> bool {
        self.body == Body::EventsSeen
    }
}

/// A companion line over its limit loses only that line; the rest of the answer stays.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CompanionCheck {
    Kept,
    Dropped,
}

/// A line from the story program, before its checks.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    Hello {
        protocol: u32,
    },
    LoreAnswer {
        id: RequestId,
        text: Option<String>,
        passages: Vec<Passage>,
        #[serde(default)]
        companion: Option<String>,
    },
    Journal {
        id: RequestId,
        page: u32,
        pages: u32,
        places: Vec<Place>,
        people: Vec<Person>,
        deeds: Vec<Deed>,
        #[serde(default)]
        companion: Option<String>,
    },
    TalkAnswer {
        id: RequestId,
        npc: String,
        text: Option<String>,
        #[serde(default)]
        companion: Option<String>,
    },
    EventsSeen {
        id: RequestId,
        #[serde(default)]
        companion: Option<String>,
    },
    ModelCall {
        call: CallId,
        prompt: String,
    },
}

/// A checked line from the story program.
#[derive(Debug, PartialEq, Eq)]
pub enum FromStory {
    Hello {
        protocol: u32,
    },
    /// The answer to the batch `id`. `None` for an answer line over `MAX_ANSWER_LINE`:
    /// the batch then gets an error, never a cut line.
    Answer {
        id: RequestId,
        answer: Option<Answer>,
        companion: CompanionCheck,
    },
    /// Step 6 of SPEC.md 9.7 runs the model. Until then the answer is `model_failed`.
    ModelCall {
        call: CallId,
        prompt: String,
    },
}

pub fn read_line(bytes: &[u8]) -> Result<FromStory, BadLine> {
    if bytes.len() > MAX_LINE {
        return Err(BadLine::TooLong);
    }
    let wire: Wire = serde_json::from_slice(bytes).map_err(|_| BadLine::Shape)?;
    let (id, body, companion) = match wire {
        Wire::Hello { protocol } => return Ok(FromStory::Hello { protocol }),
        Wire::ModelCall { call, prompt } if prompt.len() <= MAX_PROMPT => {
            return Ok(FromStory::ModelCall { call, prompt });
        }
        Wire::ModelCall { .. } => return Err(BadLine::TooLong),
        Wire::LoreAnswer {
            id,
            text,
            passages,
            companion,
        } => (id, Body::LoreAnswer { text, passages }, companion),
        Wire::Journal {
            id,
            page,
            pages,
            places,
            people,
            deeds,
            companion,
        } => {
            let journal = Body::Journal {
                page,
                pages,
                places,
                people,
                deeds,
            };
            (id, journal, companion)
        }
        Wire::TalkAnswer {
            id,
            npc,
            text,
            companion,
        } => (id, Body::TalkAnswer { npc, text }, companion),
        Wire::EventsSeen { id, companion } => (id, Body::EventsSeen, companion),
    };
    if !body_fits(&body) {
        return Err(BadLine::Text);
    }
    let (companion, check) = checked_companion(companion);
    let answer = (bytes.len() <= MAX_ANSWER_LINE).then_some(Answer { body, companion });
    Ok(FromStory::Answer {
        id,
        answer,
        companion: check,
    })
}

fn checked_companion(companion: Option<String>) -> (Option<String>, CompanionCheck) {
    match companion {
        Some(line) if !is_short(&line, MAX_COMPANION) => (None, CompanionCheck::Dropped),
        kept => (kept, CompanionCheck::Kept),
    }
}

fn body_fits(body: &Body) -> bool {
    match body {
        Body::LoreAnswer { text, passages } => {
            text.as_deref().is_none_or(|t| is_printable(t, MAX_ANSWER))
                && passages.len() <= MAX_PASSAGES
                && passages.iter().all(passage_fits)
        }
        Body::Journal {
            places,
            people,
            deeds,
            ..
        } => {
            places.iter().all(|p| {
                is_short(&p.name, MAX_NAME) && is_short_or_none(p.within.as_deref(), MAX_NAME)
            }) && people.iter().all(|p| {
                is_short(&p.name, MAX_NAME) && is_short_or_none(p.place.as_deref(), MAX_NAME)
            }) && deeds
                .iter()
                .all(|d| is_short_or_none(d.place.as_deref(), MAX_NAME))
        }
        Body::TalkAnswer { npc, text } => {
            is_short(npc, MAX_NPC) && is_short_or_none(text.as_deref(), MAX_TALK_ANSWER)
        }
        Body::EventsSeen => true,
    }
}

fn passage_fits(passage: &Passage) -> bool {
    is_printable(&passage.text, MAX_PASSAGE) && is_short(&passage.source, MAX_SOURCE)
}

// The reply for the game.

/// `note` is a line of the bridge, never of the story program.
#[derive(Serialize)]
struct Reply<'a> {
    #[serde(flatten)]
    body: Body,
    companion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'a str>,
}

/// The done reply of the batch: one JSON line, made from the checked answer. Every `|`
/// in a text comes out doubled (S10), so the game shows it as text. `None` when the
/// slot writer would cut the reply (S12), because a cut JSON line is no reply.
pub fn reply_text(answer: &Answer, note: Option<&str>) -> Option<String> {
    let reply = Reply {
        body: game_safe(&answer.body),
        companion: answer.companion.as_deref().map(game_text),
        note,
    };
    let line = serde_json::to_string(&reply).ok()?;
    fits_a_slot_record(&line).then_some(line)
}

fn fits_a_slot_record(text: &str) -> bool {
    let reply = SlotReply {
        chat: Vec::new(),
        id: 0,
        status: Status::Done,
        text: text.as_bytes().to_vec(),
    };
    prepare_replies(&[reply])
        .first()
        .is_some_and(|kept| kept.text.len() == text.len())
}

fn game_safe(body: &Body) -> Body {
    match body {
        Body::LoreAnswer { text, passages } => Body::LoreAnswer {
            text: text.as_deref().map(game_text),
            passages: passages.iter().map(safe_passage).collect(),
        },
        Body::Journal {
            page,
            pages,
            places,
            people,
            deeds,
        } => Body::Journal {
            page: *page,
            pages: *pages,
            places: places.iter().map(safe_place).collect(),
            people: people.iter().map(safe_person).collect(),
            deeds: deeds.iter().map(safe_deed).collect(),
        },
        Body::TalkAnswer { npc, text } => Body::TalkAnswer {
            npc: game_text(npc),
            text: text.as_deref().map(game_text),
        },
        Body::EventsSeen => Body::EventsSeen,
    }
}

fn safe_passage(passage: &Passage) -> Passage {
    Passage {
        text: game_text(&passage.text),
        source: game_text(&passage.source),
    }
}

fn safe_place(place: &Place) -> Place {
    Place {
        name: game_text(&place.name),
        within: place.within.as_deref().map(game_text),
        first_visit: place.first_visit,
    }
}

fn safe_person(person: &Person) -> Person {
    Person {
        name: game_text(&person.name),
        place: person.place.as_deref().map(game_text),
        first_met: person.first_met,
    }
}

fn safe_deed(deed: &Deed) -> Deed {
    Deed {
        place: deed.place.as_deref().map(game_text),
        ..deed.clone()
    }
}

/// `chat_safe` adds only `|` bytes after `|` bytes, so valid UTF-8 stays valid.
fn game_text(text: &str) -> String {
    String::from_utf8_lossy(&chat_safe(text.as_bytes())).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOURNAL: &str = r#"{"type":"journal","id":4,"page":0,"pages":2,"places":[{"name":"Goldshire","within":"Elwynn Forest","first_visit":100}],"people":[{"name":"Marshal Dughan","place":null,"first_met":101}],"deeds":[{"kind":"level","from":null,"to":2,"at":102,"place":"Goldshire"}]}"#;

    fn lore(text: Option<&str>, passages: Vec<Passage>) -> Answer {
        Answer {
            body: Body::LoreAnswer {
                text: text.map(str::to_owned),
                passages,
            },
            companion: None,
        }
    }

    fn read_answer(line: &[u8]) -> (Option<Answer>, CompanionCheck) {
        match read_line(line) {
            Ok(FromStory::Answer {
                answer, companion, ..
            }) => (answer, companion),
            other => panic!("not an answer: {other:?}"),
        }
    }

    fn answer_of(line: &[u8]) -> Option<Answer> {
        read_answer(line).0
    }

    #[test]
    fn the_hello_of_the_bridge_names_the_version_and_the_app() {
        assert_eq!(
            hello_line(),
            "{\"type\":\"hello\",\"protocol\":1,\"app\":\"timeways\"}\n"
        );
    }

    #[test]
    fn the_end_of_a_batch_and_a_failed_model_call_carry_their_ids() {
        assert_eq!(
            batch_end_line(RequestId(3)),
            "{\"type\":\"batch_end\",\"id\":3}\n"
        );
        assert_eq!(
            model_failed_line(CallId(4)),
            "{\"type\":\"model_failed\",\"call\":4}\n"
        );
    }

    #[test]
    fn a_lore_answer_reads_with_a_text_or_with_null() {
        let line = br#"{"type":"lore_answer","id":3,"text":"Goblins [1].","passages":[{"text":"It fell.","source":"https://example.test/1"}]}"#;
        let passage = Passage {
            text: "It fell.".into(),
            source: "https://example.test/1".into(),
        };
        assert_eq!(
            read_line(line),
            Ok(FromStory::Answer {
                id: RequestId(3),
                answer: Some(lore(Some("Goblins [1]."), vec![passage])),
                companion: CompanionCheck::Kept,
            })
        );
        let null = br#"{"type":"lore_answer","id":3,"text":null,"passages":[]}"#;
        assert_eq!(answer_of(null), Some(lore(None, Vec::new())));
    }

    #[test]
    fn a_journal_reads_with_its_places_people_and_deeds() {
        let Some(Answer {
            body:
                Body::Journal {
                    pages,
                    places,
                    people,
                    deeds,
                    ..
                },
            ..
        }) = answer_of(JOURNAL.as_bytes())
        else {
            panic!("not a journal");
        };
        assert_eq!(pages, 2);
        assert_eq!(places[0].within.as_deref(), Some("Elwynn Forest"));
        assert_eq!(people[0].place, None);
        assert_eq!(deeds[0].kind, DeedKind::Level);
        assert_eq!(deeds[0].from, None);
    }

    fn talk_answer(npc: &str, text: Option<&str>) -> String {
        serde_json::json!({ "type": "talk_answer", "id": 6, "npc": npc, "text": text }).to_string()
    }

    #[test]
    fn a_talk_answer_reads_with_a_text_or_with_null_and_its_reply_doubles_pipes() {
        let answer =
            answer_of(talk_answer("Marshal Dughan", Some("Kill |cff00ff00wolves.")).as_bytes())
                .unwrap();
        assert_eq!(
            reply_text(&answer, None).unwrap(),
            r#"{"type":"talk_answer","npc":"Marshal Dughan","text":"Kill ||cff00ff00wolves.","companion":null}"#
        );
        let silent = answer_of(talk_answer("Marshal Dughan", None).as_bytes()).unwrap();
        assert_eq!(
            silent.body,
            Body::TalkAnswer {
                npc: "Marshal Dughan".into(),
                text: None
            }
        );
    }

    #[test]
    fn a_talk_answer_of_1600_bytes_on_one_line_passes_and_more_is_refused() {
        let longest = talk_answer("n", Some(&"é".repeat(800)));
        assert!(answer_of(longest.as_bytes()).is_some());
        for bad in [
            talk_answer("n", Some(&format!("{}t", "é".repeat(800)))),
            talk_answer("n", Some("two\nlines")),
            talk_answer(&"n".repeat(65), None),
        ] {
            assert_eq!(read_line(bad.as_bytes()), Err(BadLine::Text), "{bad}");
        }
    }

    #[test]
    fn events_seen_reads_with_or_without_a_companion() {
        let quiet = answer_of(br#"{"type":"events_seen","id":5,"companion":null}"#).unwrap();
        assert!(quiet.is_events_seen());
        assert_eq!(quiet.companion, None);
        let talk = br#"{"type":"events_seen","id":5,"companion":"A wolf howls."}"#;
        assert_eq!(
            answer_of(talk).unwrap().companion.as_deref(),
            Some("A wolf howls.")
        );
        assert!(answer_of(br#"{"type":"events_seen","id":5}"#).is_some());
    }

    #[test]
    fn a_lore_answer_and_a_journal_take_a_companion_too() {
        let lore = br#"{"type":"lore_answer","id":3,"text":null,"passages":[],"companion":"Hm."}"#;
        assert_eq!(answer_of(lore).unwrap().companion.as_deref(), Some("Hm."));
        let journal = JOURNAL.replace(r#""id":4,"#, r#""id":4,"companion":"Hm.","#);
        assert_eq!(
            answer_of(journal.as_bytes()).unwrap().companion.as_deref(),
            Some("Hm.")
        );
    }

    #[test]
    fn a_companion_of_1000_bytes_stays_and_a_longer_one_is_dropped_alone() {
        let line = |companion: &str| {
            serde_json::json!({ "type": "events_seen", "id": 5, "companion": companion })
                .to_string()
        };
        let (kept, check) = read_answer(line(&"c".repeat(1000)).as_bytes());
        assert_eq!(check, CompanionCheck::Kept);
        assert_eq!(kept.unwrap().companion.map(|c| c.len()), Some(1000));

        let (dropped, check) = read_answer(line(&"c".repeat(1001)).as_bytes());
        assert_eq!(check, CompanionCheck::Dropped);
        assert_eq!(
            dropped.unwrap(),
            Answer {
                body: Body::EventsSeen,
                companion: None
            }
        );
        let (_, check) = read_answer(line("a\nb").as_bytes());
        assert_eq!(check, CompanionCheck::Dropped);
    }

    #[test]
    fn the_hello_and_a_model_call_of_the_story_program_read() {
        assert_eq!(
            read_line(br#"{"type":"hello","protocol":1}"#),
            Ok(FromStory::Hello { protocol: 1 })
        );
        assert_eq!(
            read_line(br#"{"type":"model_call","call":1,"prompt":"tell"}"#),
            Ok(FromStory::ModelCall {
                call: CallId(1),
                prompt: "tell".into()
            })
        );
    }

    #[test]
    fn a_line_of_the_story_program_of_the_wrong_shape_is_refused() {
        let bad = [
            "not json".to_owned(),
            String::new(),
            r#"{"type":"lore_answer","id":3}"#.to_owned(),
            r#"{"type":"shell","command":"rm -rf ~"}"#.to_owned(),
            r#"{"type":"hello","protocol":1,"name":"extra"}"#.to_owned(),
            r#"{"type":"hello","protocol":-1}"#.to_owned(),
            r#"{"type":"lore_answer","id":3,"text":"x","passages":[{"text":"a","source":"b","links":[]}]}"#.to_owned(),
            r#"{"type":"lore_answer","id":3.5,"text":"x","passages":[]}"#.to_owned(),
            r#"{"type":"hello","protocol":1,"protocol":2}"#.to_owned(),
            r#"{"type":"events_seen","companion":null}"#.to_owned(),
            r#"{"type":"events_seen","id":5,"companion":7}"#.to_owned(),
            JOURNAL.replace(r#""kind":"level""#, r#""kind":"murder""#),
            JOURNAL.replace(r#""first_met":101"#, r#""first_met":101,"secret":1"#),
            JOURNAL.replace(r#""pages":2,"#, ""),
        ];
        for line in bad {
            assert_eq!(read_line(line.as_bytes()), Err(BadLine::Shape), "{line}");
        }
    }

    #[test]
    fn an_answer_over_its_limits_is_refused() {
        let long = format!(
            r#"{{"type":"lore_answer","id":3,"text":"{}","passages":[]}}"#,
            "x".repeat(MAX_ANSWER + 1)
        );
        assert_eq!(read_line(long.as_bytes()), Err(BadLine::Text));
        let passage = r#"{"text":"a","source":"b"}"#;
        let many = format!(
            r#"{{"type":"lore_answer","id":3,"text":null,"passages":[{}]}}"#,
            [passage; MAX_PASSAGES + 1].join(",")
        );
        assert_eq!(read_line(many.as_bytes()), Err(BadLine::Text));
        let control = br#"{"type":"lore_answer","id":3,"text":"a\u0000b","passages":[]}"#;
        assert_eq!(read_line(control), Err(BadLine::Text));
        let name = JOURNAL.replace("Goldshire\",\"within", "Gold\\nshire\",\"within");
        assert_eq!(read_line(name.as_bytes()), Err(BadLine::Text));
    }

    /// A lore answer line of `len` bytes: six passages of 3500 bytes, and a first one
    /// that fills the rest.
    fn answer_line(len: usize) -> String {
        let line = |first: usize| {
            let mut texts = vec!["p".repeat(first)];
            texts.extend((0..6).map(|_| "p".repeat(3500)));
            let passages: Vec<String> = texts
                .iter()
                .map(|t| format!(r#"{{"text":"{t}","source":""}}"#))
                .collect();
            format!(
                r#"{{"type":"lore_answer","id":3,"text":null,"passages":[{}]}}"#,
                passages.join(",")
            )
        };
        line(len - line(0).len())
    }

    #[test]
    fn an_answer_line_of_the_limit_passes_and_one_byte_more_asks_for_an_error() {
        let at_limit = answer_line(MAX_ANSWER_LINE);
        let over = answer_line(MAX_ANSWER_LINE + 1);
        assert_eq!((at_limit.len(), over.len()), (24_576, 24_577));

        let kept = answer_of(at_limit.as_bytes());

        assert!(
            reply_text(&kept.unwrap(), None).is_some(),
            "it fits a slot record"
        );
        assert_eq!(answer_of(over.as_bytes()), None);
    }

    #[test]
    fn a_model_prompt_over_the_limit_is_refused() {
        let long = format!(
            r#"{{"type":"model_call","call":9,"prompt":"{}"}}"#,
            "p".repeat(MAX_PROMPT + 1)
        );
        assert_eq!(read_line(long.as_bytes()), Err(BadLine::TooLong));
        assert_eq!(read_line(&vec![b' '; MAX_LINE + 1]), Err(BadLine::TooLong));
    }

    #[test]
    fn the_reply_is_the_checked_answer_with_every_pipe_doubled() {
        let passage = Passage {
            text: "a|b".into(),
            source: "https://x/|".into(),
        };
        let mut answer = lore(Some("see |Hitem:1|h[x]|h\nnow"), vec![passage]);
        answer.companion = Some("|cffff0000red".into());
        assert_eq!(
            reply_text(&answer, None).unwrap(),
            r#"{"type":"lore_answer","text":"see ||Hitem:1||h[x]||h\nnow","passages":[{"text":"a||b","source":"https://x/||"}],"companion":"||cffff0000red"}"#
        );
        assert_eq!(
            reply_text(&lore(None, Vec::new()), None).unwrap(),
            r#"{"type":"lore_answer","text":null,"passages":[],"companion":null}"#
        );
    }

    #[test]
    fn the_reply_to_events_seen_has_only_the_companion() {
        let answer = Answer {
            body: Body::EventsSeen,
            companion: Some("A wolf howls.".into()),
        };
        assert_eq!(
            reply_text(&answer, None).unwrap(),
            r#"{"type":"events_seen","companion":"A wolf howls."}"#
        );
    }

    #[test]
    fn the_reply_of_a_journal_doubles_every_pipe_in_its_names() {
        let journal = JOURNAL.replace("Marshal Dughan", "Marshal |cffff0000Dughan");
        let answer = answer_of(journal.as_bytes()).unwrap();

        let reply = reply_text(&answer, None).unwrap();

        assert!(reply.starts_with(r#"{"type":"journal","page":0,"pages":2,"places""#));
        assert!(
            reply.contains(r#""name":"Marshal ||cffff0000Dughan""#),
            "{reply}"
        );
        assert!(!reply.contains(r#""id""#));
    }

    #[test]
    fn a_note_of_the_bridge_goes_last_in_the_reply() {
        assert_eq!(
            reply_text(&lore(None, Vec::new()), Some("no sandbox")).unwrap(),
            r#"{"type":"lore_answer","text":null,"passages":[],"companion":null,"note":"no sandbox"}"#
        );
    }

    #[test]
    fn a_reply_that_the_slot_writer_would_cut_is_none() {
        let passage = Passage {
            text: "é".repeat(MAX_PASSAGE / 2),
            source: String::new(),
        };
        let wide = lore(None, vec![passage; MAX_PASSAGES]);

        assert_eq!(reply_text(&wide, None), None);
    }
}
