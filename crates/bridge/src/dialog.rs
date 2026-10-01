//! The dialogs and notices of the OS for desktop requests (SPEC.md 6.6.3), with the
//! tools that the user already has. The text goes in an argument or an environment
//! variable, never into a script, so it cannot run as code.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use crate::program::find_program;
use crate::wsl;

/// A dialog gives up by itself after this, in case the bridge stops first.
const BACKSTOP_SECONDS: u64 = 60 * 60;
const TITLE: &str = "Gnomish Relay";
const SUMMARY: &str = "Gnomish Relay needs your approval";
/// zenity exits with this code when its window closes with no button press.
const ZENITY_CLOSED: &str = "3";
/// After a notice closes, its `NotificationClosed` signal comes within this time.
const REASON_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// A notice with buttons. It needs a server with the `actions` capability.
    NotifySend,
    Zenity,
    Osascript,
    /// A message box of PowerShell.
    MessageBox,
    /// The same message box, from WSL through interop (SPEC.md 11.5).
    MessageBoxFromWsl,
}

/// A program, its arguments, and its extra environment variables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub tool: Tool,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// A program that prints the `NotificationClosed` signals of the notice server.
    pub close_watch: Option<CloseWatch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseWatch {
    pub program: String,
    pub args: Vec<String>,
}

/// A button of a dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Approve,
    Deny,
}

/// How a dialog ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    Pressed(Button),
    /// Closed, expired, or replaced. The user gave no answer.
    NoButton,
}

/// The reason of a `NotificationClosed` signal (Desktop Notifications spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Expired,
    Dismissed,
    ClosedByCall,
    Undefined,
}

impl CloseReason {
    fn from_code(code: u32) -> CloseReason {
        match code {
            1 => CloseReason::Expired,
            2 => CloseReason::Dismissed,
            3 => CloseReason::ClosedByCall,
            _ => CloseReason::Undefined,
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            CloseReason::Expired => "it expired",
            CloseReason::Dismissed => "dismissed by the user",
            CloseReason::ClosedByCall => "closed by a call",
            CloseReason::Undefined => "undefined reason",
        }
    }
}

/// The id that `notify-send --print-id` prints on its first line.
pub fn notice_id(stdout: &str) -> Option<u32> {
    stdout.lines().next()?.trim().parse().ok()
}

/// The reason in a line of `gdbus monitor`, when the line closes the notice `id`. For
/// example `...NotificationClosed (uint32 17, uint32 2)`.
pub fn closed_reason(line: &str, id: u32) -> Option<CloseReason> {
    let (_, args) = line.split_once(".NotificationClosed (")?;
    let (closed, reason) = args.trim_end().strip_suffix(')')?.split_once(", ")?;
    let closed: u32 = closed.strip_prefix("uint32 ")?.parse().ok()?;
    let reason: u32 = reason.strip_prefix("uint32 ")?.parse().ok()?;
    (closed == id).then(|| CloseReason::from_code(reason))
}

/// After a dialog closes with no answer, one more dialog asks. zenity comes first,
/// because its window cannot expire and no notice replaces it.
pub fn next_tool(first: Tool, zenity: bool, display: bool) -> Option<Tool> {
    match first {
        Tool::NotifySend if zenity && display => Some(Tool::Zenity),
        Tool::NotifySend => Some(Tool::NotifySend),
        _ => None,
    }
}

/// The second dialog of `next_tool` on this computer.
pub fn find_next_tool(first: Tool) -> Option<Tool> {
    next_tool(first, has_program("zenity"), has_display())
}

