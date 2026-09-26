//! Copies bytes both ways between two streams, for the proxy of the sandbox and its
//! forwarder inside the sandbox (SPEC.md 6.6.4).

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How often a quiet side checks whether the whole connection is idle.
const POLL: Duration = Duration::from_secs(5);

/// A stream that can split into a reader and a writer.
pub trait Pipe: Read + Write + Send + Sized + 'static {
    fn split(&self) -> io::Result<Self>;
    fn shut(&self, how: Shutdown);
    fn set_read_timeout(&self, limit: Option<Duration>) -> io::Result<()>;
}

impl Pipe for TcpStream {
    fn split(&self) -> io::Result<Self> {
        self.try_clone()
    }

    fn shut(&self, how: Shutdown) {
        let _ = self.shutdown(how);
    }

    fn set_read_timeout(&self, limit: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, limit)
    }
}

#[cfg(unix)]
impl Pipe for std::os::unix::net::UnixStream {
    fn split(&self) -> io::Result<Self> {
        self.try_clone()
    }

    fn shut(&self, how: Shutdown) {
        let _ = self.shutdown(how);
    }

    fn set_read_timeout(&self, limit: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, limit)
    }
}

/// Copies both ways until both sides end, or until neither side sends for `idle`. One
/// side can stay quiet for a long time while the other sends, as in a long download.
pub fn relay<A: Pipe, B: Pipe>(a: A, b: B, idle: Duration) -> io::Result<()> {
    let poll = Some(POLL.min(idle));
    a.set_read_timeout(poll)?;
    b.set_read_timeout(poll)?;
    let last = Arc::new(Mutex::new(Instant::now()));
    let (a_reader, b_reader) = (a.split()?, b.split()?);
    let up_last = Arc::clone(&last);
    let up = std::thread::spawn(move || pump(a_reader, b, &up_last, idle));
    pump(b_reader, a, &last, idle);
    let _ = up.join();
    Ok(())
}

fn pump<R: Pipe, W: Pipe>(mut from: R, mut to: W, last: &Mutex<Instant>, idle: Duration) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) => {
                to.shut(Shutdown::Write);
                return;
            }
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
                *last.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
            }
            Err(e) if is_wait(&e) && !is_idle(last, idle) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    from.shut(Shutdown::Both);
    to.shut(Shutdown::Both);
}

fn is_wait(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn is_idle(last: &Mutex<Instant>, idle: Duration) -> bool {
    last.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .elapsed()
        >= idle
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Two connected streams: the one that connected, and the one that the listener got.
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let near = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (far, _) = listener.accept().unwrap();
        (near, far)
    }

    #[test]
    fn the_relay_copies_both_ways_and_passes_the_end_on() {
        let (mut client, client_side) = pair();
        let (server_side, mut server) = pair();
        let relay = std::thread::spawn(move || {
            relay(client_side, server_side, Duration::from_secs(10)).unwrap();
        });

        client.write_all(b"ping").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut got = String::new();
        server.read_to_string(&mut got).unwrap();
        server.write_all(b"pong").unwrap();
        server.shutdown(Shutdown::Write).unwrap();
        let mut back = String::new();
        client.read_to_string(&mut back).unwrap();

        assert_eq!(got, "ping");
        assert_eq!(back, "pong");
        relay.join().unwrap();
    }

    #[test]
    fn a_connection_where_no_side_sends_ends_after_the_idle_time() {
        let (mut client, client_side) = pair();
        let (server_side, _server) = pair();

        relay(client_side, server_side, Duration::from_millis(200)).unwrap();

        let mut rest = Vec::new();
        assert_eq!(client.read_to_end(&mut rest).unwrap(), 0);
    }
}
