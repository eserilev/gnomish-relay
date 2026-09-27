//! The proxy of the sandbox (SPEC.md 6.6.4): the only way out for a command of a game
//! run. It takes `CONNECT` to a host of the allow list on port 443 or 80, and it never
//! looks inside the TLS. It resolves the name once, checks every address, and connects
//! to a checked address, so no second lookup can lead somewhere else.

use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::allow_hosts::HostList;
use crate::connect_line::{BadRequest, MAX_HEAD, Target, head_end, parse_connect};
use crate::pipe::{Pipe, relay};
use crate::public_ip::is_public;

pub const PORTS: [u16; 2] = [443, 80];

/// How the proxy finds and reaches a host. Tests put a fake server behind a public address.
#[derive(Clone, Copy)]
pub struct Net {
    pub resolve: fn(&str, u16) -> io::Result<Vec<SocketAddr>>,
    pub connect: fn(&SocketAddr, Duration) -> io::Result<TcpStream>,
}

impl Net {
    pub fn system() -> Net {
        Net {
            resolve: system_resolve,
            connect: TcpStream::connect_timeout,
        }
    }
}

fn system_resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    Ok((host, port).to_socket_addrs()?.collect())
}

impl std::fmt::Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Net")
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// More connections at once get `503`.
    pub connections: usize,
    /// The longest wait for the head of a request.
    pub head: Duration,
    pub connect: Duration,
    /// A connection where neither side sends for this long ends.
    pub idle: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            connections: 64,
            head: Duration::from_secs(10),
            connect: Duration::from_secs(10),
            idle: Duration::from_mins(5),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProxySettings {
    pub hosts: Arc<HostList>,
    pub net: Net,
    pub limits: Limits,
}

impl ProxySettings {
    pub fn new(hosts: HostList) -> ProxySettings {
        ProxySettings {
            hosts: Arc::new(hosts),
            net: Net::system(),
            limits: Limits::default(),
        }
    }
}

/// One proxy for one run. It stops when it drops.
pub struct Proxy {
    stop: Arc<AtomicBool>,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}

