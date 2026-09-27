use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use rappct::capability::derive_named_capability_sids;
use rappct::launch::{JobLimits, LaunchOptions, StdioConfig};
use rappct::{AppContainerProfile, SecurityCapabilities, launch_in_container_with_io};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("outer") => outer(),
        Some("loop") => inner_loop(),
        Some("net") => inner_net(&args[2]),
        Some("spawn") => inner_spawn(),
        _ => eprintln!("bad args"),
    }
}

fn inner_spawn() {
    let cmd = "C:\\Windows\\System32\\cmd.exe";
    let s = Command::new(cmd).args(["/c", "echo child-inherit"]).status();
    println!("SPAWN-INHERIT {s:?}");
    let o = Command::new(cmd).args(["/c", "echo child-piped"]).output();
    println!("SPAWN-PIPED {:?}", o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()));
    let n = Command::new(cmd).args(["/c", "echo child-null"]).stdout(Stdio::null()).status();
    println!("SPAWN-NULL {n:?}");
}

/// Listens on loopback, then a child process (same container) connects.
fn inner_loop() {
    let l = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            println!("LOOP-BIND-FAIL {e}");
            return;
        }
    };
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming() {
            match c {
                Ok(mut c) => {
                    let _ = c.write_all(b"pong\n");
                }
                Err(e) => println!("LOOP-ACCEPT-FAIL {e}"),
            }
        }
    });
    let exe = std::env::current_exe().unwrap();
    let s = Command::new(&exe).args(["net", &format!("127.0.0.1:{port}")]).status();
    println!("LOOP-CHILD-STATUS {s:?}");
}

fn inner_net(addr: &str) {
    let r = std::net::TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(5));
    match r {
        Ok(mut s) => {
            let mut b = [0u8; 5];
            let n = s.read(&mut b).unwrap_or(0);
            println!("NET-CONNECT-OK {addr} {}", String::from_utf8_lossy(&b[..n]).trim())
        }
        Err(e) => println!("NET-CONNECT-FAIL {addr} {e}"),
    }
}

fn icacls(args: &[&str]) {
    let o = Command::new("icacls").args(args).output().unwrap();
    println!("icacls {:?} -> {:?}", args, o.status.code());
}

fn cap(name: &str) -> String {
    derive_named_capability_sids(&[name]).unwrap()[0].sid_sddl.clone()
}

struct Ran {
    code: u32,
    out: String,
}

fn run_ac(sec: &SecurityCapabilities, exe: &Path, cmdline: &str, cwd: &Path, env: &[(String, String)], stdio: StdioConfig) -> Ran {
    let opts = LaunchOptions {
        exe: exe.to_path_buf(),
        cmdline: Some(cmdline.to_string()),
        cwd: Some(cwd.to_path_buf()),
        env: Some(env.iter().map(|(k, v)| (k.into(), v.into())).collect()),
        stdio,
        suspended: false,
        join_job: Some(JobLimits { memory_bytes: None, cpu_rate_percent: None, kill_on_job_close: true }),
        startup_timeout: None,
    };
    let mut io = match launch_in_container_with_io(sec, &opts) {
        Ok(io) => io,
        Err(e) => return Ran { code: 9999, out: format!("LAUNCH-FAIL {e:?}") },
    };
    drop(io.stdin.take());
    let out = io.stdout.take();
    let err = io.stderr.take();
    let t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut err) = err {
            let _ = err.read_to_string(&mut s);
        }
        s
    });
    let mut s = String::new();
    if let Some(mut out) = out {
        let _ = out.read_to_string(&mut s);
    }
    let e = t.join().unwrap();
    let code = io.wait(Some(Duration::from_secs(60))).unwrap_or(8888);
    Ran { code, out: format!("{s}{e}") }
}

fn show(name: &str, r: &Ran) {
    println!("=== {name}: code={} ({:#x}) out={}", r.code, r.code, r.out.trim());
}

