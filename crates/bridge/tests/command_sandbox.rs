//! The sandbox of the commands of a game run, with the real tool of the OS (SPEC.md 6.6.4
//! and 14.5): `bwrap` on Linux and `sandbox-exec` on macOS. Each test runs a command
//! through `gnomish-relay --sandbox-run`, as Claude Code does. With no working tool the
//! tests skip, unless `GNOMISH_REQUIRE_BWRAP` or `GNOMISH_REQUIRE_SANDBOX_EXEC` is set,
//! as in CI.
#![cfg(any(target_os = "linux", target_os = "macos"))]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::{self, BufRead, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bridge::allow_hosts::{Defaults, HostList};
use bridge::command_sandbox::{self, CommandSandbox, Guarded, RunWalls, WALLS_VAR};
use bridge::proxy::{Limits, Net, ProxySettings};
use bridge::story_sandbox::{self, Sandbox};

const REQUIRE: &str = if cfg!(target_os = "macos") {
    "GNOMISH_REQUIRE_SANDBOX_EXEC"
} else {
    "GNOMISH_REQUIRE_BWRAP"
};

/// `None` skips the test on a computer with no working sandbox.
fn tool() -> Option<Sandbox> {
    let tool = story_sandbox::detect();
    if tool != Sandbox::None {
        return Some(tool);
    }
    assert!(
        std::env::var_os(REQUIRE).is_none(),
        "the sandbox tool is missing or does not work, and {REQUIRE} is set"
    );
    eprintln!("skipped: the sandbox tool is missing or does not work");
    None
}

/// A home with the keys and the state of the bridge, an ssh key, a chat folder, and a
/// second project. It is not under `/tmp`, because the sandbox hides all of `/tmp`. The
/// name of the home has a quote, a backslash, and a space, so each path of the Seatbelt
/// profile needs its escape (S32).
struct Machine {
    _root: tempfile::TempDir,
    home: PathBuf,
    config: PathBuf,
    data: PathBuf,
    chat: PathBuf,
    other: PathBuf,
}

/// The target folder, unless a part of its path is hidden, as in a worktree under
/// `.claude/`. The home cache folder then takes its place.
fn test_root() -> PathBuf {
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    if !target.components().any(|c| c.as_os_str() == ".claude") {
        return target;
    }
    let cache = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache");
    fs::create_dir_all(&cache).unwrap();
    cache
}

fn machine() -> Machine {
    let root = tempfile::tempdir_in(test_root()).unwrap();
    let home = root.path().canonicalize().unwrap().join("ho\"me \\ x");
    let config = home.join(".config/gnomish-relay");
    let data = home.join(".local/share/gnomish-relay");
    let chat = home.join("Code/app");
    let other = home.join("Code/other");
    for dir in [
        &config,
        &data,
        &chat.join(".git/hooks"),
        &other,
        &home.join(".ssh"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(config.join("strip.key"), "secret strip key").unwrap();
    fs::write(data.join("state.json"), "secret state").unwrap();
    fs::write(home.join(".ssh/id_ed25519"), "secret ssh key").unwrap();
    fs::write(chat.join(".env"), "secret env").unwrap();
    fs::write(chat.join(".git/hooks/pre-commit"), "old hook").unwrap();
    fs::write(chat.join("notes.txt"), "plain notes").unwrap();
    Machine {
        _root: root,
        home,
        config,
        data,
        chat,
        other,
    }
}

fn sandbox(m: &Machine, tool: Sandbox) -> CommandSandbox {
    CommandSandbox::new(
        tool,
        PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
        Some(m.home.clone()),
    )
}

fn prepare(m: &Machine, sandbox: &CommandSandbox) -> RunWalls {
    let guarded = Guarded {
        config_dir: &m.config,
        data_dir: &m.data,
    };
    command_sandbox::prepare(sandbox, &guarded, &m.chat, "chat test").unwrap()
}

fn walls(m: &Machine, tool: Sandbox) -> RunWalls {
    prepare(m, &sandbox(m, tool))
}

/// A public address that stands for the fake server behind the proxy.
const PUBLIC: [u8; 4] = [93, 184, 216, 34];

/// A web server on this computer that answers with the first line of each request.
fn server() -> SocketAddr {
    static SERVER: OnceLock<SocketAddr> = OnceLock::new();
    *SERVER.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = io::BufReader::new(&stream).read_line(&mut line);
                let body = format!("hello {}", line.trim_end());
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        addr
    })
}

fn fake_resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let ip: IpAddr = match host {
        "allowed.test" => PUBLIC.into(),
        "local.test" => [127, 0, 0, 1].into(),
        "private.test" => [192, 168, 1, 1].into(),
        _ => return Err(io::ErrorKind::NotFound.into()),
    };
    Ok(vec![SocketAddr::new(ip, port)])
}