/// A notice server shows the body of a notice as markup, so `<b>` or an S15 escape
/// such as `<U+202E>` would hide text.
pub fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Deny is the default button of each dialog, so Enter never approves.
pub fn dialog(tool: Tool, text: &str) -> Dialog {
    let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    let backstop = BACKSTOP_SECONDS.to_string();
    let (program, mut args, env) = match tool {
        Tool::NotifySend => (
            "notify-send",
            words(&[
                "-a",
                TITLE,
                "-u",
                "critical",
                "--print-id",
                "-A",
                "approve=Approve",
                "-A",
                "deny=Deny",
                SUMMARY,
            ]),
            Vec::new(),
        ),
        Tool::Zenity => (
            "zenity",
            words(&[
                "--question",
                "--no-markup",
                "--width",
                "600",
                "--title",
                TITLE,
                "--ok-label",
                "Approve",
                "--cancel-label",
                "Deny",
                "--default-cancel",
                "--timeout",
                &backstop,
                "--text",
            ]),
            vec![("ZENITY_ESC".to_owned(), ZENITY_CLOSED.to_owned())],
        ),
        Tool::Osascript => (
            "osascript",
            words(&[
                "-e",
                "on run argv",
                "-e",
                "activate",
                "-e",
                MAC_DIALOG,
                "-e",
                "end run",
            ]),
            Vec::new(),
        ),
        Tool::MessageBox => (
            "powershell",
            words(&["-NoProfile", "-NonInteractive", "-Command", WINDOWS_BOX]),
            vec![(NOTICE_VAR.to_owned(), box_text(text))],
        ),
        Tool::MessageBoxFromWsl => (
            WSL_POWERSHELL,
            words(&["-NoProfile", "-NonInteractive", "-Command", WINDOWS_BOX]),
            from_wsl(box_text(text)),
        ),
    };
    match tool {
        Tool::NotifySend => args.push(escape_markup(text)),
        Tool::Zenity => args.push(text.to_owned()),
        Tool::Osascript => args.extend([text.to_owned(), backstop]),
        Tool::MessageBox | Tool::MessageBoxFromWsl => {}
    }
    let program = match tool {
        Tool::MessageBoxFromWsl => wsl_powershell().unwrap_or_else(|| program.to_owned()),
        _ => program.to_owned(),
    };
    let close_watch = (tool == Tool::NotifySend).then(gdbus_monitor);
    Dialog {
        tool,
        program,
        args,
        env,
        close_watch,
    }
}

fn gdbus_monitor() -> CloseWatch {
    let args = [
        "monitor",
        "--session",
        "--dest",
        "org.freedesktop.Notifications",
    ];
    CloseWatch {
        program: "gdbus".to_owned(),
        args: args.map(str::to_owned).into(),
    }
}

const NOTICE_VAR: &str = "GNOMISH_NOTICE";
/// The name on the `PATH` of WSL. The bridge looks for the full path first, because
/// its own `PATH` holds no Windows folder.
const WSL_POWERSHELL: &str = "powershell.exe";

fn box_text(text: &str) -> String {
    format!("Yes = Approve, No = Deny.\n\n{text}")
}

/// WSL passes a variable to a Windows program only when `WSLENV` names it.
pub fn wslenv_with_notice(existing: Option<&str>) -> String {
    match existing.filter(|e| !e.is_empty()) {
        Some(existing) => format!("{NOTICE_VAR}:{existing}"),
        None => NOTICE_VAR.to_owned(),
    }
}

fn from_wsl(notice: String) -> Vec<(String, String)> {
    let existing = std::env::var("WSLENV").ok();
    vec![
        (NOTICE_VAR.to_owned(), notice),
        ("WSLENV".to_owned(), wslenv_with_notice(existing.as_deref())),
    ]
}

fn wsl_powershell() -> Option<String> {
    let path = wsl::windows_program(Path::new(wsl::MOUNT_ROOT), WSL_POWERSHELL)?;
    Some(path.to_string_lossy().into_owned())
}

const MAC_DIALOG: &str = "display dialog (item 1 of argv) with title \"Gnomish Relay\" \
buttons {\"Deny\", \"Approve\"} default button \"Deny\" cancel button \"Deny\" with icon caution \
giving up after (item 2 of argv as integer)";

/// `DefaultDesktopOnly` puts the box on top with no window of its own.
const WINDOWS_BOX: &str = "Add-Type -AssemblyName System.Windows.Forms; \
[System.Windows.Forms.MessageBox]::Show($env:GNOMISH_NOTICE, 'Gnomish Relay', 'YesNo', 'Warning', 'Button2', 'DefaultDesktopOnly')";

