//! The structured log of the bridge: a span for each message and each desktop request,
//! the old line on stderr, and a JSON-lines file (SPEC.md 8.5).

use std::fmt::Debug;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use protocol::action::ToolCall;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Span, Subscriber};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

use crate::iso_time::iso_time;
use crate::log_command::command_field;
use crate::log_file::{FILES, LOG_DIR, MAX_FILE_BYTES, RotatingFile};
use crate::relay::Job;

const DEFAULT_LEVEL: &str = "info";
/// The event field that holds the text of `log`.
const MESSAGE: &str = "message";

/// The fields of one span or one event, in the order of their names in the code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields(Vec<(&'static str, String)>);

impl Fields {
    fn set(&mut self, name: &'static str, value: String) {
        match self.0.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.0.push((name, value)),
        }
    }

    fn take(&mut self, name: &str) -> Option<String> {
        let at = self.0.iter().position(|(n, _)| *n == name)?;
        Some(self.0.remove(at).1)
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field.name(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.set(field.name(), format!("{value:?}"));
    }
}

/// `<unix seconds> <line>` as before, then ` key=value` for each field (SPEC.md 8.5).
pub fn human_line(unix: u64, line: &str, fields: &Fields) -> String {
    let mut out = format!("{unix} {}", line.escape_debug());
    for (name, value) in &fields.0 {
        out.push(' ');
        out.push_str(name);
        out.push('=');
        out.push_str(&human_value(value));
    }
    out
}

fn human_value(value: &str) -> String {
    let escaped = value.escape_debug().to_string();
    if escaped.is_empty() || escaped.contains([' ', '"']) {
        return format!("\"{escaped}\"");
    }
    escaped
}

pub fn json_line(millis: u128, level: &str, line: &str, fields: &Fields) -> String {
    let mut object = serde_json::Map::new();
    object.insert("time".into(), iso_time(millis).into());
    let unix = u64::try_from(millis / 1000).unwrap_or(u64::MAX);
    object.insert("unix".into(), unix.into());
    object.insert("level".into(), level.into());
    object.insert("line".into(), line.into());
    for (name, value) in &fields.0 {
        object.insert((*name).into(), value.clone().into());
    }
    serde_json::Value::Object(object).to_string()
}

/// When a span opened, for the line that says how long it took.
struct Opened(Instant);

/// Writes each event to stderr and to the JSON file, and one line when a span closes.
pub struct LogLayer {
    stderr: Mutex<Box<dyn Write + Send>>,
    file: Option<Mutex<RotatingFile>>,
}

impl LogLayer {
    pub fn new(stderr: Box<dyn Write + Send>, file: Option<RotatingFile>) -> LogLayer {
        LogLayer {
            stderr: Mutex::new(stderr),
            file: file.map(Mutex::new),
        }
    }

    fn write_stderr(&self, line: &str) {
        if let Ok(mut stderr) = self.stderr.lock() {
            let _ = writeln!(stderr, "{line}");
        }
    }

    fn write(&self, level: &str, line: &str, fields: &Fields) {
        let millis = now_millis();
        let unix = u64::try_from(millis / 1000).unwrap_or(u64::MAX);
        self.write_stderr(&human_line(unix, line, fields));
        self.write_file(&json_line(millis, level, line, fields));
    }

    /// A file that fails says so once on stderr, and the log goes on there.
    fn write_file(&self, line: &str) {
        let Some(file) = &self.file else {
            return;
        };
        let Ok(mut file) = file.lock() else {
            return;
        };
        if let Err(e) = file.write_line(line) {
            self.write_stderr(&format!("cannot write the JSON log: {e:#}"));
        }
    }
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &tracing::span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        let mut extensions = span.extensions_mut();
        extensions.insert(fields);
        extensions.insert(Opened(Instant::now()));
    }

    fn on_record(&self, id: &tracing::span::Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        if let Some(fields) = span.extensions_mut().get_mut::<Fields>() {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = span_fields(event, &ctx);
        let mut own = Fields::default();
        event.record(&mut own);
        let line = own.take(MESSAGE).unwrap_or_default();
        fields.0.extend(own.0);
        let level = event.metadata().level().as_str().to_ascii_lowercase();
        self.write(&level, &line, &fields);
    }

    /// How long a message, a question, or a desktop request took (SPEC.md 8.5).
    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(Opened(opened)) = span.extensions().get::<Opened>().map(|o| Opened(o.0)) else {
            return;
        };
        let millis = opened.elapsed().as_millis();
        let mut fields = Fields::default();
        for each in span.scope().from_root() {
            if let Some(own) = each.extensions().get::<Fields>() {
                for (name, value) in &own.0 {
                    fields.set(name, value.clone());
                }
            }
        }
        fields.set("duration_ms", millis.to_string());
        let line = format!("{} took {millis} ms", span.name());
        self.write("info", &line, &fields);
    }
}