enum Wake {
    #[cfg(unix)]
    Unix(PathBuf),
    Tcp(SocketAddr),
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept loop wakes up only for a connection.
        match &self.wake {
            #[cfg(unix)]
            Wake::Unix(path) => drop(std::os::unix::net::UnixStream::connect(path)),
            Wake::Tcp(addr) => drop(TcpStream::connect(addr)),
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The proxy on a Unix socket at `path`, for the forwarder inside `bwrap`.
#[cfg(unix)]
pub fn listen_unix(path: &Path, settings: ProxySettings, tag: String) -> io::Result<Proxy> {
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    let stop = Arc::new(AtomicBool::new(false));
    let context = Context::new(settings, tag, Arc::clone(&stop));
    let thread = std::thread::spawn(move || context.serve_all(listener.incoming()));
    Ok(Proxy {
        stop,
        wake: Wake::Unix(path.to_owned()),
        thread: Some(thread),
    })
}

/// The proxy on a loopback port, for Seatbelt, which allows only that port.
pub fn listen_tcp(settings: ProxySettings, tag: String) -> io::Result<(Proxy, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let context = Context::new(settings, tag, Arc::clone(&stop));
    let thread = std::thread::spawn(move || context.serve_all(listener.incoming()));
    let proxy = Proxy {
        stop,
        wake: Wake::Tcp(addr),
        thread: Some(thread),
    };
    Ok((proxy, addr.port()))
}

/// What every connection of one proxy shares.
struct Context {
    settings: ProxySettings,
    /// Names the chat in each log line.
    tag: String,
    open: AtomicUsize,
    stop: Arc<AtomicBool>,
}

impl Context {
    fn new(settings: ProxySettings, tag: String, stop: Arc<AtomicBool>) -> Arc<Context> {
        Arc::new(Context {
            settings,
            tag,
            open: AtomicUsize::new(0),
            stop,
        })
    }

    fn serve_all<S: Pipe>(self: &Arc<Self>, clients: impl Iterator<Item = io::Result<S>>) {
        for client in clients {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            if let Ok(client) = client {
                self.start(client);
            }
        }
    }

    fn start<S: Pipe>(self: &Arc<Self>, mut client: S) {
        if self.open.fetch_add(1, Ordering::SeqCst) >= self.settings.limits.connections {
            self.open.fetch_sub(1, Ordering::SeqCst);
            self.refuse(
                &mut client,
                "503 Service Unavailable",
                "too many connections",
            );
            return;
        }
        let context = Arc::clone(self);
        std::thread::spawn(move || {
            context.serve(client);
            context.open.fetch_sub(1, Ordering::SeqCst);
        });
    }

    fn serve<S: Pipe>(&self, mut client: S) {
        let limits = self.settings.limits;
        let Ok((head, rest)) = read_head(&mut client, limits.head) else {
            self.refuse(&mut client, "400 Bad Request", "no request head");
            return;
        };
        let target = match parse_connect(&head) {
            Ok(target) => target,
            Err(bad) => {
                self.refuse_request(&mut client, &head, &bad);
                return;
            }
        };
        let server = match self.open_server(&target) {
            Ok(server) => server,
            Err((status, why)) => {
                let what = format!("{}:{}: {why}", target.host, target.port);
                self.refuse(&mut client, status, &what);
                return;
            }
        };
        let _ = self.tunnel(client, server, &rest);
    }

    fn tunnel<S: Pipe>(&self, mut client: S, mut server: TcpStream, rest: &[u8]) -> io::Result<()> {
        client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")?;
        server.write_all(rest)?;
        relay(client, server, self.settings.limits.idle)
    }

    /// The connection to the host, or the status and the reason of a refusal.
    fn open_server(&self, target: &Target) -> Result<TcpStream, (&'static str, String)> {
        let forbidden = |why: &str| ("403 Forbidden", why.to_owned());
        if !PORTS.contains(&target.port) {
            return Err(forbidden("the proxy takes only ports 443 and 80"));
        }
        if !self.settings.hosts.allows(&target.host) {
            return Err(forbidden(
                "not on the allow list; add it to allow_hosts in [sandbox] of config.toml",
            ));
        }
        let net = self.settings.net;
        let addrs = (net.resolve)(&target.host, target.port)
            .map_err(|e| ("502 Bad Gateway", format!("no address: {e}")))?;
        if addrs.is_empty() {
            return Err(("502 Bad Gateway", "no address".into()));
        }
        if let Some(inside) = addrs.iter().find(|a| !is_public(a.ip())) {
            return Err(forbidden(&format!(
                "resolves to {}, which is not on the public internet",
                inside.ip()
            )));
        }
        addrs
            .iter()
            .find_map(|addr| (net.connect)(addr, self.settings.limits.connect).ok())
            .ok_or(("502 Bad Gateway", "no connection".into()))
    }

    fn refuse_request<S: Pipe>(&self, client: &mut S, head: &[u8], bad: &BadRequest) {
        let line = head.split(|&b| b == b'\r').next().unwrap_or_default();
        let line = String::from_utf8_lossy(&line[..line.len().min(200)]).into_owned();
        let status = match bad {
            BadRequest::NotConnect => "405 Method Not Allowed",
            BadRequest::IpAddress | BadRequest::BadHost => "403 Forbidden",
            BadRequest::NotHttp | BadRequest::NoPort => "400 Bad Request",
        };
        self.refuse(client, status, &format!("{line:?}: {}", bad.reason()));
    }

    fn refuse<S: Pipe>(&self, client: &mut S, status: &str, what: &str) {
        crate::run::log(&format!("proxy of {}: refused {what}", self.tag));
        let body = format!("gnomish-relay proxy: refused {what}\n");
        let answer = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = client.write_all(answer.as_bytes());
    }
}

/// The head and the bytes that came after it in the same reads.
fn read_head<S: Pipe>(client: &mut S, limit: Duration) -> io::Result<(Vec<u8>, Vec<u8>)> {
    client.set_read_timeout(Some(limit))?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        if let Some(end) = head_end(&bytes) {
            let rest = bytes.split_off(end);
            return Ok((bytes, rest));
        }
        if bytes.len() > MAX_HEAD {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let n = client.read(&mut buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(&buf[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::allow_hosts::Defaults;
    use std::io::{BufRead, Read};
    use std::sync::{Mutex, OnceLock};

    /// A public address that stands for the fake server in these tests.
    const PUBLIC: [u8; 4] = [93, 184, 216, 34];

    /// A server that answers each connection with `hello` and the first line it got.
    fn server() -> SocketAddr {
        static SERVER: OnceLock<SocketAddr> = OnceLock::new();
        *SERVER.get_or_init(|| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    std::thread::spawn(move || {
                        let mut stream = stream;
                        let mut line = String::new();
                        let _ = io::BufReader::new(&stream).read_line(&mut line);
                        let _ = write!(stream, "hello {line}");
                    });
                }
            });
            addr
        })
    }

    fn connected() -> &'static Mutex<Vec<SocketAddr>> {
        static CONNECTED: OnceLock<Mutex<Vec<SocketAddr>>> = OnceLock::new();
        CONNECTED.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn fake_resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        let ip: std::net::IpAddr = match host {
            "allowed.test" => PUBLIC.into(),
            "local.test" => [127, 0, 0, 1].into(),
            "private.test" => [10, 0, 0, 7].into(),
            "mapped.test" => "::ffff:192.168.0.1".parse().unwrap(),
            "mixed.test" => {
                return Ok(vec![
                    SocketAddr::from((PUBLIC, port)),
                    SocketAddr::from(([169, 254, 169, 254], port)),
                ]);
            }
            _ => return Err(io::ErrorKind::NotFound.into()),
        };
        Ok(vec![SocketAddr::new(ip, port)])
    }

    /// Records each address, and reaches the fake server in place of the public address.
    fn fake_connect(addr: &SocketAddr, limit: Duration) -> io::Result<TcpStream> {
        connected().lock().unwrap().push(*addr);
        if addr.ip() == std::net::IpAddr::from(PUBLIC) {
            return TcpStream::connect_timeout(&server(), limit);
        }
        TcpStream::connect_timeout(addr, limit)
    }

    fn settings(connections: usize) -> ProxySettings {
        let names = [
            "allowed.test",
            "local.test",
            "private.test",
            "mapped.test",
            "mixed.test",
        ];
        let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        ProxySettings {
            hosts: Arc::new(HostList::new(Defaults::Off, &names).unwrap()),
            net: Net {
                resolve: fake_resolve,
                connect: fake_connect,
            },
            limits: Limits {
                connections,
                ..Limits::default()
            },
        }
    }

    /// Sends `head` and then `then`, and gives all that comes back.
    fn ask(port: u16, head: &str, then: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(then.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut back = String::new();
        let _ = stream.read_to_string(&mut back);
        back
    }

    /// Sends `ping` only after a 200, as a real client does. A refusal closes the socket,
    /// and a byte that the proxy never read resets it, so the answer could be lost.
    fn connect_to(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "CONNECT {target} HTTP/1.1\r\n\r\n").unwrap();
        let mut back = answer_head(&mut stream);
        if back.starts_with("HTTP/1.1 200 ") {
            stream.write_all(b"ping\n").unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
        }
        let _ = stream.read_to_string(&mut back);
        back
    }

    /// Reads byte by byte, so no byte after the head leaves the stream.
    fn answer_head(stream: &mut TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
            head.push(byte[0]);
        }
        String::from_utf8(head).unwrap()
    }