fn fake_connect(addr: &SocketAddr, limit: Duration) -> io::Result<TcpStream> {
    if addr.ip() == IpAddr::from(PUBLIC) {
        return TcpStream::connect_timeout(&server(), limit);
    }
    TcpStream::connect_timeout(addr, limit)
}

/// The walls of a run whose proxy knows `allowed.test` and two names that lead back to
/// this computer or its network.
fn proxied_walls(m: &Machine, tool: Sandbox) -> RunWalls {
    let names = ["allowed.test", "local.test", "private.test"].map(String::from);
    let settings = ProxySettings {
        hosts: Arc::new(HostList::new(Defaults::Off, &names).unwrap()),
        net: Net {
            resolve: fake_resolve,
            connect: fake_connect,
        },
        limits: Limits::default(),
    };
    prepare(m, &sandbox(m, tool).with_proxy(settings))
}

struct Ran {
    ok: bool,
    out: String,
}

/// Runs `command` in `cwd` as Claude Code does with `CLAUDE_CODE_SHELL_PREFIX`.
fn run_in(run: &RunWalls, cwd: &Path, command: &str) -> Ran {
    let file = run
        .claude_vars(Path::new(env!("CARGO_BIN_EXE_gnomish-relay")))
        .into_iter()
        .find(|(name, _)| name == WALLS_VAR)
        .unwrap()
        .1;
    let output = Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .args(["--sandbox-run", command])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env(WALLS_VAR, file)
        .output()
        .unwrap();
    Ran {
        ok: output.status.success(),
        out: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn run(run: &RunWalls, m: &Machine, command: &str) -> Ran {
    run_in(run, &m.chat, command)
}

#[test]
fn a_write_inside_the_chat_folder_and_the_temp_folder_works() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(
        &w,
        &m,
        "echo made > made.txt && echo temp > \"$TMPDIR/t\" && cat notes.txt",
    );

    assert!(ran.ok, "{}", ran.out);
    assert_eq!(
        fs::read_to_string(m.chat.join("made.txt")).unwrap(),
        "made\n"
    );
    assert!(ran.out.contains("plain notes"));
    assert!(w.wrapper_ran());
}

#[test]
fn a_write_outside_the_chat_folder_fails() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    for target in [
        "../other/x",
        "$HOME/x",
        "/tmp/gnomish-relay-escape",
        "/var/tmp/gnomish-relay-escape",
    ] {
        let ran = run(&w, &m, &format!("echo x > \"{target}\""));
        assert!(!ran.ok, "{target}: {}", ran.out);
    }
    assert!(!m.other.join("x").exists());
    assert!(!m.home.join("x").exists());
    assert!(!Path::new("/tmp/gnomish-relay-escape").exists());
}

#[test]
fn a_read_of_a_hidden_path_shows_no_secret() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    for path in [
        m.config.join("strip.key"),
        m.data.join("state.json"),
        m.home.join(".ssh/id_ed25519"),
        m.chat.join(".env"),
    ] {
        let ran = run(
            &w,
            &m,
            &format!(
                "cat '{}'",
                path.display().to_string().replace('\'', "'\\''")
            ),
        );
        assert!(
            !ran.out.contains("secret"),
            "{}: {}",
            path.display(),
            ran.out
        );
    }
}

#[test]
fn a_command_cannot_change_a_git_hook() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(&w, &m, "echo evil > .git/hooks/pre-commit");

    assert!(!ran.ok, "{}", ran.out);
    assert_eq!(
        fs::read_to_string(m.chat.join(".git/hooks/pre-commit")).unwrap(),
        "old hook"
    );
}

#[test]
fn a_child_process_stays_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(&w, &m, "sh -c 'bash -c \"echo x > ../other/child\"'");

    assert!(!ran.ok, "{}", ran.out);
    assert!(!m.other.join("child").exists());
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_chat_folder_does_not_escape() {
    let Some(tool) = tool() else { return };
    let m = machine();
    std::os::unix::fs::symlink(&m.other, m.chat.join("out")).unwrap();
    std::os::unix::fs::symlink(m.home.join(".ssh"), m.chat.join("keys")).unwrap();
    let w = walls(&m, tool);

    let write = run(&w, &m, "echo x > out/x");
    let read = run(&w, &m, "cat keys/id_ed25519");

    assert!(!write.ok, "{}", write.out);
    assert!(!m.other.join("x").exists());
    assert!(!read.out.contains("secret"), "{}", read.out);
}

