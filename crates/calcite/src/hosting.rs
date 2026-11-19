//! Servers running on this computer: their list, the processes, and what
//! they print.
//!
//! A server is a plain child process. Its output is read on two small
//! threads and sent to the UI; commands go straight into its stdin. When
//! Calcite quits, every running server is told to `stop`, so the world is
//! saved even though Calcite doesn't wait around for it.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use calcite_core::local_server::{self, LocalServer, NewServer};
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, SharedString};

use crate::backend::{Backend, data_dir};

/// Lines of console kept per server.
const SCROLLBACK: usize = 2000;
/// After `stop`, how long before Force stop is offered.
pub const STOP_GRACE: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Finding Java (maybe downloading it) or loading the world.
    Starting,
    Online,
    Stopping,
}

pub struct Running {
    pub state: RunState,
    pub lines: VecDeque<SharedString>,
    pub players: Vec<String>,
    pub since: Instant,
    pub stop_asked: Option<Instant>,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
    child: Option<Arc<Mutex<Child>>>,
}

impl Running {
    fn push(&mut self, line: impl Into<SharedString>) {
        if self.lines.len() == SCROLLBACK {
            self.lines.pop_front();
        }
        self.lines.push_back(line.into());
    }
}

enum Event {
    Line(PathBuf, String),
    Started(PathBuf, Arc<Mutex<ChildStdin>>, Arc<Mutex<Child>>),
    Exited(PathBuf, Option<i32>),
    Failed(PathBuf, String),
}

pub struct Hosting {
    pub servers: Vec<LocalServer>,
    pub running: HashMap<PathBuf, Running>,
    /// What the server being made is doing right now.
    pub creating: Option<SharedString>,
    pub create_error: Option<SharedString>,
    /// Release versions, newest first, once fetched.
    pub versions: Option<Vec<String>>,
    /// The last thing a stopped server said, for when it fails at once.
    pub last_exit: HashMap<PathBuf, SharedString>,
    /// What adding voice chat to a server is doing, or how it went.
    pub voice_status: HashMap<PathBuf, (SharedString, bool)>,
    events: UnboundedSender<Event>,
}

struct HostingHandle(Entity<Hosting>);
impl Global for HostingHandle {}

impl Hosting {
    pub fn global(cx: &App) -> Entity<Hosting> {
        cx.global::<HostingHandle>().0.clone()
    }