/// The button that ended the dialog. A closed, dismissed, or timed-out dialog has none.
/// `code` is the exit code. Deny is the cancel button of osascript, so its error is Deny.
pub fn pressed(tool: Tool, code: Option<i32>, stdout: &str) -> Option<Button> {
    match tool {
        Tool::NotifySend => match stdout.lines().last().map(str::trim) {
            Some("approve") => Some(Button::Approve),
            Some("deny") => Some(Button::Deny),
            _ => None,
        },
        Tool::Zenity => match code {
            Some(0) => Some(Button::Approve),
            Some(1) => Some(Button::Deny),
            _ => None,
        },
        Tool::Osascript if stdout.contains("gave up:true") => None,
        Tool::Osascript if code == Some(0) && stdout.contains("button returned:Approve") => {
            Some(Button::Approve)
        }
        Tool::Osascript => Some(Button::Deny),
        Tool::MessageBox | Tool::MessageBoxFromWsl => match stdout.trim() {
            "Yes" => Some(Button::Approve),
            "No" => Some(Button::Deny),
            _ => None,
        },
    }
}

/// A notice with no buttons on a server with no `actions` shows no Approve, and can
/// never answer. So notify-send comes first only with `actions`.
pub fn linux_tool(notify_actions: bool, zenity: bool, display: bool) -> Option<Tool> {
    if notify_actions {
        return Some(Tool::NotifySend);
    }
    (zenity && display).then_some(Tool::Zenity)
}

fn has_program(name: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_program(name, &path, cfg!(windows)).is_some()
}

/// Asked for each dialog, because the notice server can change.
fn notify_actions() -> bool {
    if !has_program("notify-send") || !has_program("gdbus") {
        return false;
    }
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--timeout",
            "5",
            "--dest",
            "org.freedesktop.Notifications",
            "--object-path",
            "/org/freedesktop/Notifications",
            "--method",
            "org.freedesktop.Notifications.GetCapabilities",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    output.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("'actions'"))
}

fn has_display() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

/// Under WSL, the box of Windows comes first, because `WSLg` is often missing. With no
/// interop, the tools of Linux apply.
pub fn wsl_tool(interop: bool, powershell: bool) -> Option<Tool> {
    (interop && powershell).then_some(Tool::MessageBoxFromWsl)
}

/// The dialog tool of this computer, if it has one.
pub fn find_tool() -> Option<Tool> {
    if let Some(tool) = wsl_tool(wsl::interop(), wsl_powershell().is_some()) {
        return Some(tool);
    }
    match std::env::consts::OS {
        "macos" => has_program("osascript").then_some(Tool::Osascript),
        "windows" => has_program("powershell").then_some(Tool::MessageBox),
        _ => linux_tool(notify_actions(), has_program("zenity"), has_display()),
    }
}

fn command(program: &str, args: &[String], env: &[(String, String)]) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the bridge runs with no console, so no window flashes.
        command.creation_flags(0x0800_0000);
    }
    command
}

/// The lines of a running `CloseWatch`. The program stops when this drops.
struct Watching {
    child: Child,
    lines: Receiver<String>,
}

impl Watching {
    fn start(watch: &CloseWatch) -> Option<Watching> {
        let mut child = command(&watch.program, &watch.args, &[])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        Some(Watching { child, lines })
    }

    fn reason(&self, id: u32) -> Option<CloseReason> {
        let deadline = Instant::now() + REASON_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).ok()?;
            if let Some(reason) = closed_reason(&line, id) {
                return Some(reason);
            }
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A dialog that runs, and that stops when its request no longer waits.
pub struct Shown {
    tool: Tool,
    child: Child,
    stdout: String,
    watching: Option<Watching>,
}

impl Shown {
    /// The close watch starts first, so it sees the close of the notice.
    pub fn start(dialog: &Dialog) -> Option<Shown> {
        let watching = dialog.close_watch.as_ref().and_then(Watching::start);
        let child = command(&dialog.program, &dialog.args, &dialog.env)
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        Some(Shown {
            tool: dialog.tool,
            child,
            stdout: String::new(),
            watching,
        })
    }