fn outer() {
    let root = std::env::temp_dir().join(format!("acspike-{}", std::process::id()));
    let chat = root.join("chat");
    let temp = root.join("temp");
    std::fs::create_dir_all(chat.join(".git/hooks")).unwrap();
    std::fs::create_dir_all(chat.join(".git2/hooks")).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(chat.join(".env"), "secret env").unwrap();
    std::fs::write(chat.join(".env2"), "secret env2").unwrap();
    std::fs::write(chat.join(".git/hooks/pre-commit"), "old hook").unwrap();
    std::fs::write(chat.join(".git2/hooks/pre-commit"), "old hook2").unwrap();

    let prof = AppContainerProfile::ensure(&format!("gnomish.spike.{}", std::process::id()), "spike", Some("spike run")).unwrap();
    let pkg = prof.sid.to_string();
    let cap_chat = cap("gnomishRelayChatSpike");
    let chat_s = chat.display().to_string();
    icacls(&[&chat_s, "/grant", &format!("*{cap_chat}:(OI)(CI)M")]);
    icacls(&[&temp.display().to_string(), "/grant", &format!("*{pkg}:(OI)(CI)M")]);
    // Denies keyed to the package SID.
    icacls(&[&chat.join(".env").display().to_string(), "/deny", &format!("*{pkg}:F")]);
    icacls(&[&chat.join(".git\\hooks").display().to_string(), "/deny", &format!("*{pkg}:(OI)(CI)F")]);
    icacls(&[&chat.join(".git").display().to_string(), "/deny", &format!("*{pkg}:(DE)")]);
    // Denies keyed to ALL APPLICATION PACKAGES.
    icacls(&[&chat.join(".env2").display().to_string(), "/deny", "*S-1-15-2-1:F"]);
    icacls(&[&chat.join(".git2\\hooks").display().to_string(), "/deny", "*S-1-15-2-1:(OI)(CI)F"]);
    let exe = std::env::current_exe().unwrap();
    icacls(&[&exe.display().to_string(), "/grant", &format!("*{pkg}:RX")]);

    let caps = derive_named_capability_sids(&["gnomishRelayChatSpike"]).unwrap();
    let sec = SecurityCapabilities { package: prof.sid.clone(), caps, lpac: false };
    let mut env: Vec<(String, String)> = ["SystemRoot", "PATH", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TERM", "USERNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .chain([("HOME".into(), temp.display().to_string()), ("TMPDIR".into(), temp.display().to_string())])
        .collect();
    env.sort_by_key(|(k, _)| k.to_ascii_uppercase());
    let cmd = PathBuf::from("C:\\Windows\\System32\\cmd.exe");
    let p = StdioConfig::Pipe;

    show("read env (pkg deny)", &run_ac(&sec, &cmd, "cmd /c type .env", &chat, &env, p));
    show("read env2 (AAP deny)", &run_ac(&sec, &cmd, "cmd /c type .env2", &chat, &env, p));
    show("del env", &run_ac(&sec, &cmd, "cmd /c del .env", &chat, &env, p));
    println!(".env exists {}", chat.join(".env").exists());
    show("write hook (pkg deny)", &run_ac(&sec, &cmd, "cmd /c echo evil> .git\\hooks\\pre-commit", &chat, &env, p));
    println!("hook {:?}", std::fs::read_to_string(chat.join(".git/hooks/pre-commit")));
    show("write hook2 (AAP deny)", &run_ac(&sec, &cmd, "cmd /c echo evil> .git2\\hooks\\pre-commit", &chat, &env, p));
    println!("hook2 {:?}", std::fs::read_to_string(chat.join(".git2/hooks/pre-commit")));
    show("new file in hooks", &run_ac(&sec, &cmd, "cmd /c echo evil> .git\\hooks\\post-checkout", &chat, &env, p));
    println!("post-checkout {}", chat.join(".git/hooks/post-checkout").exists());
    show("rename .git", &run_ac(&sec, &cmd, "cmd /c ren .git gitx", &chat, &env, p));
    println!(".git exists {}", chat.join(".git").exists());
    show("write chat", &run_ac(&sec, &cmd, "cmd /c echo made> made.txt", &chat, &env, p));
    println!("made {}", chat.join("made.txt").exists());

    let tools: [(&str, &str); 7] = [
        ("C:\\Windows\\System32\\whoami.exe", "whoami"),
        ("C:\\Windows\\System32\\where.exe", "where cmd"),
        ("C:\\Program Files\\Git\\cmd\\git.exe", "git --version"),
        ("C:\\Program Files\\Git\\mingw64\\bin\\curl.exe", "curl --version"),
        ("C:\\Program Files\\Git\\usr\\bin\\ls.exe", "ls -la"),
        ("C:\\Program Files\\Git\\usr\\bin\\bash.exe", "bash -c \"echo hi; pwd -P; (echo sub); echo $(echo s2)\""),
        ("C:\\Program Files\\Git\\bin\\bash.exe", "bash -c \"echo hi\""),
    ];
    for (exe_path, line) in tools {
        for (label, stdio) in [("pipe", StdioConfig::Pipe), ("null", StdioConfig::Null), ("inherit", StdioConfig::Inherit)] {
            show(&format!("{line} [{label}]"), &run_ac(&sec, Path::new(exe_path), line, &chat, &env, stdio));
        }
    }
    show("rust spawn", &run_ac(&sec, &exe, "acspike spawn", &chat, &env, p));
    show("rust spawn inherit", &run_ac(&sec, &exe, "acspike spawn", &chat, &env, StdioConfig::Inherit));
    show("loop", &run_ac(&sec, &exe, "acspike loop", &chat, &env, p));
    let _ = prof.delete();
}