#[test]
fn a_command_reaches_no_network() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let ran = run(&w, &m, &format!("echo hi > /dev/tcp/127.0.0.1/{port}"));

    assert!(!ran.ok, "{}", ran.out);
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "a connection came in");
}

#[test]
fn quotes_line_breaks_and_substitutions_run_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(
        &w,
        &m,
        "echo \"it's\"\necho $(echo inner) `echo tick`\necho x > ../other/q",
    );

    assert!(ran.out.contains("it's"), "{}", ran.out);
    assert!(ran.out.contains("inner tick"), "{}", ran.out);
    assert!(!m.other.join("q").exists());
}

#[test]
fn a_command_in_a_hidden_folder_does_not_start() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run_in(&w, &m.home.join(".ssh"), "cat id_ed25519");

    assert!(!ran.ok, "{}", ran.out);
    assert!(!ran.out.contains("secret"), "{}", ran.out);
}

#[test]
fn a_command_reaches_an_allowed_host_through_the_proxy() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = proxied_walls(&m, tool);

    let ran = run(
        &w,
        &m,
        "curl -sS --max-time 20 --proxytunnel http://allowed.test/x",
    );

    assert!(ran.ok, "{}", ran.out);
    assert_eq!(ran.out, "hello GET /x HTTP/1.1");
}

#[test]
fn a_host_that_is_not_on_the_list_is_refused() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = proxied_walls(&m, tool);

    let ran = run(&w, &m, "curl -sS --max-time 20 https://example.com/");

    assert!(!ran.ok, "{}", ran.out);
    assert!(ran.out.contains("403"), "{}", ran.out);
}

#[test]
fn an_ip_address_is_refused_by_the_proxy() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = proxied_walls(&m, tool);

    let ran = run(&w, &m, "curl -sS --max-time 20 https://93.184.216.34/");

    assert!(!ran.ok, "{}", ran.out);
    assert!(ran.out.contains("403"), "{}", ran.out);
}

#[test]
fn a_name_that_resolves_to_this_computer_or_its_network_is_refused() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = proxied_walls(&m, tool);

    for host in ["local.test", "private.test"] {
        let ran = run(&w, &m, &format!("curl -sS --max-time 20 https://{host}/"));
        assert!(!ran.ok, "{host}: {}", ran.out);
        assert!(ran.out.contains("403"), "{host}: {}", ran.out);
    }
}

#[test]
fn a_connection_that_skips_the_proxy_fails() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = proxied_walls(&m, tool);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let direct = run(&w, &m, &format!("echo hi > /dev/tcp/127.0.0.1/{port}"));
    let no_proxy = run(
        &w,
        &m,
        &format!("curl -sS --max-time 20 --noproxy '*' http://127.0.0.1:{port}/"),
    );

    assert!(!direct.ok, "{}", direct.out);
    assert!(!no_proxy.ok, "{}", no_proxy.out);
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "a connection came in");
}

/// One small fetch over the internet, through the real proxy and the default hosts. It
/// checks that TLS works inside the sandbox, on macOS also through the Security framework
/// with the keychain services denied. It skips when this computer is offline.
#[test]
fn a_command_fetches_the_index_config_of_crates_io_through_the_real_proxy() {
    let Some(tool) = tool() else { return };
    if !is_online("index.crates.io:443") {
        eprintln!("skipped: index.crates.io is out of reach");
        return;
    }
    let m = machine();
    let hosts = HostList::new(Defaults::Keep, &[]).unwrap();
    let w = prepare(&m, &sandbox(&m, tool).with_proxy(ProxySettings::new(hosts)));

    let fetch = "curl -sS --max-time 60 https://index.crates.io/config.json";
    let ran = run(
        &w,
        &m,
        &format!(
            "{fetch} && if curl -V | grep -q SecureTransport; then CURL_SSL_BACKEND=secure-transport {fetch}; fi"
        ),
    );

    assert!(ran.ok, "{}", ran.out);
    assert!(ran.out.contains("static.crates.io"), "{}", ran.out);
}