    pub fn init(cx: &mut App) {
        let (events, mut rx) = unbounded::<Event>();
        let hosting = cx.new(|_| Hosting {
            servers: Vec::new(),
            running: HashMap::new(),
            creating: None,
            create_error: None,
            versions: None,
            last_exit: HashMap::new(),
            voice_status: HashMap::new(),
            events,
        });
        cx.set_global(HostingHandle(hosting.clone()));
        hosting.update(cx, |h, cx| h.reload(cx));

        let weak = hosting.downgrade();
        cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while let Some(event) = rx.next().await {
                if weak
                    .update(cx, |h, cx| {
                        h.handle(event, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // Quitting Calcite saves and stops every world instead of leaving
        // servers running with nobody able to reach their console.
        let weak = hosting.downgrade();
        cx.on_app_quit(move |cx| {
            if let Some(hosting) = weak.upgrade() {
                hosting.update(cx, |h, _| {
                    for running in h.running.values() {
                        if let Some(stdin) = &running.stdin
                            && let Ok(mut stdin) = stdin.lock()
                        {
                            let _ = stdin.write_all(b"stop\n");
                            let _ = stdin.flush();
                        }
                    }
                });
            }
            async {}
        })
        .detach();
    }

    pub fn root() -> Option<PathBuf> {
        data_dir().map(|d| local_server::root(&d))
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.servers = Self::root()
            .map(|root| local_server::list(&root))
            .unwrap_or_default();
        cx.notify();
    }

    pub fn server(&self, dir: &Path) -> Option<&LocalServer> {
        self.servers.iter().find(|s| s.dir == dir)
    }

    pub fn load_versions(&mut self, cx: &mut Context<Self>) {
        if self.versions.is_some() {
            return;
        }
        let http = http_client();
        let task =
            Backend::global(cx).run(async move { local_server::game_versions(&http?).await });
        cx.spawn(async move |this, cx| {
            let versions = task.await.ok().and_then(|r| r.ok());
            let _ = this.update(cx, |h, cx| {
                h.versions = Some(versions.unwrap_or_default());
                cx.notify();
            });
        })
        .detach();
    }

    pub fn create(&mut self, new: NewServer, cx: &mut Context<Self>) {
        let Some(root) = Self::root() else { return };
        if self.creating.is_some() {
            return;
        }
        self.creating = Some("Getting ready".into());
        self.create_error = None;
        cx.notify();
        let (progress_tx, mut progress_rx) = unbounded::<String>();
        let task = Backend::global(cx).run(async move {
            let http = http_client()?;
            local_server::create(&http, &root, new, |line| {
                let _ = progress_tx.unbounded_send(line.to_string());
            })
            .await
        });
        cx.spawn(async move |this, cx| {
            use futures::StreamExt as _;
            // Progress lines until the channel closes with the task.
            let progress = {
                let this = this.clone();
                let cx = cx.clone();
                async move {
                    let mut cx = cx;
                    while let Some(line) = progress_rx.next().await {
                        let _ = this.update(&mut cx, |h, cx| {
                            h.creating = Some(line.into());
                            cx.notify();
                        });
                    }
                }
            };
            let (result, ()) = futures::join!(task, progress);
            let _ = this.update(cx, |h, cx| {
                h.creating = None;
                match result.and_then(|r| r) {
                    Ok(_) => h.reload(cx),
                    Err(error) => h.create_error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn accept_eula(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if let Some(server) = self.server(dir) {
            let _ = server.accept_eula();
        }
        cx.notify();
    }

    /// Put Simple Voice Chat on a Fabric or Paper server.
    pub fn add_voice_chat(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let Some(server) = self.server(dir).cloned() else {
            return;
        };
        let dir = dir.to_path_buf();
        self.voice_status
            .insert(dir.clone(), ("Adding Simple Voice Chat…".into(), false));
        cx.notify();
        let task = Backend::global(cx)
            .run(async move { local_server::add_voice_chat(&http_client()?, &server).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.and_then(|r| r);
            let _ = this.update(cx, |h, cx| {
                let message = match result {
                    Ok(_) if h.running.contains_key(&dir) => (
                        "Simple Voice Chat added. Restart the server to turn it on.".into(),
                        false,
                    ),
                    Ok(_) => (
                        "Simple Voice Chat added. Players need it in their game too, and it uses UDP port 24454 next to the game port.".into(),
                        false,
                    ),
                    Err(error) => (format!("Couldn't add voice chat: {error:#}").into(), true),
                };
                h.voice_status.insert(dir.clone(), message);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_memory(&mut self, dir: &Path, memory_mb: u32, cx: &mut Context<Self>) {
        if let Some(server) = self.servers.iter_mut().find(|s| s.dir == dir) {
            server.config.memory_mb = memory_mb;
            let _ = server.save();
        }
        cx.notify();
    }

    pub fn start(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let Some(server) = self.server(dir).cloned() else {
            return;
        };
        if self.running.contains_key(dir) || !server.eula_accepted() {
            return;
        }
        let mut running = Running {
            state: RunState::Starting,
            lines: VecDeque::new(),
            players: Vec::new(),
            since: Instant::now(),
            stop_asked: None,
            stdin: None,
            child: None,
        };
        running.push(format!(
            "Starting {} {} with Java {}…",
            server.config.kind.label(),
            server.config.game_version,
            server.config.java
        ));
        self.running.insert(dir.to_path_buf(), running);
        self.last_exit.remove(dir);
        cx.notify();

        let events = self.events.clone();
        let java = server.config.java;
        // Runs on its own; nothing waits for it here.
        drop(Backend::global(cx).run(async move {
            let path = match find_java(java).await {
                Ok(path) => path,
                Err(error) => {
                    let _ = events.unbounded_send(Event::Failed(
                        server.dir.clone(),
                        format!("Couldn't get Java {java}: {error:#}"),
                    ));
                    return;
                }
            };
            if let Err(error) = spawn(&server, &path, events.clone()) {
                let _ = events.unbounded_send(Event::Failed(
                    server.dir.clone(),
                    format!("Couldn't start the server: {error:#}"),
                ));
            }
        }));
    }

    /// Type a command into the server's console.
    pub fn send(&mut self, dir: &Path, command: &str, cx: &mut Context<Self>) {
        let command = command.trim().trim_start_matches('/');
        if command.is_empty() {
            return;
        }
        if let Some(running) = self.running.get_mut(dir) {
            let sent = running.stdin.as_ref().is_some_and(|stdin| {
                stdin.lock().is_ok_and(|mut stdin| {
                    writeln!(stdin, "{command}").is_ok() && stdin.flush().is_ok()
                })
            });
            if sent {
                running.push(format!("> {command}"));
                if command == "stop" {
                    running.state = RunState::Stopping;
                    running.stop_asked = Some(Instant::now());
                }
            }
        }
        cx.notify();
    }

    pub fn stop(&mut self, dir: &Path, cx: &mut Context<Self>) {
        self.send(dir, "stop", cx);
    }

    /// For a server that won't stop.
    pub fn kill(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if let Some(running) = self.running.get_mut(dir) {
            if let Some(child) = &running.child
                && let Ok(mut child) = child.lock()
            {
                let _ = child.kill();
            }
            running.push("Force stopped.");
        }
        cx.notify();
    }

    /// Delete a stopped server's folder, world and all.
    pub fn delete(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if self.running.contains_key(dir) {
            return;
        }
        let root = Self::root();
        let dir = dir.to_path_buf();
        // Only ever inside Calcite's servers folder.
        if !root.is_some_and(|root| dir.starts_with(&root) && dir != root) {
            return;
        }
        self.servers.retain(|s| s.dir != dir);
        cx.notify();
        cx.background_spawn(async move {
            if let Err(error) = std::fs::remove_dir_all(&dir) {
                tracing::warn!("couldn't delete server {}: {error}", dir.display());
            }
        })
        .detach();
    }

    fn handle(&mut self, event: Event, cx: &App) {
        let name = |dir: &Path, servers: &[LocalServer]| {
            servers
                .iter()
                .find(|s| s.dir == dir)
                .map(|s| s.config.name.clone())
                .unwrap_or_else(|| "Your server".into())
        };
        let mut went_online = false;
        let stopped_now = matches!(event, Event::Exited(..) | Event::Failed(..));
        let event_dir = match &event {
            Event::Line(dir, _)
            | Event::Started(dir, ..)
            | Event::Exited(dir, _)
            | Event::Failed(dir, _) => dir.clone(),
        };
        match event {
            Event::Started(dir, stdin, child) => {
                if let Some(running) = self.running.get_mut(&dir) {
                    running.stdin = Some(stdin);
                    running.child = Some(child);
                }
            }
            Event::Line(dir, line) => {
                if let Some(running) = self.running.get_mut(&dir) {
                    if running.state == RunState::Starting
                        && line.contains("Done (")
                        && line.contains("For help")
                    {
                        running.state = RunState::Online;
                        went_online = true;
                    }
                    if let Some(name) = player_event(&line, " joined the game") {
                        if !running.players.contains(&name) {
                            running.players.push(name);
                        }
                    } else if let Some(name) = player_event(&line, " left the game") {
                        running.players.retain(|p| *p != name);
                    }
                    running.push(line);
                }
            }
            Event::Failed(dir, message) => {
                self.running.remove(&dir);
                self.last_exit.insert(dir, message.into());
            }
            Event::Exited(dir, code) => {
                if let Some(running) = self.running.remove(&dir) {
                    let asked = running.stop_asked.is_some();
                    if !asked && code != Some(0) {
                        let tail = running
                            .lines
                            .iter()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .cloned()
                            .unwrap_or_default();
                        let code = code.map_or("unknown".to_string(), |c| c.to_string());
                        self.last_exit.insert(
                            dir,
                            format!("The server stopped on its own (exit code {code}). {tail}")
                                .into(),
                        );
                    }
                }
            }
        }
        if went_online {
            crate::notify::send(
                format!("{} is online", name(&event_dir, &self.servers)),
                "Friends on your network can join now.",
                cx,
            );
        }
        if let Some(message) = self.last_exit.get(&event_dir)
            && !self.running.contains_key(&event_dir)
            && stopped_now
        {
            crate::notify::send(
                format!("{} stopped", name(&event_dir, &self.servers)),
                message.to_string(),
                cx,
            );
        }
    }
}

/// `[12:00:00] [Server thread/INFO]: Steve joined the game` → "Steve".
fn player_event(line: &str, suffix: &str) -> Option<String> {
    let before = line.trim_end().strip_suffix(suffix)?;
    let name = before.rsplit(": ").next()?.trim();
    (!name.is_empty() && !name.contains(' ')).then(|| name.to_string())
}

fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("calcite/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .build()?)
}

/// A Java the server can use: one Calcite already knows, or a fresh one.
async fn find_java(major: u32) -> anyhow::Result<PathBuf> {
    if let Ok(known) = theseus::jre::get_java_versions().await
        && let Some(java) = known.get(&major)
        && Path::new(&java.path).exists()
    {
        return Ok(PathBuf::from(&java.path));
    }
    theseus::jre::auto_install_java(major)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn spawn(server: &LocalServer, java: &Path, events: UnboundedSender<Event>) -> anyhow::Result<()> {
    let mut command = Command::new(java);
    command
        .args(server.java_args())
        .current_dir(&server.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // CREATE_NO_WINDOW: no console window next to Calcite.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    let stdin = Arc::new(Mutex::new(child.stdin.take().expect("piped")));
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let child = Arc::new(Mutex::new(child));
    let dir = server.dir.clone();
    let _ = events.unbounded_send(Event::Started(dir.clone(), stdin, child.clone()));

    for (name, pipe) in [
        (
            "server-out",
            Box::new(stdout) as Box<dyn std::io::Read + Send>,
        ),
        ("server-err", Box::new(stderr)),
    ] {
        let events = events.clone();
        let dir = dir.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                // Keep reading even past text that isn't UTF-8: a reader
                // that stops would fill the pipe and freeze the server.
                let mut reader = BufReader::new(pipe);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = String::from_utf8_lossy(&buf)
                        .trim_end_matches(['\n', '\r'])
                        .to_string();
                    if events
                        .unbounded_send(Event::Line(dir.clone(), line))
                        .is_err()
                    {
                        // Calcite is going away; keep draining so the
                        // server never blocks on a full pipe.
                        while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                            buf.clear();
                        }
                        break;
                    }
                }
            })?;
    }
    std::thread::Builder::new()
        .name("server-wait".into())
        .spawn(move || {
            let code = loop {
                let status = child.lock().ok().and_then(|mut c| c.try_wait().ok());
                match status {
                    Some(Some(status)) => break status.code(),
                    Some(None) => std::thread::sleep(Duration::from_millis(300)),
                    None => break None,
                }
            };
            let _ = events.unbounded_send(Event::Exited(dir, code));
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Start a fake server (a shell script standing in for Java), see its
    /// output, type into it, and see it exit after `stop`.
    #[test]
    #[cfg(unix)]
    fn a_server_process_talks_and_stops() {
        use futures::StreamExt as _;
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("fake-java");
        std::fs::write(
            &java,
            "#!/bin/sh\nprintf 'caf\\351\\n'\necho 'Done (1.0s)! For help, type \"help\"'\n\
             while read line; do echo \"got $line\"; [ \"$line\" = stop ] && exit 0; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        let server = LocalServer {
            dir: dir.path().to_path_buf(),
            config: calcite_core::local_server::Config {
                name: "Test".into(),
                kind: calcite_core::local_server::Kind::Vanilla,
                game_version: "1.21.4".into(),
                loader_version: None,
                java: 21,
                memory_mb: 1024,
                port: 25565,
                created: 0,
            },
        };
        let (tx, mut rx) = unbounded();
        spawn(&server, &java, tx).unwrap();
        let mut lines = Vec::new();
        let mut code = None;
        futures::executor::block_on(async {
            while let Some(event) = rx.next().await {
                match event {
                    Event::Started(_, stdin, _) => {
                        writeln!(stdin.lock().unwrap(), "say hi").unwrap();
                        writeln!(stdin.lock().unwrap(), "stop").unwrap();
                    }
                    Event::Line(_, line) => lines.push(line),
                    Event::Exited(_, c) => {
                        code = c;
                        break;
                    }
                    Event::Failed(_, message) => panic!("{message}"),
                }
            }
        });
        assert_eq!(lines[0], "caf\u{fffd}", "bad UTF-8 is shown, not fatal");
        assert!(lines[1].starts_with("Done ("), "{lines:?}");
        assert!(lines.contains(&"got say hi".to_string()), "{lines:?}");
        assert!(lines.contains(&"got stop".to_string()), "{lines:?}");
        assert_eq!(code, Some(0));
    }

    #[test]
    fn players_are_read_from_join_and_leave_lines() {
        assert_eq!(
            player_event(
                "[12:00:00] [Server thread/INFO]: Steve joined the game",
                " joined the game"
            ),
            Some("Steve".into())
        );
        assert_eq!(
            player_event(
                "[12:00:00] [Server thread/INFO]: <Alex> I joined the game",
                " joined the game"
            ),
            None
        );
        assert_eq!(player_event("Steve left", " left the game"), None);
    }
}