/// The fields of each span around `event`, the outer span first.
fn span_fields<S>(event: &Event<'_>, ctx: &Context<'_, S>) -> Fields
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let mut all = Fields::default();
    let Some(scope) = ctx.event_scope(event) else {
        return all;
    };
    for span in scope.from_root() {
        if let Some(fields) = span.extensions().get::<Fields>() {
            for (name, value) in &fields.0 {
                all.set(name, value.clone());
            }
        }
    }
    all
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// The filter of `RUST_LOG`, and the problem with it when it does not parse.
pub fn level_filter(rust_log: Option<&str>) -> (Targets, Option<String>) {
    let default = || Targets::new().with_default(tracing::Level::INFO);
    let Some(text) = rust_log.filter(|t| !t.trim().is_empty()) else {
        return (default(), None);
    };
    match Targets::from_str(text) {
        Ok(targets) => (targets, None),
        Err(e) => (
            default(),
            Some(format!(
                "RUST_LOG {text:?} does not parse ({e}), so the level is {DEFAULT_LEVEL}"
            )),
        ),
    }
}

/// Starts the log of `run`: stderr, and the JSON file in the data folder.
pub fn start(data: &Path) {
    let (filter, problem) = level_filter(std::env::var("RUST_LOG").ok().as_deref());
    let file = RotatingFile::new(&data.join(LOG_DIR), MAX_FILE_BYTES, FILES);
    let layer = LogLayer::new(Box::new(std::io::stderr()), Some(file));
    let subscriber = Registry::default().with(layer.with_filter(filter));
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        return;
    }
    if let Some(problem) = problem {
        crate::run::log(&problem);
    }
}

/// With no subscriber, as in `setup` and in most tests, `log` keeps its plain line.
pub fn is_on() -> bool {
    tracing::dispatcher::get_default(|d| !d.is::<tracing::subscriber::NoSubscriber>())
}

/// The span of one message of a chat. It holds no text of the message.
pub fn message_span(job: &Job) -> Span {
    tracing::info_span!(
        "message",
        chat = %job.chat,
        message_id = job.id.0,
        agent = job.agent.as_str(),
        permission = job.permission.word(),
        folder = job.cwd.as_str(),
    )
}

/// The real folder of the run in progress, once it is known.
pub fn record_folder(folder: &str) {
    Span::current().record("folder", folder);
}

pub fn record_permission(word: &str) {
    Span::current().record("permission", word);
}

/// The full command of a shell call, with its secrets hidden. Other calls get no span.
pub fn command_span(tool: &ToolCall) -> Span {
    match tool {
        ToolCall::Command { raw, .. } => {
            tracing::info_span!("command", command = command_field(raw).as_str())
        }
        ToolCall::Files { .. } | ToolCall::Unknown => Span::none(),
    }
}

