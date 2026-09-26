//! The dialogs and notices of the OS for desktop requests (SPEC.md 6.6.3), with the
//! tools that the user already has. The text goes in an argument or an environment
//! variable, never into a script, so it cannot run as code.

use std::io::Read;
use std::process::{Child, Command, Stdio};

use crate::program::find_program;

/// A dialog gives up by itself after this, in case the bridge stops first.
const BACKSTOP_SECONDS: u64 = 60 * 60;
const TITLE: &str = "Gnomish Relay";
const SUMMARY: &str = "Gnomish Relay: approve?";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// A notice with buttons. It needs a server with the `actions` capability.
    NotifySend,
    Zenity,
    Osascript,
    /// A message box of PowerShell.
    MessageBox,
}

/// A program, its arguments, and its extra environment variables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub tool: Tool,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
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
            Vec::new(),
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
            vec![(
                "GNOMISH_NOTICE".to_owned(),
                format!("Yes = Approve, No = Deny.\n\n{text}"),
            )],
        ),
    };
    match tool {
        Tool::NotifySend => args.push(escape_markup(text)),
        Tool::Zenity => args.push(text.to_owned()),
        Tool::Osascript => args.extend([text.to_owned(), backstop]),
        Tool::MessageBox => {}
    }
    Dialog {
        tool,
        program: program.to_owned(),
        args,
        env,
    }
}

const MAC_DIALOG: &str = "display dialog (item 1 of argv) with title \"Gnomish Relay\" \
buttons {\"Deny\", \"Approve\"} default button \"Deny\" cancel button \"Deny\" with icon caution \
giving up after (item 2 of argv as integer)";

/// `DefaultDesktopOnly` puts the box on top with no window of its own.
const WINDOWS_BOX: &str = "Add-Type -AssemblyName System.Windows.Forms; \
[System.Windows.Forms.MessageBox]::Show($env:GNOMISH_NOTICE, 'Gnomish Relay', 'YesNo', 'Warning', 'Button2', 'DefaultDesktopOnly')";

/// Only a click on Approve approves. A closed, dismissed, or timed-out dialog denies.
pub fn approved(tool: Tool, success: bool, stdout: &str) -> bool {
    match tool {
        Tool::NotifySend => stdout.trim() == "approve",
        Tool::Zenity => success,
        Tool::Osascript => {
            success
                && stdout.contains("button returned:Approve")
                && !stdout.contains("gave up:true")
        }
        Tool::MessageBox => stdout.trim() == "Yes",
    }
}

/// A notice with no buttons on a server with no `actions` shows no Approve, and then
/// closes as a Deny. So notify-send comes first only with `actions`.
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

/// The dialog tool of this computer, if it has one.
pub fn find_tool() -> Option<Tool> {
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

/// A dialog that runs, and that stops when its request no longer waits.
pub struct Shown {
    tool: Tool,
    child: Child,
}

impl Shown {
    pub fn start(dialog: &Dialog) -> Option<Shown> {
        let child = command(&dialog.program, &dialog.args, &dialog.env)
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        Some(Shown {
            tool: dialog.tool,
            child,
        })
    }

    /// `Some` once the user answered or closed the dialog: true for Approve.
    pub fn answer(&mut self) -> Option<bool> {
        let status = match self.child.try_wait() {
            Ok(None) => return None,
            Ok(Some(status)) => status,
            Err(_) => return Some(false),
        };
        let mut stdout = String::new();
        if let Some(out) = self.child.stdout.as_mut() {
            let _ = out.read_to_string(&mut stdout);
        }
        Some(approved(self.tool, status.success(), &stdout))
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
            ["-NoProfile", "-NonInteractive", "-Command", WINDOWS_TOAST]
                .map(str::to_owned)
                .into(),
            vec![("GNOMISH_NOTICE".into(), text.into())],
        )),
        _ => None,
    }
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
    let Some((program, args, env)) = notice_command(std::env::consts::OS, text) else {
        return;
    };
    if !has_program(&program) {
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
    fn only_a_click_on_approve_approves() {
        use Tool::{MessageBox, NotifySend, Osascript, Zenity};
        let cases = [
            (NotifySend, true, "approve\n", true),
            (NotifySend, true, "deny\n", false),
            (NotifySend, true, "", false),
            (Zenity, true, "", true),
            (Zenity, false, "", false),
            (
                Osascript,
                true,
                "button returned:Approve, gave up:false\n",
                true,
            ),
            (Osascript, true, "button returned:, gave up:true\n", false),
            (
                Osascript,
                true,
                "button returned:Deny, gave up:false\n",
                false,
            ),
            (Osascript, false, "", false),
            (MessageBox, true, "Yes\r\n", true),
            (MessageBox, true, "No\r\n", false),
            (MessageBox, true, "garbage", false),
        ];
        for (tool, success, stdout, approve) in cases {
            assert_eq!(
                approved(tool, success, stdout),
                approve,
                "{tool:?} {stdout:?}"
            );
        }
    }

    #[test]
    fn linux_uses_a_notice_with_buttons_only_when_the_server_shows_buttons() {
        assert_eq!(linux_tool(true, true, true), Some(Tool::NotifySend));
        assert_eq!(linux_tool(false, true, true), Some(Tool::Zenity));
        assert_eq!(linux_tool(false, true, false), None);
        assert_eq!(linux_tool(false, false, true), None);
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
        assert!(notice_command("haiku", text).is_none());
    }
}