/// A keychain of the test, unlocked, with one item that any program reads with no prompt.
/// With the proxy, a command cannot read it; with no proxy, it can.
#[cfg(target_os = "macos")]
#[test]
fn with_the_proxy_a_command_cannot_read_the_keychain() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let keychain = m.home.join("probe.keychain");
    let keychain = keychain.to_str().unwrap();
    let security = |args: &[&str]| {
        let ok = Command::new("/usr/bin/security")
            .args(args)
            .status()
            .unwrap();
        assert!(ok.success(), "security {args:?}");
    };
    security(&["create-keychain", "-p", "pw", keychain]);
    security(&["unlock-keychain", "-p", "pw", keychain]);
    security(&[
        "add-generic-password",
        "-a",
        "probe",
        "-s",
        "gnomish-relay-probe",
        "-w",
        "probe-secret-value",
        "-A",
        keychain,
    ]);
    let read =
        format!("/usr/bin/security find-generic-password -s gnomish-relay-probe -w '{keychain}'");
    let plain = walls(&m, tool.clone());
    let hosts = HostList::new(Defaults::Keep, &[]).unwrap();
    let proxied = prepare(&m, &sandbox(&m, tool).with_proxy(ProxySettings::new(hosts)));

    let open = run(&plain, &m, &read);
    let closed = run(&proxied, &m, &read);

    let _ = Command::new("/usr/bin/security")
        .args(["delete-keychain", keychain])
        .status();
    assert!(open.out.contains("probe-secret-value"), "{}", open.out);
    assert!(!closed.out.contains("probe-secret-value"), "{}", closed.out);
}

fn is_online(host: &str) -> bool {
    let Some(addr) = host.to_socket_addrs().ok().and_then(|mut all| all.next()) else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_secs(5)).is_ok()
}

/// A live run of `cargo fetch` for a small crate, with the real proxy, the default
/// hosts, and the real home folder. It needs the internet and `cargo`. On Linux the
/// crate lands in the copy-on-write view of `~/.cargo`; macOS has none, so there the
/// command puts cargo in the chat folder.
#[test]
#[ignore = "live: fetches a crate from crates.io"]
fn cargo_fetch_of_a_small_crate_works_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let m = machine();
    fs::create_dir_all(m.chat.join("src")).unwrap();
    fs::write(
        m.chat.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nitoa = \"=1.0.2\"\n",
    )
    .unwrap();
    fs::write(m.chat.join("src/main.rs"), "fn main() {}\n").unwrap();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cached = |home: &Path| crate_files(home, "itoa-1.0.2.crate");
    let before = cached(&home);
    let mut sandbox = sandbox(&m, tool.clone()).with_proxy(ProxySettings::new(
        HostList::new(Defaults::Keep, &[]).unwrap(),
    ));
    sandbox.home = Some(home.clone());
    sandbox.overlay = command_sandbox::detect_overlay(&tool);
    let w = prepare(&m, &sandbox);
    let cargo_home = if cfg!(target_os = "macos") {
        "CARGO_HOME=\"$PWD/.cargo-home\" "
    } else {
        ""
    };

    let command = format!("HOME='{}' {cargo_home}cargo fetch", home.display());
    let ran = run(&w, &m, &command);

    assert!(ran.ok, "{}", ran.out);
    assert!(m.chat.join("Cargo.lock").exists());
    assert_eq!(cached(&home), before, "the real home folder changed");
}

/// The crate files of that name in the cache of cargo under `home`.
fn crate_files(home: &Path, name: &str) -> usize {
    let cache = home.join(".cargo/registry/cache");
    let Ok(indexes) = fs::read_dir(cache) else {
        return 0;
    };
    indexes
        .flatten()
        .filter(|index| index.path().join(name).exists())
        .count()
}

#[cfg(target_os = "linux")]
#[test]
fn a_download_into_the_home_of_cargo_lands_in_the_temp_folder_and_the_home_never_changes() {
    let Some(tool) = tool() else { return };
    // The bwrap 0.9.0 of Ubuntu 24.04 in CI has no overlay, so this test skips there.
    if command_sandbox::detect_overlay(&tool) == command_sandbox::Overlay::Missing {
        eprintln!("skipped: bwrap has no overlay");
        return;
    }
    let m = machine();
    let registry = m.home.join(".cargo/registry");
    fs::create_dir_all(&registry).unwrap();
    fs::write(registry.join("old.crate"), "old crate").unwrap();
    fs::write(m.home.join(".cargo/credentials.toml"), "secret token").unwrap();
    let mut sandbox = sandbox(&m, tool.clone());
    sandbox.overlay = command_sandbox::Overlay::Works;
    let w = prepare(&m, &sandbox);
    let cargo = m
        .home
        .join(".cargo")
        .display()
        .to_string()
        .replace('\'', "'\\''");

    let ran = run(
        &w,
        &m,
        &format!(
            "cd '{cargo}' && cat registry/old.crate && echo new > registry/new.crate && cat registry/new.crate credentials.toml"
        ),
    );

    assert!(ran.ok, "{}", ran.out);
    assert!(
        ran.out.contains("old crate") && ran.out.contains("new"),
        "{}",
        ran.out
    );
    assert!(!ran.out.contains("secret"), "{}", ran.out);
    assert!(!registry.join("new.crate").exists());
    let upper = &w.walls.overlays[0].upper;
    assert!(upper.starts_with(&w.walls.temp));
    assert!(upper.join("registry/new.crate").exists());
}