    /// `Some` once the dialog ended.
    pub fn ended(&mut self) -> Option<Ended> {
        let code = match self.child.try_wait() {
            Ok(None) => return None,
            Ok(Some(status)) => status.code(),
            Err(_) => return Some(Ended::NoButton),
        };
        if let Some(out) = self.child.stdout.as_mut() {
            let _ = out.read_to_string(&mut self.stdout);
        }
        let button = pressed(self.tool, code, &self.stdout);
        Some(button.map_or(Ended::NoButton, Ended::Pressed))
    }

    /// Why the notice server closed the notice, after `ended` gave `NoButton`.
    pub fn close_reason(&self) -> Option<CloseReason> {
        let id = notice_id(&self.stdout)?;
        self.watching.as_ref()?.reason(id)
    }

    /// notify-send closes its notice on SIGTERM, but not on SIGKILL. So first TERM
    /// through `kill`, which needs no unsafe code, then a kill.
    pub fn stop(mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id().to_string();
            let _ = command("kill", &["-TERM".to_owned(), pid], &[]).status();
            for _ in 0..10 {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A program, its arguments, and its extra environment variables.
pub type NoticeCommand = (String, Vec<String>, Vec<(String, String)>);

/// The program and the arguments that show `text` as a plain notice of the OS, for a
/// computer with no dialog tool.
pub fn notice_command(os: &str, text: &str) -> Option<NoticeCommand> {
    let title = TITLE.to_owned();
    match os {
        "linux" => Some(("notify-send".into(), vec![title, text.into()], Vec::new())),
        "macos" => Some((
            "osascript".into(),
            [
                "-e",
                "on run argv",
                "-e",
                "display notification (item 1 of argv) with title \"Gnomish Relay\"",
                "-e",
                "end run",
                text,
            ]
            .map(str::to_owned)
            .into(),
            Vec::new(),
        )),
        "windows" => Some((
            "powershell".into(),
            toast_args(),
            vec![(NOTICE_VAR.into(), text.into())],
        )),
        "wsl" => Some((
            wsl_powershell().unwrap_or_else(|| WSL_POWERSHELL.into()),
            toast_args(),
            from_wsl(text.into()),
        )),
        _ => None,
    }
}

fn toast_args() -> Vec<String> {
    ["-NoProfile", "-NonInteractive", "-Command", WINDOWS_TOAST]
        .map(str::to_owned)
        .into()
}

/// A toast through the app id of PowerShell, which every Windows 10 and 11 has.
const WINDOWS_TOAST: &str = "\
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
$t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
$x = $t.GetElementsByTagName('text'); \
$x[0].AppendChild($t.CreateTextNode('Gnomish Relay')) > $null; \
$x[1].AppendChild($t.CreateTextNode($env:GNOMISH_NOTICE)) > $null; \
$app = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe'; \
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($app).Show([Windows.UI.Notifications.ToastNotification]::new($t))";

/// Best effort: with no tool for notices, the log line is the notice.
pub fn show_notice(text: &str) {
    let os = if wsl::interop() {
        "wsl"
    } else {
        std::env::consts::OS
    };
    let Some((program, args, env)) = notice_command(os, text) else {
        return;
    };
    if !Path::new(&program).is_file() && !has_program(&program) {
        return;
    }
    let mut notice = command(&program, &args, &env);
    notice.stdout(Stdio::null());
    if let Ok(mut child) = notice.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTILE: &str = "\"; rm -rf ~; \" <b>x</b> & <U+202E>";

    #[test]
    fn the_text_of_each_dialog_stays_out_of_the_script() {
        let windows = dialog(Tool::MessageBox, HOSTILE);
        assert!(!windows.args.iter().any(|a| a.contains("rm -rf")));
        assert_eq!(
            windows.env,
            [(
                "GNOMISH_NOTICE".to_owned(),
                format!("Yes = Approve, No = Deny.\n\n{HOSTILE}")
            )]
        );
        let mac = dialog(Tool::Osascript, HOSTILE);
        let (script, argv) = mac.args.split_at(mac.args.len() - 2);
        assert!(!script.iter().any(|a| a.contains("rm -rf")));
        assert_eq!(argv[0], HOSTILE);
        let zenity = dialog(Tool::Zenity, HOSTILE);
        assert!(zenity.args.contains(&"--no-markup".to_owned()));
        assert_eq!(zenity.args.last().map(String::as_str), Some(HOSTILE));
    }

    #[test]
    fn a_notice_with_buttons_shows_markup_as_plain_text() {
        let shown = dialog(Tool::NotifySend, HOSTILE);
        assert_eq!(shown.program, "notify-send");
        assert_eq!(
            shown.args.last().map(String::as_str),
            Some("\"; rm -rf ~; \" &lt;b&gt;x&lt;/b&gt; &amp; &lt;U+202E&gt;")
        );
        assert!(shown.args.contains(&"approve=Approve".to_owned()));
    }

    #[test]
    fn deny_is_the_default_button_of_each_dialog() {
        let zenity = dialog(Tool::Zenity, "x");
        assert!(zenity.args.contains(&"--default-cancel".to_owned()));
        assert!(MAC_DIALOG.contains("default button \"Deny\""));
        assert!(WINDOWS_BOX.contains("'Button2'"));
    }

    #[test]
    fn only_a_press_on_a_button_answers_and_a_closed_dialog_has_no_button() {
        use Button::{Approve, Deny};
        use Tool::{MessageBox, NotifySend, Osascript, Zenity};
        let cases = [
            (NotifySend, Some(0), "42\napprove\n", Some(Approve)),
            (NotifySend, Some(0), "42\ndeny\n", Some(Deny)),
            (NotifySend, Some(0), "42\n", None),
            (NotifySend, Some(0), "", None),
            (Zenity, Some(0), "", Some(Approve)),
            (Zenity, Some(1), "", Some(Deny)),
            (Zenity, Some(3), "", None),
            (Zenity, Some(5), "", None),
            (Zenity, None, "", None),
            (
                Osascript,
                Some(0),
                "button returned:Approve, gave up:false\n",
                Some(Approve),
            ),
            (Osascript, Some(0), "button returned:, gave up:true\n", None),
            (Osascript, Some(1), "", Some(Deny)),
            (MessageBox, Some(0), "Yes\r\n", Some(Approve)),
            (MessageBox, Some(0), "No\r\n", Some(Deny)),
            (MessageBox, Some(0), "garbage", None),
        ];
        for (tool, code, stdout, button) in cases {
            assert_eq!(
                pressed(tool, code, stdout),
                button,
                "{tool:?} {code:?} {stdout:?}"
            );
        }
    }

    #[test]
    fn a_closed_zenity_window_exits_with_its_own_code() {
        let zenity = dialog(Tool::Zenity, "x");

        assert_eq!(zenity.env, [("ZENITY_ESC".to_owned(), "3".to_owned())]);
        assert_eq!(pressed(Tool::Zenity, Some(3), ""), None);
    }

    #[test]
    fn a_notice_prints_its_id_and_its_close_watch_listens_to_the_notice_server() {
        let shown = dialog(Tool::NotifySend, "x");

        assert!(shown.args.contains(&"--print-id".to_owned()));
        let watch = shown.close_watch.unwrap();
        assert_eq!(watch.program, "gdbus");
        assert_eq!(
            watch.args,
            [
                "monitor",
                "--session",
                "--dest",
                "org.freedesktop.Notifications"
            ]
        );
        assert_eq!(dialog(Tool::Zenity, "x").close_watch, None);
    }

    #[test]
    fn the_close_signal_of_a_notice_gives_its_reason() {
        let line = |id: u32, reason: u32| {
            format!(
                "/org/freedesktop/Notifications: org.freedesktop.Notifications.NotificationClosed \
                 (uint32 {id}, uint32 {reason})"
            )
        };

        assert_eq!(closed_reason(&line(7, 1), 7), Some(CloseReason::Expired));
        assert_eq!(closed_reason(&line(7, 2), 7), Some(CloseReason::Dismissed));
        assert_eq!(
            closed_reason(&line(7, 3), 7),
            Some(CloseReason::ClosedByCall)
        );
        assert_eq!(closed_reason(&line(7, 4), 7), Some(CloseReason::Undefined));
        assert_eq!(closed_reason(&line(8, 2), 7), None, "another notice");
        assert_eq!(
            closed_reason(
                "/org/freedesktop/Notifications: org.freedesktop.Notifications.ActionInvoked \
                 (uint32 7, 'approve')",
                7
            ),
            None
        );
        assert_eq!(notice_id("42\napprove\n"), Some(42));
        assert_eq!(notice_id("approve\n"), None);
    }

    #[test]
    fn after_a_closed_notice_zenity_asks_next_else_the_notice_once_more() {
        assert_eq!(next_tool(Tool::NotifySend, true, true), Some(Tool::Zenity));
        assert_eq!(
            next_tool(Tool::NotifySend, false, true),
            Some(Tool::NotifySend)
        );
        assert_eq!(
            next_tool(Tool::NotifySend, true, false),
            Some(Tool::NotifySend)
        );
        assert_eq!(next_tool(Tool::Zenity, true, true), None);
        assert_eq!(next_tool(Tool::Osascript, true, true), None);
        assert_eq!(next_tool(Tool::MessageBox, true, true), None);
    }

    #[test]
    fn linux_uses_a_notice_with_buttons_only_when_the_server_shows_buttons() {
        assert_eq!(linux_tool(true, true, true), Some(Tool::NotifySend));
        assert_eq!(linux_tool(false, true, true), Some(Tool::Zenity));
        assert_eq!(linux_tool(false, true, false), None);
        assert_eq!(linux_tool(false, false, true), None);
    }

    #[test]
    fn under_wsl_the_windows_box_gets_its_text_through_wslenv() {
        let shown = dialog(Tool::MessageBoxFromWsl, HOSTILE);

        assert!(!shown.args.iter().any(|a| a.contains("rm -rf")));
        assert!(
            shown.program.ends_with("powershell.exe"),
            "{}",
            shown.program
        );
        assert_eq!(shown.env[0].0, "GNOMISH_NOTICE");
        assert!(shown.env[0].1.ends_with(HOSTILE));
        assert_eq!(shown.env[1].0, "WSLENV");
        assert!(shown.env[1].1.starts_with("GNOMISH_NOTICE"));
        assert_eq!(
            pressed(Tool::MessageBoxFromWsl, Some(0), "Yes\r\n"),
            Some(Button::Approve)
        );
        assert_eq!(
            pressed(Tool::MessageBoxFromWsl, Some(0), "No\r\n"),
            Some(Button::Deny)
        );
    }

    #[test]
    fn wslenv_keeps_the_names_that_it_had() {
        assert_eq!(wslenv_with_notice(None), "GNOMISH_NOTICE");
        assert_eq!(wslenv_with_notice(Some("")), "GNOMISH_NOTICE");
        assert_eq!(
            wslenv_with_notice(Some("USERPROFILE/p")),
            "GNOMISH_NOTICE:USERPROFILE/p"
        );
    }

    #[test]
    fn under_wsl_the_windows_box_needs_interop_and_powershell() {
        assert_eq!(wsl_tool(true, true), Some(Tool::MessageBoxFromWsl));
        assert_eq!(wsl_tool(true, false), None);
        assert_eq!(wsl_tool(false, true), None);
    }

    #[test]
    fn the_text_of_a_notice_stays_an_argument() {
        let text = "\"; rm -rf ~; \"";
        let (program, args, _) = notice_command("linux", text).unwrap();
        assert_eq!((program.as_str(), args[1].as_str()), ("notify-send", text));
        let (_, args, _) = notice_command("macos", text).unwrap();
        assert_eq!(args.last().map(String::as_str), Some(text));
        assert!(!args[..args.len() - 1].iter().any(|a| a.contains("rm -rf")));
        let (_, args, env) = notice_command("windows", text).unwrap();
        assert!(!args.iter().any(|a| a.contains("rm -rf")));
        assert_eq!(env, [("GNOMISH_NOTICE".to_owned(), text.to_owned())]);
        let (_, args, env) = notice_command("wsl", text).unwrap();
        assert!(!args.iter().any(|a| a.contains("rm -rf")));
        assert_eq!(env[0], ("GNOMISH_NOTICE".to_owned(), text.to_owned()));
        assert_eq!(env[1].0, "WSLENV");
        assert!(notice_command("haiku", text).is_none());
    }
}
