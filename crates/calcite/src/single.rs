//! One Calcite at a time per data folder. Opening it again hands the
//! request (come to the front, or open these files) to the copy that's
//! already running, then quits.
//!
//! The first copy holds an exclusive lock on `calcite.lock` and listens on
//! a random localhost port, written with a random token to `calcite.port`.
//! Only a process that can read that file can talk to it.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Focus,
    Open(Vec<PathBuf>),
}

pub enum Claim {
    /// This is the only copy. Keep the lock for as long as it runs;
    /// requests from later launches arrive on `requests`.
    First {
        lock: File,
        requests: UnboundedReceiver<Request>,
    },
    /// Another copy is running and got the request: this one should quit.
    Handed,
}

pub fn claim(dir: &Path, request: &Request) -> anyhow::Result<Claim> {
    std::fs::create_dir_all(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("calcite.lock"))?;
    match lock.try_lock() {
        Ok(()) => listen(dir, lock),
        Err(std::fs::TryLockError::WouldBlock) => {
            hand_over(dir, request)?;
            Ok(Claim::Handed)
        }
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

fn listen(dir: &Path, lock: File) -> anyhow::Result<Claim> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let token = format!("{:032x}", rand_token());
    write_private(&dir.join("calcite.port"), &format!("{port} {token}\n"))?;
    let (tx, requests) = unbounded();
    std::thread::Builder::new()
        .name("calcite-single".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                if let Some(request) = read_request(stream, &token)
                    && tx.unbounded_send(request).is_err()
                {
                    break;
                }
            }
        })?;
    Ok(Claim::First { lock, requests })
}

fn read_request(stream: TcpStream, token: &str) -> Option<Request> {
    let mut reader = BufReader::new(stream.take(64 * 1024));
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    if first.trim() != token {
        return None;
    }
    let mut paths = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if let Some(path) = line.strip_prefix("open ") {
            paths.push(PathBuf::from(path));
        }
    }
    Some(if paths.is_empty() {
        Request::Focus
    } else {
        Request::Open(paths)
    })
}

fn hand_over(dir: &Path, request: &Request) -> anyhow::Result<()> {
    let info = std::fs::read_to_string(dir.join("calcite.port"))?;
    let mut parts = info.split_whitespace();
    let port: u16 = parts.next().unwrap_or_default().parse()?;
    let token = parts.next().unwrap_or_default();
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))?;
    let mut message = format!("{token}\n");
    match request {
        Request::Focus => message.push_str("focus\n"),
        Request::Open(paths) => {
            for path in paths {
                message.push_str(&format!("open {}\n", path.display()));
            }
        }
    }
    stream.write_all(message.as_bytes())?;
    Ok(())
}

/// Write a file only this user can read.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

/// 122 random bits from the system (a v4 UUID).
fn rand_token() -> u128 {
    u128::from_le_bytes(*uuid::Uuid::new_v4().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;

    #[test]
    fn second_launch_hands_over_and_first_hears_it() {
        let dir = tempfile::tempdir().unwrap();
        let Claim::First { lock, mut requests } = claim(dir.path(), &Request::Focus).unwrap()
        else {
            panic!("the first claim should win");
        };
        let open = Request::Open(vec![PathBuf::from("/tmp/My Pack.mrpack")]);
        assert!(matches!(claim(dir.path(), &open).unwrap(), Claim::Handed));
        let heard = futures::executor::block_on(requests.next()).unwrap();
        assert_eq!(heard, open);
        assert!(matches!(
            claim(dir.path(), &Request::Focus).unwrap(),
            Claim::Handed
        ));
        assert_eq!(
            futures::executor::block_on(requests.next()).unwrap(),
            Request::Focus
        );
        drop(lock);
    }

    #[test]
    fn strangers_without_the_token_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let Claim::First { lock, mut requests } = claim(dir.path(), &Request::Focus).unwrap()
        else {
            panic!("the first claim should win");
        };
        let info = std::fs::read_to_string(dir.path().join("calcite.port")).unwrap();
        let port: u16 = info.split_whitespace().next().unwrap().parse().unwrap();
        let mut stranger = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stranger
            .write_all(b"wrong-token\nopen /etc/passwd\n")
            .unwrap();
        drop(stranger);
        // A real request afterwards is the first thing that arrives.
        assert!(matches!(
            claim(dir.path(), &Request::Focus).unwrap(),
            Claim::Handed
        ));
        assert_eq!(
            futures::executor::block_on(requests.next()).unwrap(),
            Request::Focus
        );
        drop(lock);
    }
}
