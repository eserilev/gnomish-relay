//! `gnomish-relay attach`: ssh runs it on the server, and it copies stdin and stdout to
//! the serve socket (SPEC.md 11.7.3).

use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::dirs::Dirs;
use crate::serve::SOCKET_FILE;

const START_WAIT: Duration = Duration::from_secs(15);

pub fn attach(dirs: &Dirs) -> Result<()> {
    let path = dirs.data.join(SOCKET_FILE);
    let stream = match connect(&path) {
        Ok(stream) => stream,
        Err(Missing::No(e)) => return Err(e),
        Err(Missing::Yes) => {
            start_serve()?;
            wait_for(&path)?
        }
    };
    pipe(stream)
}

/// Whether `serve` needs a start: no socket, or a socket that nobody holds.
pub enum Missing {
    Yes,
    No(anyhow::Error),
}

/// A link in the data folder could lead to the socket of another program.
pub fn connect(path: &Path) -> Result<UnixStream, Missing> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(Missing::Yes),
        Err(e) => return Err(Missing::No(e.into())),
    };
    if meta.file_type().is_symlink() || !meta.file_type().is_socket() {
        return Err(Missing::No(anyhow::anyhow!(
            "{} is not a socket. Remove it, then try again.",
            path.display()
        )));
    }
    UnixStream::connect(path).map_err(|e| match e.kind() {
        ErrorKind::ConnectionRefused | ErrorKind::NotFound => Missing::Yes,
        _ => Missing::No(e.into()),
    })
}

/// Its own process group, so the end of the ssh session does not end it.
fn start_serve() -> Result<()> {
    Command::new(std::env::current_exe()?)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("cannot start gnomish-relay serve")?;
    Ok(())
}

fn wait_for(path: &Path) -> Result<UnixStream> {
    let start = Instant::now();
    while start.elapsed() < START_WAIT {
        match connect(path) {
            Ok(stream) => return Ok(stream),
            Err(Missing::No(e)) => return Err(e),
            Err(Missing::Yes) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    bail!(
        "gnomish-relay serve did not start. Run `gnomish-relay serve` on this computer to see why."
    )
}

fn pipe(stream: UnixStream) -> Result<()> {
    let mut to_serve = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = copy_flushed(&mut std::io::stdin().lock(), &mut to_serve);
        let _ = to_serve.shutdown(Shutdown::Write);
    });
    let mut from_serve = stream;
    copy_flushed(&mut from_serve, &mut std::io::stdout().lock())
}

/// A plain loop, not `io::copy`: on Linux `io::copy` can move a socket into a pipe with
/// `splice`, and then a line of the link waited in the kernel (found in the diff test).
fn copy_flushed(from: &mut impl Read, to: &mut impl Write) -> Result<()> {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        to.write_all(&buffer[..read])?;
        to.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_refuses_a_socket_that_is_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("other.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&elsewhere).unwrap();
        let path = dir.path().join(SOCKET_FILE);
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();

        assert!(matches!(connect(&path), Err(Missing::No(_))));
    }

    #[test]
    fn attach_refuses_a_plain_file_in_place_of_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_FILE);
        std::fs::write(&path, b"").unwrap();

        assert!(matches!(connect(&path), Err(Missing::No(_))));
    }

    #[test]
    fn a_missing_or_dead_socket_asks_for_a_start_of_serve() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_FILE);
        assert!(matches!(connect(&path), Err(Missing::Yes)));

        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());

        assert!(matches!(connect(&path), Err(Missing::Yes)));
    }
}
