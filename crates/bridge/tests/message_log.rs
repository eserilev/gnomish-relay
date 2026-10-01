//! The log lines of one message, through a real subscriber.
//!
//! This test has a file and a process of its own. `tracing` caches the interest of each
//! callsite for the whole process. A parallel test that hits the `message` span with no
//! subscriber can cache "never" for it while this subscriber starts. The span then
//! drops out, and the lines lose their fields.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bridge::agent::Echo;
use bridge::config::{Permission, Policy};
use bridge::folder_path::{path_bytes, real_path};
use bridge::ids::hex;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::run::{Bridge, Paths, now};
use common::{install_window, screenshot_png, signed_frame, strip_rows};
use protocol::apps::App;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

/// stderr of the log, shared with the layer.
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

fn echo_bridge(root: &std::path::Path) -> Bridge {
    let paths = Paths {
        addons: root.join("Interface/AddOns"),
        screenshots: root.join("Screenshots"),
        accounts: root.join("WTF/Account"),
        state: root.join("data"),
        config: root.join("data/config"),
    };
    for folder in [
        &paths.addons,
        &paths.screenshots,
        &paths.accounts,
        &paths.state,
    ] {
        fs::create_dir_all(folder).unwrap();
    }
    install_window(&paths.addons, App::Relay);
    let base = path_bytes(&std::env::temp_dir().canonicalize().unwrap());
    let policy = Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    };
    let keys = KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap();
    let agents = [("claude".to_owned(), Arc::new(Echo) as _)].into();
    Bridge::new(paths, policy, keys, agents).unwrap()
}

fn strip_png(text: &str) -> Vec<u8> {
    let payload = format!("tok\x1fc1\x1f7\x1f\x1f\x1f\x1f{text}");
    let frame = signed_frame(now(), payload.as_bytes(), KEY);
    screenshot_png(&strip_rows(&frame))
}

/// On Windows, the first line has the folder of the request, `C:/x`. The later lines
/// have the real folder, `C:\x`, which the log writes as `C:\\x`.
fn slashed_folder(line: &str) -> String {
    let value = line.split(" folder=").nth(1).unwrap();
    let value = value.split(' ').next().unwrap();
    value.replace(r"\\", "/")
}

#[test]
fn the_log_lines_of_a_message_carry_its_chat_id_agent_and_folder() {
    let root = tempfile::tempdir().unwrap();
    let mut bridge = echo_bridge(root.path());
    let stderr = LogBuffer::default();
    let layer = bridge::logging::LogLayer::new(Box::new(stderr.clone()), None);
    let filter = bridge::logging::level_filter(None).0;
    let subscriber = tracing_subscriber::Registry::default().with(layer.with_filter(filter));
    fs::write(
        root.path().join("Screenshots/WoWScrnShot_1.png"),
        strip_png("a private prompt"),
    )
    .unwrap();

    let written = tracing::subscriber::with_default(subscriber, || {
        common::step_until_within(&mut bridge, Duration::from_secs(30), || {
            stderr.text().contains("reply c1 #7 written")
        })
    });

    let text = stderr.text();
    assert!(written, "{text}");
    let real = real_path(&std::env::temp_dir()).unwrap();
    let folder = real.display().to_string().replace('\\', "/");
    let fields = " chat=c1 message_id=7 agent=claude permission=auto-edit folder=";
    for start in ["run c1 #7 ", "done c1 #7", "reply c1 #7 written"] {
        let line = text.lines().find(|l| l.contains(start)).unwrap();
        assert!(line.contains(fields), "{line}");
        assert_eq!(slashed_folder(line), folder, "{line}");
    }
    let done = text.lines().find(|l| l.contains("done c1 #7")).unwrap();
    assert!(done.ends_with(" result=reply"), "{done}");
    assert!(!text.contains("a private prompt"), "{text}");
}