    #[test]
    fn an_allowed_host_gets_a_tunnel_to_the_checked_address() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        let back = connect_to(port, "Allowed.Test:443");

        assert!(back.starts_with("HTTP/1.1 200 "), "{back}");
        assert!(back.ends_with("hello ping\n"), "{back}");
        assert!(
            connected()
                .lock()
                .unwrap()
                .contains(&SocketAddr::from((PUBLIC, 443)))
        );
    }

    #[test]
    fn bytes_right_after_the_head_reach_the_host() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        let back = ask(port, "CONNECT allowed.test:443 HTTP/1.1\r\n\r\nearly\n", "");

        assert!(back.ends_with("hello early\n"), "{back}");
    }

    #[test]
    fn a_host_that_is_not_on_the_list_is_refused() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        let back = connect_to(port, "example.com:443");

        assert!(back.starts_with("HTTP/1.1 403 "), "{back}");
        assert!(back.contains("allow_hosts"), "{back}");
    }

    #[test]
    fn a_port_other_than_443_or_80_is_refused() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        assert!(connect_to(port, "allowed.test:22").starts_with("HTTP/1.1 403 "));
        assert!(connect_to(port, "allowed.test:80").starts_with("HTTP/1.1 200 "));
    }

    #[test]
    fn an_ip_address_is_refused() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        let back = connect_to(port, &server().to_string());

        assert!(back.starts_with("HTTP/1.1 403 "), "{back}");
        assert!(back.contains("IP address"), "{back}");
    }

    #[test]
    fn a_name_that_resolves_to_this_computer_or_its_network_is_refused() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        for host in ["local.test", "private.test", "mapped.test", "mixed.test"] {
            let back = connect_to(port, &format!("{host}:443"));
            assert!(back.starts_with("HTTP/1.1 403 "), "{host}: {back}");
            assert!(back.contains("not on the public internet"), "{back}");
        }
        let tried = connected().lock().unwrap().clone();
        assert!(tried.iter().all(|a| is_public(a.ip())), "{tried:?}");
    }

    #[test]
    fn a_plain_http_request_is_refused() {
        let (_proxy, port) = listen_tcp(settings(8), "chat test".into()).unwrap();

        let back = ask(port, "GET http://allowed.test/ HTTP/1.1\r\n\r\n", "");

        assert!(back.starts_with("HTTP/1.1 405 "), "{back}");
    }

    #[test]
    fn a_name_with_no_address_or_no_server_gets_a_bad_gateway() {
        let mut settings = settings(8);
        settings.hosts = Arc::new(HostList::new(Defaults::Off, &["gone.test".into()]).unwrap());
        let (_proxy, port) = listen_tcp(settings, "chat test".into()).unwrap();

        assert!(connect_to(port, "gone.test:443").starts_with("HTTP/1.1 502 "));
    }

    #[test]
    fn a_head_that_is_too_long_or_never_ends_is_refused() {
        let mut settings = settings(8);
        settings.limits.head = Duration::from_millis(200);
        let (_proxy, port) = listen_tcp(settings, "chat test".into()).unwrap();

        let long = format!(
            "CONNECT allowed.test:443 HTTP/1.1\r\nX: {}",
            "a".repeat(MAX_HEAD)
        );
        assert!(ask(port, &long, "").starts_with("HTTP/1.1 400 "));
        let mut open = TcpStream::connect(("127.0.0.1", port)).unwrap();
        open.write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\n")
            .unwrap();
        let mut back = String::new();
        let _ = open.read_to_string(&mut back);
        assert!(back.starts_with("HTTP/1.1 400 "), "{back}");
    }

    #[test]
    fn more_connections_than_the_limit_are_refused() {
        let (_proxy, port) = listen_tcp(settings(1), "chat test".into()).unwrap();
        let mut first = TcpStream::connect(("127.0.0.1", port)).unwrap();
        first
            .write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut ok = [0u8; 12];
        first.read_exact(&mut ok).unwrap();

        // The proxy answers at once and closes. A request that the client wrote after the
        // close would reset the socket, and the answer could be lost, so it writes none.
        let mut second = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut back = String::new();
        let _ = second.read_to_string(&mut back);

        assert_eq!(&ok, b"HTTP/1.1 200");
        assert!(back.starts_with("HTTP/1.1 503 "), "{back}");
    }

    #[cfg(unix)]
    #[test]
    fn the_proxy_on_a_unix_socket_serves_and_stops_when_it_drops() {
        use std::os::unix::net::UnixStream;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy");
        let proxy = listen_unix(&path, settings(8), "chat test".into()).unwrap();
        let mut stream = UnixStream::connect(&path).unwrap();
        stream
            .write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\n\r\nping\n")
            .unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut back = String::new();
        stream.read_to_string(&mut back).unwrap();

        drop(proxy);

        assert!(back.ends_with("hello ping\n"), "{back}");
        assert!(UnixStream::connect(&path).is_err());
    }

    #[test]
    fn the_system_resolver_finds_no_address_for_a_name_that_cannot_exist() {
        let net = Net::system();

        assert!((net.resolve)("no-such-host.invalid", 443).map_or(true, |a| a.is_empty()));
        assert_eq!(format!("{net:?}"), "Net");
    }
}
