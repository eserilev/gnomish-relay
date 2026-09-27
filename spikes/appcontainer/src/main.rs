use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rappct::capability::derive_named_capability_sids;
use rappct::launch::{JobLimits, LaunchOptions, StdioConfig};
use rappct::{SecurityCapabilities, derive_sid_from_name, launch_in_container_with_io};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("outer") => outer(),
        Some("unix") => inner_unix(&args[2]),
        Some("loop") => inner_loop(&args[2]),
        Some("net") => inner_net(&args[2]),
        Some("serve") => inner_serve(),
        _ => eprintln!("bad args"),
    }
}

fn inner_unix(path: &str) {
    let addr = socket2::SockAddr::unix(path).unwrap();
    let s = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).unwrap();
    match s.connect(&addr) {
        Ok(()) => {
            (&s).write_all(b"ping").unwrap();
            let mut b = [0u8; 4];
            (&s).read_exact(&mut b).unwrap();
            println!("UNIX-CONNECT-OK {}", String::from_utf8_lossy(&b));
        }
        Err(e) => println!("UNIX-CONNECT-FAIL {e}"),
    }
}

/// Listens on loopback, then a child process (same container) connects.
fn inner_loop(bash: &str) {
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
    let self_conn = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(5),
    );
    println!("LOOP-SELF {:?}", self_conn.map(|_| "ok"));
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(&exe).args(["net", &format!("127.0.0.1:{port}")]).output();
    match out {
        Ok(o) => println!(
            "LOOP-CHILD-EXE {} {}",
            String::from_utf8_lossy(&o.stdout).trim(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => println!("LOOP-CHILD-EXE-SPAWN-FAIL {e}"),
    }
    let out = Command::new(bash)
        .args(["-c", &format!("exec 3<>/dev/tcp/127.0.0.1/{port} && head -c 5 <&3")])
        .output();
    match out {
        Ok(o) => println!(
            "LOOP-CHILD-BASH status={:?} out={} err={}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => println!("LOOP-CHILD-BASH-SPAWN-FAIL {e}"),
    }
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

fn inner_serve() {
    println!("SERVE");
}

fn icacls(args: &[&str]) {
    let o = Command::new("icacls").args(args).output().unwrap();
    println!(
        "icacls {:?} -> {:?} {}{}",
        args,
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).trim(),
        String::from_utf8_lossy(&o.stderr).trim()
    );
}

fn cap(name: &str) -> String {
    derive_named_capability_sids(&[name]).unwrap()[0].sid_sddl.clone()
}

struct Ran {
    code: u32,
    out: String,
}

fn run_ac(sec: &SecurityCapabilities, exe: &Path, cmdline: &str, cwd: &Path, env: &[(String, String)]) -> Ran {
    let opts = LaunchOptions {
        exe: exe.to_path_buf(),
        cmdline: Some(cmdline.to_string()),
        cwd: Some(cwd.to_path_buf()),
        env: Some(env.iter().map(|(k, v)| (k.into(), v.into())).collect()),
        stdio: StdioConfig::Pipe,
        suspended: false,
        join_job: Some(JobLimits { memory_bytes: None, cpu_rate_percent: None, kill_on_job_close: true }),
        startup_timeout: None,
    };
    let mut io = match launch_in_container_with_io(sec, &opts) {
        Ok(io) => io,
        Err(e) => return Ran { code: 9999, out: format!("LAUNCH-FAIL {e}") },
    };
    drop(io.stdin.take());
    let mut out = io.stdout.take().unwrap();
    let mut err = io.stderr.take().unwrap();
    let t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let mut s = String::new();
    let _ = out.read_to_string(&mut s);
    let e = t.join().unwrap();
    let code = io.wait(Some(Duration::from_secs(60))).unwrap_or(8888);
    Ran { code, out: format!("{s}{e}") }
}

fn show(name: &str, r: &Ran) {
    println!("=== {name}: code={} out={}", r.code, r.out.trim());
}

fn outer() {
    let root = std::env::temp_dir().join(format!("acspike-{}", std::process::id()));
    let chat = root.join("chat");
    let other = root.join("other");
    let temp = root.join("temp");
    std::fs::create_dir_all(chat.join(".git/hooks")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(chat.join(".env"), "secret env").unwrap();
    std::fs::write(chat.join("notes.txt"), "plain notes").unwrap();
    std::fs::write(chat.join(".git/hooks/pre-commit"), "old hook").unwrap();
    std::fs::write(other.join("o.txt"), "other text").unwrap();

    let pkg = derive_sid_from_name(&format!("gnomish.spike.{}", std::process::id())).unwrap();
    println!("package {pkg}");
    let cap_chat = cap("gnomishRelayChatSpike");
    let cap_cmd = cap("gnomishRelayCommand");
    println!("caps {cap_chat} {cap_cmd}");
    let chat_s = chat.display().to_string();
    let temp_s = temp.display().to_string();
    icacls(&[&chat_s, "/grant", &format!("*{cap_chat}:(OI)(CI)M")]);
    icacls(&[&temp_s, "/grant", &format!("*{cap_cmd}:(OI)(CI)M")]);
    icacls(&[&chat.join(".env").display().to_string(), "/deny", &format!("*{cap_cmd}:F")]);
    icacls(&[&chat.join(".git/hooks").display().to_string(), "/deny", &format!("*{cap_cmd}:(OI)(CI)F")]);
    icacls(&[&chat.join(".git").display().to_string(), "/deny", &format!("*{cap_cmd}:(DE)")]);
    let exe = std::env::current_exe().unwrap();
    icacls(&[&exe.display().to_string(), "/grant", &format!("*{cap_cmd}:RX")]);
    icacls(&[&chat_s]);
    icacls(&[&chat.join(".env").display().to_string()]);

    let caps = derive_named_capability_sids(&["gnomishRelayChatSpike", "gnomishRelayCommand"]).unwrap();
    let sec = SecurityCapabilities { package: pkg.clone(), caps, lpac: false };
    let env: Vec<(String, String)> = ["SystemRoot", "PATH", "PATHEXT", "ComSpec"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .chain([("HOME".into(), temp_s.clone()), ("TEMP".into(), temp_s.clone()), ("TMP".into(), temp_s.clone()), ("TMPDIR".into(), temp_s.clone())])
        .collect();
    let cmd = PathBuf::from(std::env::var("ComSpec").unwrap_or("C:\\Windows\\System32\\cmd.exe".into()));

    show("cmd echo", &run_ac(&sec, &cmd, "cmd /c echo hello", &chat, &env));
    show("cmd write chat", &run_ac(&sec, &cmd, "cmd /c echo made> made.txt", &chat, &env));
    println!("made exists {}", chat.join("made.txt").exists());
    show("cmd write other", &run_ac(&sec, &cmd, &format!("cmd /c echo x> \"{}\"", other.join("x.txt").display()), &chat, &env));
    println!("other x exists {}", other.join("x.txt").exists());
    show("cmd read other", &run_ac(&sec, &cmd, &format!("cmd /c type \"{}\"", other.join("o.txt").display()), &chat, &env));
    show("cmd read notes", &run_ac(&sec, &cmd, "cmd /c type notes.txt", &chat, &env));
    show("cmd read env", &run_ac(&sec, &cmd, "cmd /c type .env", &chat, &env));
    show("cmd write hook", &run_ac(&sec, &cmd, "cmd /c echo evil> .git\\hooks\\pre-commit", &chat, &env));
    println!("hook now {:?}", std::fs::read_to_string(chat.join(".git/hooks/pre-commit")));
    show("cmd write temp", &run_ac(&sec, &cmd, &format!("cmd /c echo t> \"{}\"", temp.join("t.txt").display()), &chat, &env));
    println!("temp t exists {}", temp.join("t.txt").exists());
    let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
    show("cmd dir userprofile", &run_ac(&sec, &cmd, &format!("cmd /c dir \"{userprofile}\""), &chat, &env));
    show("cmd whoami", &run_ac(&sec, &cmd, "cmd /c whoami /groups", &chat, &env));

    // Git bash.
    for bash in ["C:\\Program Files\\Git\\bin\\bash.exe", "C:\\Program Files\\Git\\usr\\bin\\bash.exe"] {
        let b = PathBuf::from(bash);
        show(&format!("bash {bash} echo"), &run_ac(&sec, &b, "bash -c 'echo hi; pwd; echo made2 > made2.txt; cat notes.txt; cat .env; echo evil > .git/hooks/pre-commit; echo x > ../other/y.txt; ls'", &chat, &env));
        println!("made2 {}", chat.join("made2.txt").exists());
        println!("other y {}", other.join("y.txt").exists());
    }

    let b = PathBuf::from("C:\\Program Files\\Git\\bin\\bash.exe");
    show("bash pwd -P", &run_ac(&sec, &b, "bash -c 'cd \"$PWD\" && pwd -P && pwd'", &chat, &env));
    show("bash rename .git", &run_ac(&sec, &b, "bash -c 'mv .git x && echo MOVED'", &chat, &env));
    println!(".git exists {}", chat.join(".git").exists());
    show("bash rm .env", &run_ac(&sec, &b, "bash -c 'rm -f .env; mv .env e2; ls -la'", &chat, &env));
    println!(".env exists {}", chat.join(".env").exists());
    show("git init+status", &run_ac(&sec, &b, "bash -c 'mkdir -p sub && cd sub && git init -q && git status && echo GITOK'", &chat, &env));
    show("bash subshell fork", &run_ac(&sec, &b, "bash -c '(echo sub1); echo $(echo sub2) | cat; sh -c \"echo sh3\"'", &chat, &env));

    // Exe inside the container.
    show("exe serve", &run_ac(&sec, &exe, "acspike serve", &chat, &env));

    // AF_UNIX.
    let sock = temp.join("proxy.sock");
    let addr = socket2::SockAddr::unix(&sock).unwrap();
    let l = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).unwrap();
    l.bind(&addr).unwrap();
    l.listen(8).unwrap();
    icacls(&[&sock.display().to_string()]);
    std::thread::spawn(move || {
        while let Ok((c, _)) = l.accept() {
            let mut b = [0u8; 4];
            if (&c).read_exact(&mut b).is_ok() {
                let _ = (&c).write_all(b"pong");
            }
        }
    });
    show("unix connect", &run_ac(&sec, &exe, &format!("acspike unix \"{}\"", sock.display()), &chat, &env));

    // Loopback inside the same container.
    show("loop", &run_ac(&sec, &exe, "acspike loop \"C:\\Program Files\\Git\\bin\\bash.exe\"", &chat, &env));

    // Loopback to an outside listener, and the internet.
    let outside = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = outside.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in outside.incoming().flatten() {
            let mut c = c;
            let _ = c.write_all(b"OUTSD");
        }
    });
    show("net outside loopback", &run_ac(&sec, &exe, &format!("acspike net 127.0.0.1:{port}"), &chat, &env));
    show("net internet", &run_ac(&sec, &exe, "acspike net 1.1.1.1:443", &chat, &env));
    // The same with internetClient, to see that the test can pass.
    let caps2 = derive_named_capability_sids(&["internetClient"]).unwrap();
    let sec2 = SecurityCapabilities { package: pkg, caps: caps2, lpac: false };
    icacls(&[&exe.display().to_string(), "/grant", "*S-1-15-2-1:RX"]);
    show("net internet with internetClient", &run_ac(&sec2, &exe, "acspike net 1.1.1.1:443", &temp, &env));
}