pub fn request_span(id: &str, kind: &str) -> Span {
    tracing::info_span!("desktop_request", request = id, kind = kind)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::Permission;
    use crate::lane::{ChatId, MessageId};
    use crate::relay::{Session, Work};

    /// stderr of a test, shared with the layer.
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn subscriber(stderr: &Buffer, file: Option<RotatingFile>) -> impl Subscriber + Send + Sync {
        let layer = LogLayer::new(Box::new(stderr.clone()), file);
        Registry::default().with(layer.with_filter(level_filter(None).0))
    }

    fn job(cwd: &str) -> Job {
        Job {
            token: "t".into(),
            chat: ChatId::new("c7"),
            id: MessageId(12),
            agent: "claude".into(),
            permission: Permission::AutoEdit,
            asked: Permission::AutoEdit,
            cwd: cwd.into(),
            session: Session::New,
            resume: None,
            text: "secret prompt text".into(),
            work: Work::Prompt,
            new_folder: false,
        }
    }

    #[test]
    fn a_message_span_carries_chat_message_id_and_folder() {
        let stderr = Buffer::default();
        let dir = tempfile::tempdir().unwrap();
        let file = RotatingFile::new(dir.path(), MAX_FILE_BYTES, FILES);

        tracing::subscriber::with_default(subscriber(&stderr, Some(file)), || {
            let _message = message_span(&job("/w/app")).entered();
            record_folder("/w/app-real");
            crate::run::log("run c7 #12 with claude at AutoEdit");
        });

        let text = stderr.text();
        let line = text.lines().next().unwrap();
        assert!(line.ends_with(
            " run c7 #12 with claude at AutoEdit chat=c7 message_id=12 agent=claude permission=auto-edit folder=/w/app-real"
        ), "{text}");
        let log = std::fs::read_to_string(dir.path().join(crate::log_file::JSON_LOG)).unwrap();
        let json: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(json["chat"], "c7");
        assert_eq!(json["message_id"], "12");
        assert_eq!(json["folder"], "/w/app-real");
        assert_eq!(json["level"], "info");
        assert_eq!(json["line"], "run c7 #12 with claude at AutoEdit");
        assert!(!text.contains("secret prompt text"));
    }

    #[test]
    fn a_desktop_request_event_carries_its_full_command() {
        let stderr = Buffer::default();
        let data = tempfile::tempdir().unwrap();
        let approvals = crate::desktop::Approvals::new(data.path(), crate::desktop::Prompt::Off);
        let call = ToolCall::Command {
            raw: b"cargo test -p bridge --release".to_vec(),
            cwd: b"/w/app".to_vec(),
        };

        let id = tracing::subscriber::with_default(subscriber(&stderr, None), || {
            let _message = message_span(&job("/w/app")).entered();
            let _command = command_span(&call).entered();
            approvals
                .open("claude", "/w/app", "popup text", "command cargo", 1)
                .unwrap()
                .id
        });

        let text = stderr.text();
        let line = text
            .lines()
            .find(|l| l.contains("desktop request"))
            .unwrap();
        assert!(
            line.contains(" command=\"cargo test -p bridge --release\""),
            "{line}"
        );
        assert!(
            line.contains(&format!(" request={id} kind=\"tool call\"")),
            "{line}"
        );
        assert!(line.contains(" chat=c7 message_id=12 "), "{line}");
    }

    #[test]
    fn a_closed_span_logs_how_long_it_took_with_its_fields() {
        let stderr = Buffer::default();
        let dir = tempfile::tempdir().unwrap();
        let file = RotatingFile::new(dir.path(), MAX_FILE_BYTES, FILES);

        tracing::subscriber::with_default(subscriber(&stderr, Some(file)), || {
            let message = message_span(&job("/w/app"));
            std::thread::sleep(std::time::Duration::from_millis(20));
            drop(message);
        });

        let line = stderr.text();
        assert!(line.contains(" message took "), "{line}");
        assert!(line.contains(" ms chat=c7 message_id=12 "), "{line}");
        let json: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(dir.path().join(crate::log_file::JSON_LOG))
                .unwrap()
                .trim(),
        )
        .unwrap();
        let took: u64 = json["duration_ms"].as_str().unwrap().parse().unwrap();
        assert!(took >= 20, "{took}");
        assert_eq!(json["chat"], "c7");
    }

    #[test]
    fn the_journald_line_keeps_its_old_form() {
        let stderr = Buffer::default();

        tracing::subscriber::with_default(subscriber(&stderr, None), || {
            crate::run::log("watching /w/Screenshots\nfake 123 line");
        });

        let text = stderr.text();
        let (unix, rest) = text.split_once(' ').unwrap();
        assert!(unix.parse::<u32>().is_ok(), "{text}");
        assert_eq!(rest, "watching /w/Screenshots\\nfake 123 line\n");
    }

    #[test]
    fn a_field_with_a_space_or_a_quote_is_quoted() {
        let mut fields = Fields::default();
        fields.set("folder", "/w/my app".into());
        fields.set("command", "echo \"hi\"".into());
        fields.set("agent", String::new());

        assert_eq!(
            human_line(5, "x", &fields),
            r#"5 x folder="/w/my app" command="echo \"hi\"" agent="""#
        );
    }

    #[test]
    fn a_json_line_has_the_time_the_level_and_the_fields() {
        let mut fields = Fields::default();
        fields.set("chat", "c1".into());

        let line = json_line(1_790_000_000_123, "info", "a\nb", &fields);

        assert_eq!(
            line,
            r#"{"time":"2026-09-21T14:13:20.123Z","unix":1790000000,"level":"info","line":"a\nb","chat":"c1"}"#
        );
    }

    #[test]
    fn rust_log_sets_the_level() {
        let (filter, problem) = level_filter(Some("warn"));

        assert!(problem.is_none());
        assert!(!filter.would_enable("bridge::run", &tracing::Level::INFO));
        assert!(filter.would_enable("bridge::run", &tracing::Level::WARN));
    }

    #[test]
    fn a_bad_rust_log_gives_info_and_says_why() {
        let (filter, problem) = level_filter(Some("bridge=loud"));

        assert!(problem.unwrap().contains("RUST_LOG"));
        assert!(filter.would_enable("bridge::run", &tracing::Level::INFO));
        assert!(!filter.would_enable("bridge::run", &tracing::Level::DEBUG));
    }

    #[test]
    fn with_no_subscriber_the_log_is_off() {
        assert!(!is_on());
    }
}
