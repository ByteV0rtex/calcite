//! Live application state, kept in sync with the launcher backend.
//!
//! The backend is the source of truth. This entity holds a UI-friendly copy
//! that views observe. Backend events (instance changes, game start/stop,
//! install progress, log lines) update it as they arrive; nothing polls.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, SharedString, Task};
use theseus::AppEvent;
use theseus::data::{InstanceMetadata, ModLoader};
use theseus::event::{InstancePayloadType, ProcessPayloadType};
use theseus::install::{InstallJobSnapshot, InstallJobStatus};

use crate::backend::Backend;

/// Most log lines kept per instance. Older lines live in the log files.
const LOG_LINES_KEPT: usize = 4000;

#[derive(Clone, Debug)]
pub struct InstanceInfo {
    pub id: String,
    pub name: SharedString,
    pub icon_path: Option<PathBuf>,
    pub loader: ModLoader,
    pub game_version: SharedString,
    pub loader_version: Option<SharedString>,
    pub last_played: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
    pub played_secs: u64,
    pub installed: bool,
    pub group_ids: Vec<String>,
    pub is_modpack: bool,
    /// Set when this instance is shared through Modrinth.
    pub shared: Option<SharedLink>,
}

/// How an instance is connected to a Modrinth shared instance.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedLink {
    pub id: String,
    /// You share it (rather than having joined someone else's).
    pub owner: bool,
}

impl InstanceInfo {
    fn from_metadata(meta: InstanceMetadata) -> Self {
        let set = &meta.applied_content_set;
        InstanceInfo {
            id: meta.instance.id.clone(),
            name: meta.instance.name.clone().into(),
            icon_path: meta.instance.icon_path.as_ref().map(PathBuf::from),
            loader: set.loader,
            game_version: set.game_version.clone().into(),
            loader_version: set.loader_version.clone().map(Into::into),
            last_played: meta.instance.last_played,
            created: meta.instance.created,
            played_secs: meta
                .instance
                .submitted_time_played
                .saturating_add(meta.instance.recent_time_played),
            installed: meta.instance.install_stage.as_str() == "installed",
            group_ids: meta.group_ids.clone(),
            is_modpack: !matches!(meta.link, theseus::data::InstanceLink::Unmanaged),
            shared: meta.shared_instance.as_ref().map(|s| SharedLink {
                id: s.id.clone(),
                owner: matches!(s.role, theseus::data::SharedInstanceRole::Owner),
            }),
        }
    }

    /// "Fabric 1.21.4", "Vanilla 26.3".
    pub fn loader_line(&self) -> String {
        format!("{} {}", loader_name(self.loader), self.game_version)
    }
}

pub fn loader_name(loader: ModLoader) -> &'static str {
    match loader {
        ModLoader::Vanilla => "Vanilla",
        ModLoader::Fabric => "Fabric",
        ModLoader::Quilt => "Quilt",
        ModLoader::Forge => "Forge",
        ModLoader::NeoForge => "NeoForge",
    }
}

#[derive(Clone, Debug)]
pub struct Running {
    pub started: DateTime<Utc>,
    /// The last minute of memory/CPU readings, oldest first.
    pub usage: VecDeque<calcite_core::monitor::Sample>,
}

/// How many one-second readings the live monitor keeps.
pub const USAGE_HISTORY: usize = 60;

#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub instance_id: Option<String>,
    pub title: SharedString,
    pub phase: &'static str,
    pub current: u64,
    pub total: u64,
    pub status: InstallJobStatus,
    pub error: Option<String>,
    pub paused: bool,
    pub can_pause: bool,
    pub can_cancel: bool,
}

/// What can be done to a download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobAction {
    Pause,
    Resume,
    Retry,
    Cancel,
    Dismiss,
}

impl Job {
    fn from_snapshot(s: &InstallJobSnapshot) -> Self {
        let (current, total) = s
            .progress
            .as_ref()
            .map(|p| (p.current, p.total))
            .unwrap_or((0, 0));
        Job {
            id: s.job_id.clone(),
            instance_id: s.instance_id.clone(),
            title: s
                .display
                .as_ref()
                .map(|d| d.title.clone())
                .unwrap_or_else(|| "Installing".into())
                .into(),
            phase: phase_label(&phase_key(&s.phase)),
            current,
            total,
            status: s.status,
            error: s
                .error
                .as_ref()
                .and_then(|e| serde_json::to_value(e).ok())
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                }),
            paused: s.paused,
            can_pause: s.can_pause,
            can_cancel: s.can_cancel,
        }
    }

    /// Stopped and waiting for you: paused, interrupted or failed.
    pub fn needs_attention(&self) -> bool {
        self.paused
            || matches!(
                self.status,
                InstallJobStatus::Interrupted | InstallJobStatus::Failed
            )
    }

    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.current as f64 / self.total as f64) as f32
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            InstallJobStatus::Queued | InstallJobStatus::Running
        )
    }
}

/// The snake_case name of an install phase, as the backend serializes it.
pub fn phase_key(phase: &theseus::install::InstallPhaseId) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Short, player-facing names for install phases.
fn phase_label(phase: &str) -> &'static str {
    match phase {
        "preparing_instance" => "Preparing",
        "resolving_pack" | "reading_pack_manifest" => "Reading modpack",
        "downloading_pack_file" => "Downloading modpack",
        "downloading_content" => "Downloading mods",
        "extracting_overrides" => "Unpacking files",
        "resolving_minecraft" | "downloading_minecraft" => "Downloading Minecraft",
        "resolving_loader" | "running_loader_processors" => "Installing mod loader",
        "preparing_java" => "Setting up Java",
        "finalizing" => "Finishing up",
        "rolling_back" => "Undoing changes",
        _ => "Working",
    }
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: uuid::Uuid,
    pub name: SharedString,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: SharedString,
    pub level: LogLevel,
    pub text: SharedString,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackendStatus {
    Starting,
    Ready,
    Failed(String),
}

pub struct AppState {
    pub status: BackendStatus,
    pub instances: Vec<InstanceInfo>,
    pub groups: Vec<(String, SharedString)>,
    pub running: HashMap<String, Running>,
    pub jobs: Vec<Job>,
    pub accounts: Vec<Account>,
    /// The most downloaded modpacks, for Home (fetched once).
    pub popular: Option<Vec<calcite_core::modrinth::SearchHit>>,
    popular_loading: bool,
    /// The active account's Minecraft face, for avatars.
    pub face: Option<(uuid::Uuid, std::sync::Arc<gpui_kit::RenderImage>)>,
    pub default_account: Option<uuid::Uuid>,
    pub logs: HashMap<String, VecDeque<LogLine>>,
    /// A message for the user that hasn't been dismissed yet.
    pub notice: Option<Notice>,
    /// Game folders to bring into an instance once its install job succeeds
    /// (official launcher imports), keyed by job id.
    pending_copies: HashMap<String, (PathBuf, Option<PathBuf>)>,
    /// Install jobs making an instance for a listed server: join it
    /// (name, address) once the install finishes.
    pending_joins: HashMap<String, (String, String)>,
    /// Bumped whenever an instance's content changes, so open pages reload.
    pub content_revision: HashMap<String, u64>,
    /// Project ids being installed per instance (for "Installing…" buttons).
    pub installing: HashMap<String, Vec<String>>,
    /// Signed in to Modrinth (friends, shared instances, your projects).
    pub modrinth: Option<ModrinthUser>,
    /// Set while the browser sign-in is open; sending cancels it.
    pub modrinth_signing_in: Option<tokio::sync::oneshot::Sender<()>>,
    pub friends: Vec<Friend>,
    friends_task: Option<Task<()>>,
    pub recent_worlds: Vec<RecentWorld>,
    refresh_task: Option<Task<()>>,
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub title: SharedString,
    pub body: SharedString,
    pub is_error: bool,
}

/// The Modrinth account Calcite is signed in with.
#[derive(Clone, Debug, PartialEq)]
pub struct ModrinthUser {
    pub id: String,
    pub username: SharedString,
    pub avatar_url: Option<String>,
}

/// A world or server you played recently, for "Jump back in".
#[derive(Clone, Debug, PartialEq)]
pub struct RecentWorld {
    pub instance_id: String,
    pub name: SharedString,
    /// Folder name (singleplayer) or address (server).
    pub key: String,
    pub server: bool,
    pub last_played: Option<DateTime<Utc>>,
    pub icon: Option<String>,
}

/// Someone on your Modrinth friends list, or a request either way.
#[derive(Clone, Debug, PartialEq)]
pub struct Friend {
    pub user_id: String,
    pub username: SharedString,
    pub avatar_url: Option<String>,
    pub accepted: bool,
    /// They asked you (only meaningful while not accepted).
    pub incoming: bool,
    pub online: bool,
    /// The instance they're playing, if they share it.
    pub playing: Option<SharedString>,
}

pub struct AppStateHandle(pub Entity<AppState>);
impl Global for AppStateHandle {}

impl AppState {
    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<AppStateHandle>().0.clone()
    }

    /// Create the state, start the backend, and pump its events.
    pub fn start(
        events: futures::channel::mpsc::UnboundedReceiver<AppEvent>,
        cx: &mut App,
    ) -> Entity<AppState> {
        let state = cx.new(|_| AppState {
            status: BackendStatus::Starting,
            instances: Vec::new(),
            groups: Vec::new(),
            running: HashMap::new(),
            jobs: Vec::new(),
            accounts: Vec::new(),
            face: None,
            popular: None,
            popular_loading: false,
            default_account: None,
            logs: HashMap::new(),
            notice: None,
            pending_copies: HashMap::new(),
            pending_joins: HashMap::new(),
            content_revision: HashMap::new(),
            installing: HashMap::new(),
            modrinth: None,
            modrinth_signing_in: None,
            friends: Vec::new(),
            friends_task: None,
            recent_worlds: Vec::new(),
            refresh_task: None,
        });
        cx.set_global(AppStateHandle(state.clone()));

        let init = Backend::global(cx).init_state();
        let weak = state.downgrade();
        let started = std::time::Instant::now();
        cx.spawn(async move |cx| {
            let result = init.await;
            tracing::info!(
                "backend init finished in {:?}: {}",
                started.elapsed(),
                if result.is_ok() { "ok" } else { "failed" }
            );
            let updated = weak.update(cx, |this, cx| match result {
                Ok(moved) => {
                    if let Some(moved) = moved {
                        this.notice = Some(Notice {
                            title: format!("Welcome over from {}", moved.source).into(),
                            body: format!(
                                "{} instances and {} account{} came with you. {} of game files are shared with the original instead of copied.",
                                moved.instances,
                                moved.accounts,
                                if moved.accounts == 1 { "" } else { "s" },
                                calcite_core::transfer::human_bytes(moved.cloned_bytes)
                            )
                            .into(),
                            is_error: false,
                        });
                    }
                    this.status = BackendStatus::Ready;
                    this.refresh_all(cx);
                    this.refresh_modrinth(cx);
                    cx.notify();
                }
                Err(error) => {
                    tracing::error!("backend failed to start: {error:#}");
                    this.status = BackendStatus::Failed(format!("{error:#}"));
                    cx.notify();
                }
            });
            if updated.is_err() {
                tracing::warn!("app state was dropped before the backend finished starting");
            }
        })
        .detach();

        let weak = state.downgrade();
        cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            let mut events = events.ready_chunks(256);
            while let Some(batch) = events.next().await {
                if weak
                    .update(cx, |this, cx| {
                        for event in batch {
                            this.handle_event(event, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        state
    }

    fn handle_event(&mut self, event: AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::Instance(payload) => match payload.event {
                InstancePayloadType::Created | InstancePayloadType::Removed => {
                    self.schedule_refresh(cx)
                }
                InstancePayloadType::Synced | InstancePayloadType::Edited => {
                    *self
                        .content_revision
                        .entry(payload.instance_id)
                        .or_default() += 1;
                    self.schedule_refresh(cx);
                }
                InstancePayloadType::ContentInstallFinished { project_ids } => {
                    if let Some(list) = self.installing.get_mut(&payload.instance_id) {
                        list.retain(|p| !project_ids.contains(p));
                    }
                    *self
                        .content_revision
                        .entry(payload.instance_id)
                        .or_default() += 1;
                }
                InstancePayloadType::ContentInstallFailed {
                    project_ids,
                    message,
                } => {
                    if let Some(list) = self.installing.get_mut(&payload.instance_id) {
                        list.retain(|p| !project_ids.contains(p));
                    }
                    let name = self
                        .instance(&payload.instance_id)
                        .map(|i| i.name.to_string())
                        .unwrap_or_default();
                    self.notify_error(format!("Couldn't add that to {name}"), message);
                }
                _ => {}
            },
            AppEvent::InstanceGroupsChanged(_) => self.schedule_refresh(cx),
            // Requests, people coming online, what they play: re-read the list.
            AppEvent::Friend(_) => self.refresh_friends(cx),
            AppEvent::Process(payload) => match payload.event {
                ProcessPayloadType::Launched => {
                    self.logs.remove(&payload.instance_id);
                    self.running.remove(&payload.instance_id);
                    self.mark_running(payload.instance_id, Utc::now(), cx);
                    if cx.global::<crate::prefs::Prefs>().minimize_on_launch {
                        for window in cx.windows() {
                            let _ = window.update(cx, |_, window, _| window.minimize_window());
                        }
                    }
                }
                ProcessPayloadType::Finished => {
                    if cx.global::<crate::prefs::Prefs>().refocus_on_exit && self.running.len() <= 1
                    {
                        cx.activate(true);
                        for window in cx.windows() {
                            let _ = window.update(cx, |_, window, _| window.activate_window());
                        }
                    }
                    let started = self.running.remove(&payload.instance_id).map(|r| r.started);
                    if let Some(started) = started {
                        self.explain_crash(payload.instance_id.clone(), started, cx);
                    }
                    if cx.global::<crate::prefs::Prefs>().backup_worlds_on_exit
                        && let Some(started) = started
                    {
                        self.backup_played_worlds(payload.instance_id.clone(), started, cx);
                    }
                    // Playtime and "last played" changed.
                    self.schedule_refresh(cx);
                }
            },
            AppEvent::InstallJob(snapshot) => {
                let job = Job::from_snapshot(&snapshot);
                let finished = !job.is_active();
                let succeeded = job.status == InstallJobStatus::Succeeded;
                match self.jobs.iter_mut().find(|j| j.id == job.id) {
                    Some(existing) => *existing = job.clone(),
                    None => self.jobs.push(job.clone()),
                }
                if finished {
                    if let Some((name, address)) = self.pending_joins.remove(&job.id)
                        && succeeded
                        && let Some(instance_id) = job.instance_id.clone()
                    {
                        self.add_server_and_join(instance_id, name, address, cx);
                    }
                    if let Some((game_dir, mods_dir)) = self.pending_copies.remove(&job.id)
                        && succeeded
                        && let Some(instance_id) = job.instance_id.clone()
                    {
                        self.copy_game_folder(game_dir, mods_dir, instance_id, cx);
                    }
                    self.schedule_refresh(cx);
                    if let Some(error) = job.error.clone().filter(|_| !succeeded) {
                        self.notice = Some(Notice {
                            title: format!("Couldn't finish \u{201c}{}\u{201d}", job.title).into(),
                            body: error.into(),
                            is_error: true,
                        });
                    }
                    // Let a finished job linger briefly so the bar reaches 100%.
                    let id = job.id.clone();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(4)).await;
                        let _ = this.update(cx, |this, cx| {
                            this.jobs.retain(|j| j.id != id || j.is_active());
                            cx.notify();
                        });
                    })
                    .detach();
                }
            }
            AppEvent::Log(payload) => {
                let line = log_line(&payload.event);
                let buffer = self.logs.entry(payload.instance_id).or_default();
                buffer.push_back(line);
                while buffer.len() > LOG_LINES_KEPT {
                    buffer.pop_front();
                }
            }
            AppEvent::Warning(payload) => {
                self.notice = Some(Notice {
                    title: "Heads up".into(),
                    body: payload.message.into(),
                    is_error: false,
                });
            }
            _ => {}
        }
    }

    /// Record a running game and watch its memory and CPU once a second
    /// until it exits. Does nothing if it's already known.
    fn mark_running(
        &mut self,
        instance_id: String,
        started: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        if self.running.contains_key(&instance_id) {
            return;
        }
        self.running.insert(
            instance_id.clone(),
            Running {
                started,
                usage: VecDeque::with_capacity(USAGE_HISTORY),
            },
        );
        let id = instance_id.clone();
        let dir =
            Backend::global(cx).call(async move { theseus::instance::get_full_path(&id).await });
        cx.spawn(async move |this, cx| {
            let Ok(dir) = dir.await else { return };
            let mut monitor = calcite_core::monitor::GameMonitor::new(&dir);
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let still_running = this
                    .read_with(cx, |this, _| {
                        this.running
                            .get(&instance_id)
                            .is_some_and(|r| r.started == started)
                    })
                    .unwrap_or(false);
                if !still_running {
                    return;
                }
                let (back, sample) = cx
                    .background_spawn(async move {
                        let sample = monitor.sample();
                        (monitor, sample)
                    })
                    .await;
                monitor = back;
                let Some(sample) = sample else { continue };
                let _ = this.update(cx, |this, cx| {
                    if let Some(running) = this.running.get_mut(&instance_id) {
                        if running.usage.len() == USAGE_HISTORY {
                            running.usage.pop_front();
                        }
                        running.usage.push_back(sample);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Look up who Calcite is signed in to Modrinth as.
    pub fn refresh_modrinth(&mut self, cx: &mut Context<Self>) {
        let fetch = Backend::global(cx).call(async {
            let Some(creds) = theseus::mr_auth::get_credentials().await? else {
                return Ok(None);
            };
            // The profile needs the network; fall back to the id offline.
            let profile = theseus::users::get_user_profile(&creds.user_id).await.ok();
            let field = |key: &str| {
                profile
                    .as_ref()
                    .and_then(|p| p.get(key))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            Ok(Some(ModrinthUser {
                username: field("username")
                    .unwrap_or_else(|| creds.user_id.clone())
                    .into(),
                avatar_url: field("avatar_url").filter(|u| !u.is_empty()),
                id: creds.user_id,
            }))
        });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(user) => {
                        this.modrinth = user;
                        this.refresh_friends(cx);
                    }
                    Err(error) => tracing::warn!("reading the Modrinth account failed: {error:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_recent_worlds(&mut self, cx: &mut Context<Self>) {
        use theseus::worlds::DisplayStatus;
        let fetch = Backend::global(cx).call(async {
            theseus::worlds::get_recent_worlds(8, DisplayStatus::Normal | DisplayStatus::Favorite)
                .await
        });
        cx.spawn(async move |this, cx| {
            let Ok(worlds) = fetch.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.recent_worlds = worlds
                    .into_iter()
                    .filter_map(|w| {
                        let value = serde_json::to_value(&w.world).ok()?;
                        let server = value.get("type").and_then(|t| t.as_str()) == Some("server");
                        let key = value
                            .get(if server { "address" } else { "path" })
                            .and_then(|v| v.as_str())?
                            .to_string();
                        Some(RecentWorld {
                            instance_id: w.instance_id,
                            name: w.world.name.clone().into(),
                            key,
                            server,
                            last_played: w.world.last_played,
                            icon: value
                                .get("icon")
                                .and_then(|i| i.as_str())
                                .map(str::to_string),
                        })
                    })
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-read the friends list (coalesced; friend events come in bursts).
    pub fn refresh_friends(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.modrinth.as_ref().map(|u| u.id.clone()) else {
            self.friends.clear();
            return;
        };
        let backend = Backend::global(cx).clone();
        self.friends_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let fetch = backend.call(async move {
                let list = theseus::friends::friends().await?;
                let statuses = theseus::friends::friend_statuses()
                    .await
                    .unwrap_or_default();
                let others: Vec<String> = list
                    .iter()
                    .map(|f| {
                        if f.id == me {
                            f.friend_id.clone()
                        } else {
                            f.id.clone()
                        }
                    })
                    .collect();
                let ids: Vec<&str> = others.iter().map(String::as_str).collect();
                let users = theseus::cache::get_user_many(&ids, None)
                    .await
                    .unwrap_or_default();
                let mut friends: Vec<Friend> = list
                    .into_iter()
                    .zip(others)
                    .map(|(f, other)| {
                        let user = users.iter().find(|u| u.id == other);
                        let status = statuses.iter().find(|s| s.user_id.to_string() == other);
                        Friend {
                            username: user
                                .map(|u| u.username.clone())
                                .unwrap_or_else(|| other.clone())
                                .into(),
                            avatar_url: user.and_then(|u| u.avatar_url.clone()),
                            accepted: f.accepted,
                            incoming: !f.accepted && f.id == me,
                            online: status.is_some(),
                            playing: status.and_then(|s| s.profile_name.clone()).map(Into::into),
                            user_id: other,
                        }
                    })
                    .collect();
                // Requests, then who's playing, who's online, everyone else.
                friends.sort_by_key(|f| {
                    let rank = match (f.accepted, f.online, f.playing.is_some()) {
                        (false, ..) => 0,
                        (true, true, true) => 1,
                        (true, true, false) => 2,
                        (true, false, _) => 3,
                    };
                    (rank, f.username.to_lowercase())
                });
                Ok(friends)
            });
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(friends) => this.friends = friends,
                    Err(error) => tracing::warn!("reading friends failed: {error:#}"),
                }
                cx.notify();
            });
        }));
    }

    /// Dev builds only: sample downloads for screenshots.
    #[cfg(feature = "snapshot")]
    pub fn demo_jobs(&mut self, cx: &mut Context<Self>) {
        let job = |n: u8,
                   title: &str,
                   status: InstallJobStatus,
                   paused: bool,
                   current: u64,
                   error: Option<&str>| Job {
            id: format!("00000000-0000-0000-0000-00000000000{n}"),
            instance_id: None,
            title: title.to_string().into(),
            phase: "Downloading mods",
            current,
            total: 100,
            status,
            error: error.map(str::to_string),
            paused,
            can_pause: true,
            can_cancel: true,
        };
        self.jobs = vec![
            job(
                1,
                "Fabulously Optimized",
                InstallJobStatus::Running,
                false,
                62,
                None,
            ),
            job(
                2,
                "Create: Above and Beyond",
                InstallJobStatus::Running,
                true,
                30,
                None,
            ),
            job(
                3,
                "Better MC",
                InstallJobStatus::Failed,
                false,
                0,
                Some("A download failed: connection reset"),
            ),
        ];
        cx.notify();
    }

    /// Dev builds only: sample friends for screenshots.
    #[cfg(feature = "snapshot")]
    pub fn demo_friends(&mut self, cx: &mut Context<Self>) {
        let friend =
            |name: &str, accepted: bool, incoming: bool, online: bool, playing: Option<&str>| {
                Friend {
                    user_id: format!("demo-{name}"),
                    username: name.to_string().into(),
                    avatar_url: None,
                    accepted,
                    incoming,
                    online,
                    playing: playing.map(|p| p.to_string().into()),
                }
            };
        self.modrinth = Some(ModrinthUser {
            id: "demo-me".into(),
            username: "you".into(),
            avatar_url: None,
        });
        self.friends = vec![
            friend("Mira", false, true, false, None),
            friend("Tobi", true, false, true, Some("Create 1.20")),
            friend("Sasha", true, false, true, None),
            friend("Noah", true, false, false, None),
            friend("Lena", true, false, false, None),
            friend("Ravi", true, false, false, None),
            friend("Ines", true, false, false, None),
        ];
        cx.notify();
    }

    /// Copy an instance (a new one appears as a download).
    pub fn duplicate(&mut self, id: String, cx: &mut Context<Self>) {
        let task =
            Backend::global(cx).call(async move { theseus::install::duplicate_instance(id).await });
        self.track_task(task, "Couldn't duplicate it", cx);
    }

    /// Check and re-download the game, loader, libraries and Java.
    pub fn repair(&mut self, id: String, cx: &mut Context<Self>) {
        let task = Backend::global(cx)
            .call(async move { theseus::install::install_existing_instance(id, true).await });
        self.track_task(task, "Couldn't repair it", cx);
    }

    /// Put the modpack back the way it ships: its own files come back
    /// and anything you added is dropped. A snapshot is taken first.
    pub fn reinstall_modpack(&mut self, id: String, cx: &mut Context<Self>) {
        let snapshots = crate::ui::pages::instance_tabs::snapshots_dir(&id);
        let task = Backend::global(cx).call(async move {
            if let Some(snaps) = snapshots
                && let Ok(dir) = theseus::instance::get_full_path(&id).await
            {
                let _ = tokio::task::spawn_blocking(move || {
                    let result = calcite_core::snapshots::take(
                        &dir,
                        &snaps,
                        "Before re-installing the modpack",
                        true,
                    );
                    calcite_core::snapshots::prune_automatic(&snaps, 10);
                    result
                })
                .await;
            }
            theseus::instance::repair_managed_modrinth(&id).await
        });
        self.track_task(task, "Couldn't re-install the modpack", cx);
    }

    /// Stop following the modpack. The files stay; the instance becomes a
    /// normal one whose Minecraft version and loader can change, but it no
    /// longer gets the pack's updates.
    pub fn unlink_modpack(&mut self, id: String, cx: &mut Context<Self>) {
        let task = Backend::global(cx).call(async move {
            theseus::instance::edit(
                &id,
                theseus::data::EditInstance {
                    link: Some(theseus::data::InstanceLink::Unmanaged),
                    ..Default::default()
                },
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.notice = Some(Notice {
                            title: "Unlinked from the modpack".into(),
                            body: "It's a normal instance now. Its mods stay, but pack updates won't arrive.".into(),
                            is_error: false,
                        })
                    }
                    Err(error) => {
                        this.notify_error("Couldn't unlink the modpack", format!("{error:#}"))
                    }
                }
                this.refresh_all(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn track_task(
        &mut self,
        task: impl Future<Output = anyhow::Result<InstallJobSnapshot>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(job) => this.track_job(job, cx),
                    Err(error) => this.notify_error(failure, format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Put a shortcut that starts this instance on the desktop.
    pub fn make_shortcut(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(inst) = self.instance(&id).cloned() else {
            return;
        };
        let icon = inst.icon_path.clone();
        let made = cx.background_spawn(async move {
            crate::shortcuts::create(&inst.name, &inst.id, icon.as_deref())
        });
        cx.spawn(async move |this, cx| {
            let result = made.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(path) => {
                        this.notice = Some(Notice {
                            title: "Shortcut on your desktop".into(),
                            body: format!(
                                "Open {} to start this instance without opening Calcite first.",
                                path.file_name()
                                    .map(|f| f.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            )
                            .into(),
                            is_error: false,
                        })
                    }
                    Err(error) => {
                        this.notify_error("Couldn't make the shortcut", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Pause, resume, retry, cancel or dismiss a download.
    pub fn job_action(&mut self, job_id: String, action: JobAction, cx: &mut Context<Self>) {
        let Ok(uuid) = uuid::Uuid::parse_str(&job_id) else {
            return;
        };
        if action == JobAction::Dismiss {
            self.jobs.retain(|j| j.id != job_id);
        }
        let task = Backend::global(cx).call(async move {
            match action {
                JobAction::Pause => theseus::install::pause_job(uuid).await.map(|_| ()),
                JobAction::Resume => theseus::install::resume_job(uuid).await.map(|_| ()),
                JobAction::Retry => theseus::install::retry_job(uuid).await.map(|_| ()),
                JobAction::Cancel => theseus::install::cancel_job(uuid).await.map(|_| ()),
                JobAction::Dismiss => theseus::install::dismiss_job(uuid).await,
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notify_error("Couldn't change that download", format!("{error:#}"));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Put instances into a group (`add`) and/or take them out of one.
    pub fn regroup(
        &mut self,
        ids: Vec<String>,
        add: Option<String>,
        remove: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let updates: Vec<theseus::instance::InstanceGroupMembershipUpdate> = ids
            .iter()
            .filter_map(|id| self.instance(id))
            .map(|inst| {
                let mut groups: Vec<String> = inst
                    .group_ids
                    .iter()
                    .filter(|g| Some(*g) != remove.as_ref())
                    .cloned()
                    .collect();
                if let Some(add) = &add
                    && !groups.contains(add)
                {
                    groups.push(add.clone());
                }
                theseus::instance::InstanceGroupMembershipUpdate {
                    instance_id: inst.id.clone(),
                    group_ids: groups,
                }
            })
            .collect();
        let task = Backend::global(cx)
            .call(async move { theseus::instance::set_group_memberships(updates).await });
        self.after_library_change(task, "Couldn't change the groups", cx);
    }

    /// Make a group and put these instances in it.
    pub fn group_into_new(&mut self, name: String, ids: Vec<String>, cx: &mut Context<Self>) {
        let backend = Backend::global(cx).clone();
        cx.spawn(async move |this, cx| {
            let created = backend
                .call(async move { theseus::instance::create_group(name).await })
                .await;
            let _ = this.update(cx, |this, cx| match created {
                Ok(group) => this.regroup(ids, Some(group.id), None, cx),
                Err(error) => {
                    this.notify_error("Couldn't make that group", format!("{error:#}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn delete_group(&mut self, id: String, cx: &mut Context<Self>) {
        let task =
            Backend::global(cx).call(async move { theseus::instance::delete_group(id).await });
        self.after_library_change(task, "Couldn't delete that group", cx);
    }

    /// Delete instances (and everything in their folders).
    pub fn delete_instances(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let task = Backend::global(cx).call(async move {
            for id in ids {
                theseus::instance::remove(&id).await?;
            }
            Ok(())
        });
        self.after_library_change(task, "Couldn't delete everything", cx);
    }

    fn after_library_change(
        &mut self,
        task: impl Future<Output = anyhow::Result<()>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notify_error(failure, format!("{error:#}"));
                }
                this.refresh_all(cx);
            });
        })
        .detach();
    }

    /// Send a friend request, or accept one.
    pub fn add_friend(&mut self, user_id: String, cx: &mut Context<Self>) {
        let task =
            Backend::global(cx).call(async move { theseus::friends::add_friend(&user_id).await });
        self.after_friend_change(task, "Couldn't send that friend request", cx);
    }

    /// Unfriend, decline a request, or cancel one you sent.
    pub fn remove_friend(&mut self, user_id: String, cx: &mut Context<Self>) {
        self.friends.retain(|f| f.user_id != user_id);
        let task = Backend::global(cx)
            .call(async move { theseus::friends::remove_friend(&user_id).await });
        self.after_friend_change(task, "Couldn't change that friendship", cx);
    }

    fn after_friend_change(
        &mut self,
        task: impl Future<Output = anyhow::Result<()>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notify_error(failure, format!("{error:#}"));
                }
                this.refresh_friends(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Coalesce bursts of change events into one refresh.
    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if self.status != BackendStatus::Ready {
            return;
        }
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| this.refresh_all(cx));
        }));
    }

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        let backend = Backend::global(cx).clone();
        let fetch = backend.call(async {
            let instances = theseus::instance::list().await?;
            let groups = theseus::instance::list_groups().await?;
            let users = theseus::minecraft_auth::users().await?;
            let default = theseus::minecraft_auth::get_default_user().await?;
            let processes = theseus::process::get_all().await?;
            let mut accounts = Vec::with_capacity(users.len());
            for user in users {
                let profile = user.maybe_online_profile().await;
                accounts.push(Account {
                    id: profile.id,
                    name: profile.name.clone().into(),
                });
            }
            Ok((instances, groups, accounts, default, processes))
        });
        cx.spawn(async move |this, cx| match fetch.await {
            Ok((instances, groups, accounts, default, processes)) => {
                let _ = this.update(cx, |this, cx| {
                    let mut list: Vec<InstanceInfo> = instances
                        .into_iter()
                        .map(InstanceInfo::from_metadata)
                        .collect();
                    list.sort_by(|a, b| {
                        b.last_played
                            .unwrap_or(b.created)
                            .cmp(&a.last_played.unwrap_or(a.created))
                    });
                    this.instances = list;
                    this.groups = groups.into_iter().map(|g| (g.id, g.name.into())).collect();
                    this.accounts = accounts;
                    this.default_account = default;
                    this.load_face(cx);
                    this.load_popular(cx);
                    for p in processes {
                        this.mark_running(p.instance_id, p.start_time, cx);
                    }
                    this.refresh_recent_worlds(cx);
                    cx.notify();
                });
            }
            Err(error) => tracing::warn!("refreshing state failed: {error:#}"),
        })
        .detach();
    }

    pub fn instance(&self, id: &str) -> Option<&InstanceInfo> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn job_for(&self, instance_id: &str) -> Option<&Job> {
        self.jobs
            .iter()
            .find(|j| j.is_active() && j.instance_id.as_deref() == Some(instance_id))
    }

    /// Fetch the active account's skin and cut out its face (once per
    /// account; again after the skin changes).
    pub fn load_face(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.active_account().map(|a| a.id) else {
            self.face = None;
            return;
        };
        if self.face.as_ref().is_some_and(|(id, _)| *id == account) {
            return;
        }
        let client = cx.global::<crate::ui::pages::discover::Api>().0.clone();
        let task = Backend::global(cx).run(async move {
            let skins = theseus::minecraft_skins::get_available_skins()
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let skin = skins
                .into_iter()
                .find(|s| s.is_equipped)
                .ok_or_else(|| anyhow::anyhow!("no skin equipped"))?;
            let url = skin.texture.to_string();
            let bytes = match url.strip_prefix("data:") {
                Some(data) => {
                    use base64::Engine as _;
                    let encoded = data.split_once(',').map(|(_, b)| b).unwrap_or_default();
                    base64::engine::general_purpose::STANDARD.decode(encoded.trim())?
                }
                None => client.bytes(&url).await?,
            };
            tokio::task::spawn_blocking(move || -> anyhow::Result<image::RgbaImage> {
                let texture = image::load_from_memory(&bytes)?.into_rgba8();
                let skin = calcite_core::skin3d::normalize(&texture)
                    .ok_or_else(|| anyhow::anyhow!("odd skin size"))?;
                Ok(calcite_core::skin3d::face(&skin, 64))
            })
            .await?
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| match result {
                Ok(face) => {
                    this.face = Some((account, crate::images::render_image(face)));
                    cx.notify();
                }
                Err(error) => tracing::debug!("no face for the account: {error:#}"),
            });
        })
        .detach();
    }

    /// Popular modpacks for Home, once per run.
    pub fn load_popular(&mut self, cx: &mut Context<Self>) {
        if self.popular.is_some() || self.popular_loading {
            return;
        }
        self.popular_loading = true;
        let client = cx.global::<crate::ui::pages::discover::Api>().0.clone();
        let task = Backend::global(cx).run(async move {
            let mut query = calcite_core::modrinth::SearchQuery::new(
                calcite_core::modrinth::ProjectType::Modpack,
            );
            query.index = calcite_core::modrinth::SearchIndex::Downloads;
            query.limit = 8;
            client.search(&query).await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.popular_loading = false;
                match result {
                    Ok(results) => {
                        this.popular = Some(results.hits);
                        cx.notify();
                    }
                    Err(error) => tracing::debug!("popular modpacks: {error:#}"),
                }
            });
        })
        .detach();
    }

    /// The face to show for an account, if it's the one loaded.
    pub fn face_for(&self, account: uuid::Uuid) -> Option<gpui_kit::ImageSource> {
        self.face
            .as_ref()
            .filter(|(id, _)| *id == account)
            .map(|(_, image)| gpui_kit::ImageSource::Render(image.clone()))
    }

    pub fn active_account(&self) -> Option<&Account> {
        self.default_account
            .and_then(|id| self.accounts.iter().find(|a| a.id == id))
            .or_else(|| self.accounts.first())
    }

    pub fn notify_error(&mut self, title: impl Into<SharedString>, error: impl std::fmt::Display) {
        self.notice = Some(Notice {
            title: title.into(),
            body: error.to_string().into(),
            is_error: true,
        });
    }

    // ------------------------------------------------------------ actions

    /// Launch an instance. If nobody is signed in yet, say so instead.
    pub fn play(&mut self, instance_id: &str, cx: &mut Context<Self>) {
        self.launch(instance_id, theseus::instance::QuickPlayType::None, cx)
    }

    /// Launch straight into a singleplayer world (Quick Play).
    pub fn play_world(&mut self, instance_id: &str, world_folder: String, cx: &mut Context<Self>) {
        self.launch(
            instance_id,
            theseus::instance::QuickPlayType::Singleplayer(world_folder),
            cx,
        )
    }

    /// Join a server listed on Modrinth. Reuses the instance made for it
    /// earlier, or makes one (plain Minecraft, or the server's modpack),
    /// adds the server to its list and starts the game straight into it.
    pub fn join_listed_server(
        &mut self,
        server: calcite_core::servers::Server,
        cx: &mut Context<Self>,
    ) {
        use calcite_core::servers::Content;
        use theseus::data::InstanceLink;
        let Some(address) = server.address().map(str::to_string) else {
            self.notify_error(
                "Can't join that server",
                format!("{} doesn't list an address.", server.name),
            );
            cx.notify();
            return;
        };
        enum Join {
            Ready(String),
            Installing(Box<InstallJobSnapshot>),
        }
        let (project_id, name, icon) = (
            server.project_id.clone(),
            server.name.clone(),
            server.icon_url.clone(),
        );
        let content = server.content().cloned();
        let task = Backend::global(cx).call(async move {
            let existing = theseus::instance::list()
                .await?
                .into_iter()
                .find(|m| match &m.link {
                    InstanceLink::ServerProject { project_id: p } => *p == project_id,
                    InstanceLink::ServerProjectModpack {
                        server_project_id, ..
                    } => *server_project_id == project_id,
                    _ => false,
                })
                .map(|m| m.instance.id);
            if let Some(id) = existing {
                return Ok(Join::Ready(id));
            }
            let job = match content {
                Some(Content::Modpack {
                    version_id,
                    project_id: pack,
                    ..
                }) => {
                    theseus::install::create_modpack_instance(
                        theseus::pack::install_from::CreatePackLocation::FromVersionId {
                            project_id: pack.clone(),
                            version_id: version_id.clone(),
                            title: name.clone(),
                            icon_url: icon,
                        },
                        Some(theseus::install::InstallPostInstallEdit {
                            name: Some(name.clone()),
                            icon_path: None,
                            link: Some(InstanceLink::ServerProjectModpack {
                                server_project_id: project_id,
                                content_project_id: pack,
                                content_version_id: version_id,
                            }),
                        }),
                    )
                    .await?
                }
                other => {
                    let wanted = match other {
                        Some(Content::Vanilla {
                            recommended_game_version,
                            supported_game_versions,
                        }) => recommended_game_version
                            .or_else(|| supported_game_versions.first().cloned()),
                        _ => None,
                    };
                    let version = match wanted {
                        Some(v) => v,
                        None => theseus::metadata::get_minecraft_versions()
                            .await?
                            .latest
                            .release
                            .as_str()
                            .to_string(),
                    };
                    theseus::install::create_instance(
                        name,
                        version,
                        ModLoader::Vanilla,
                        None,
                        None,
                        None,
                        InstanceLink::ServerProject { project_id },
                    )
                    .await?
                }
            };
            Ok(Join::Installing(Box::new(job)))
        });
        let name = server.name.clone();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Join::Ready(id)) => this.add_server_and_join(id, name, address, cx),
                    Ok(Join::Installing(job)) => {
                        this.pending_joins
                            .insert(job.job_id.clone(), (name, address));
                        this.track_job(*job, cx);
                    }
                    Err(error) => {
                        this.notify_error("Couldn't get that server ready", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Put the server in the instance's list (once) and start the game into it.
    fn add_server_and_join(
        &mut self,
        instance_id: String,
        name: String,
        address: String,
        cx: &mut Context<Self>,
    ) {
        let (id, target) = (instance_id.clone(), address.clone());
        let task = Backend::global(cx).call(async move {
            let listed = theseus::worlds::get_instance_worlds(&id)
                .await
                .unwrap_or_default()
                .into_iter()
                .any(|w| {
                    matches!(&w.details, theseus::worlds::WorldDetails::Server { address, .. }
                        if address.eq_ignore_ascii_case(&target))
                });
            if !listed {
                theseus::worlds::add_server_to_instance(
                    &id,
                    name,
                    target,
                    theseus::worlds::ServerPackStatus::Prompt,
                    None,
                    None,
                )
                .await?;
            }
            Ok(())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    tracing::warn!("couldn't add the server to {instance_id}: {error:#}");
                }
                this.play_server(&instance_id, address, cx);
            });
        })
        .detach();
    }

    /// Start the game and join a server right away.
    pub fn play_server(&mut self, instance_id: &str, address: String, cx: &mut Context<Self>) {
        self.launch(
            instance_id,
            theseus::instance::QuickPlayType::Server(
                theseus::server_address::ServerAddress::Unresolved(address),
            ),
            cx,
        )
    }

    fn launch(
        &mut self,
        instance_id: &str,
        quick_play: theseus::instance::QuickPlayType,
        cx: &mut Context<Self>,
    ) {
        if self.accounts.is_empty() {
            self.notice = Some(Notice {
                title: "Sign in to play".into(),
                body: "Add your Microsoft account from the bottom of the sidebar, then press Play again.".into(),
                is_error: false,
            });
            cx.notify();
            return;
        }
        let id = instance_id.to_string();
        let run =
            Backend::global(cx).call(async move { theseus::instance::run(&id, quick_play).await });
        cx.spawn(async move |this, cx| {
            if let Err(error) = run.await {
                let _ = this.update(cx, |this, cx| {
                    this.notify_error("Minecraft didn't start", format!("{error:#}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn stop(&mut self, instance_id: &str, cx: &mut Context<Self>) {
        let id = instance_id.to_string();
        let kill = Backend::global(cx).call(async move { theseus::instance::kill(&id).await });
        cx.spawn(async move |this, cx| {
            if let Err(error) = kill.await {
                let _ = this.update(cx, |this, cx| {
                    this.notify_error("Couldn't stop the game", format!("{error:#}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn create_instance(
        &mut self,
        name: String,
        loader: ModLoader,
        game_version: String,
        loader_version: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let create = Backend::global(cx).call(async move {
            theseus::install::create_instance(
                name,
                game_version,
                loader,
                loader_version,
                None,
                None,
                theseus::data::InstanceLink::Unmanaged,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = create.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => this.handle_event(AppEvent::InstallJob(snapshot.into()), cx),
                    Err(error) => {
                        this.notify_error("Couldn't create the instance", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn install_modpack(
        &mut self,
        project_id: String,
        version_id: String,
        title: String,
        icon_url: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let install = Backend::global(cx).call(async move {
            theseus::install::create_modpack_instance(
                theseus::pack::install_from::CreatePackLocation::FromVersionId {
                    project_id,
                    version_id,
                    title,
                    icon_url,
                },
                None,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = install.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => this.handle_event(AppEvent::InstallJob(snapshot.into()), cx),
                    Err(error) => {
                        this.notify_error("Couldn't install the modpack", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Install a modpack file (.mrpack) as a new instance.
    pub fn install_modpack_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let install = Backend::global(cx).call(async move {
            theseus::install::create_modpack_instance(
                theseus::pack::install_from::CreatePackLocation::FromFile { path },
                None,
            )
            .await
        });
        self.track_task(install, "Couldn't install that modpack", cx);
    }

    /// Put dropped mods, packs and worlds into an instance.
    pub fn add_dropped(
        &mut self,
        instance_id: String,
        items: Vec<(PathBuf, calcite_core::dropped::Dropped)>,
        cx: &mut Context<Self>,
    ) {
        use calcite_core::dropped::Dropped;
        use theseus::data::ProjectType;
        let name = self
            .instance(&instance_id)
            .map(|i| i.name.to_string())
            .unwrap_or_else(|| "the instance".into());
        let count = items.len();
        let id = instance_id.clone();
        let task = Backend::global(cx).run(async move {
            let mut failed = Vec::new();
            for (path, kind) in items {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let result: anyhow::Result<()> = match kind {
                    Dropped::World => match theseus::instance::get_full_path(&id).await {
                        Ok(dir) => tokio::task::spawn_blocking(move || {
                            calcite_core::dropped::import_world(&path, &dir.join("saves"))
                                .map(|_| ())
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r),
                        Err(e) => Err(anyhow::anyhow!("{e}")),
                    },
                    other => {
                        let project_type = match other {
                            Dropped::ResourcePack => ProjectType::ResourcePack,
                            Dropped::ShaderPack => ProjectType::ShaderPack,
                            Dropped::DataPack => ProjectType::DataPack,
                            _ => ProjectType::Mod,
                        };
                        theseus::instance::add_project_from_path(&id, &path, Some(project_type))
                            .await
                            .map(|_| ())
                            .map_err(|e| anyhow::anyhow!("{e}"))
                    }
                };
                if let Err(error) = result {
                    failed.push(format!("{file}: {error:#}"));
                }
            }
            failed
        });
        cx.spawn(async move |this, cx| {
            let failed = task.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                if failed.is_empty() {
                    this.notice = Some(Notice {
                        title: format!(
                            "Added {count} file{} to {name}",
                            if count == 1 { "" } else { "s" }
                        )
                        .into(),
                        body: "Worlds are in its Worlds tab, everything else in Content.".into(),
                        is_error: false,
                    });
                } else {
                    this.notify_error("Some files couldn't be added", failed.join("\n"));
                }
                *this.content_revision.entry(instance_id).or_default() += 1;
                cx.notify();
            });
        })
        .detach();
    }

    /// Import one instance from Prism, MultiMC, ATLauncher, GDLauncher or
    /// the CurseForge app, using the backend's importers.
    pub fn import_from_launcher(
        &mut self,
        launcher: theseus::pack::import::ImportLauncherType,
        base_path: PathBuf,
        folder: String,
        cx: &mut Context<Self>,
    ) {
        let import = Backend::global(cx).call(async move {
            theseus::install::import_instance(launcher, base_path, folder).await
        });
        cx.spawn(async move |this, cx| {
            let result = import.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => this.handle_event(AppEvent::InstallJob(snapshot.into()), cx),
                    Err(error) => {
                        this.notify_error("Couldn't import that instance", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Recreate an official-launcher installation as an instance, then bring
    /// its worlds, packs and settings over.
    pub fn import_installation(
        &mut self,
        installation: calcite_core::launchers::Installation,
        cx: &mut Context<Self>,
    ) {
        use calcite_core::launchers::Loader;
        let loader = match installation.version.loader {
            Loader::Vanilla => ModLoader::Vanilla,
            Loader::Fabric => ModLoader::Fabric,
            Loader::Quilt => ModLoader::Quilt,
            Loader::Forge => ModLoader::Forge,
            Loader::NeoForge => ModLoader::NeoForge,
        };
        let name = installation.name.clone();
        let game_dir = installation.game_dir.clone();
        let mods_dir = installation.mods_dir.clone();
        let wanted_game = installation.version.game_version.clone();
        let wanted_loader = installation.version.loader_version.clone();
        let create = Backend::global(cx).call(async move {
            // Resolve "latest-release" and check the loader build still exists.
            let game_version = if wanted_game.starts_with("latest-") {
                let catalog = theseus::metadata::get_minecraft_versions().await?;
                if wanted_game == "latest-snapshot" {
                    catalog.latest.snapshot.as_str().to_string()
                } else {
                    catalog.latest.release.as_str().to_string()
                }
            } else {
                wanted_game
            };
            let loader_version = match (crate::versions::manifest_name(loader), wanted_loader) {
                (Some(meta), Some(wanted)) => {
                    let manifest = theseus::metadata::get_loader_versions(meta).await?;
                    let catalog = crate::versions::LoaderCatalog::from_manifest(&manifest);
                    let known = catalog
                        .loaders_for(&game_version)
                        .iter()
                        .any(|l| l.id == wanted);
                    Some(if known { wanted } else { "stable".to_string() })
                }
                (Some(_), None) => Some("stable".to_string()),
                (None, _) => None,
            };
            theseus::install::create_instance(
                name,
                game_version,
                loader,
                loader_version,
                None,
                None,
                theseus::data::InstanceLink::Unmanaged,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = create.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        this.pending_copies
                            .insert(snapshot.job_id.clone(), (game_dir, mods_dir));
                        this.handle_event(AppEvent::InstallJob(snapshot.into()), cx);
                    }
                    Err(error) => {
                        this.notify_error("Couldn't import that installation", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Clone worlds, packs and settings from a game folder into an instance.
    fn copy_game_folder(
        &mut self,
        game_dir: PathBuf,
        mods_dir: Option<PathBuf>,
        instance_id: String,
        cx: &mut Context<Self>,
    ) {
        let copy = Backend::global(cx).run(async move {
            let target = theseus::instance::get_full_path(&instance_id)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            tokio::task::spawn_blocking(move || -> anyhow::Result<u64> {
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let mut files = 0;
                // Lunar and Feather keep added mods outside the game folder.
                if let Some(mods) = &mods_dir {
                    let dst = target.join("mods");
                    std::fs::create_dir_all(&dst)?;
                    for entry in std::fs::read_dir(mods)?.flatten() {
                        let to = dst.join(entry.file_name());
                        if entry.path().extension().is_some_and(|x| x == "jar")
                            && !to.exists()
                            && reflink_copy::reflink_or_copy(entry.path(), &to).is_ok()
                        {
                            files += 1;
                        }
                    }
                }
                for item in calcite_core::launchers::CARRIED_OVER {
                    // A client's own mods folder replaces the game folder's.
                    if item == "mods" && mods_dir.is_some() {
                        continue;
                    }
                    let src = game_dir.join(item);
                    let dst = target.join(item);
                    if src.is_dir() {
                        std::fs::create_dir_all(&dst)?;
                        for entry in std::fs::read_dir(&src)?.flatten() {
                            let to = dst.join(entry.file_name());
                            if to.exists() {
                                continue;
                            }
                            if entry.path().is_dir() {
                                files += calcite_core::clone::clone_tree(
                                    &entry.path(),
                                    &to,
                                    &cancel,
                                    |_| {},
                                )?
                                .files;
                            } else if reflink_copy::reflink_or_copy(entry.path(), &to).is_ok() {
                                files += 1;
                            }
                        }
                    } else if src.is_file()
                        && !dst.exists()
                        && reflink_copy::reflink_or_copy(&src, &dst).is_ok()
                    {
                        files += 1;
                    }
                }
                Ok(files)
            })
            .await?
        });
        cx.spawn(async move |this, cx| {
            let result = copy.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notify_error("Couldn't bring your worlds over", format!("{error:#}"));
                }
                this.schedule_refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Add a project (and whatever it depends on) to an instance. With no
    /// version given, the backend picks the newest one that fits.
    pub fn install_content(
        &mut self,
        instance_id: String,
        project_id: String,
        version_id: Option<String>,
        content_type: theseus::data::ContentType,
        cx: &mut Context<Self>,
    ) {
        self.installing
            .entry(instance_id.clone())
            .or_default()
            .push(project_id.clone());
        cx.notify();
        let id = instance_id.clone();
        let project = project_id.clone();
        let install = Backend::global(cx).call(async move {
            theseus::instance::install_project_with_dependencies(
                &id,
                theseus::instance::InstallProjectWithDependenciesRequest {
                    project_id: project,
                    version_id,
                    content_type,
                    selected: Default::default(),
                },
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = install.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    if let Some(list) = this.installing.get_mut(&instance_id) {
                        list.retain(|p| p != &project_id);
                    }
                    this.notify_error("Couldn't add that", format!("{error:#}"));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// If the game left a crash report this session, say what went wrong.
    fn explain_crash(&mut self, instance_id: String, since: DateTime<Utc>, cx: &mut Context<Self>) {
        let name = self
            .instance(&instance_id)
            .map(|i| i.name.to_string())
            .unwrap_or_default();
        let task = Backend::global(cx).run(async move {
            let dir = theseus::instance::get_full_path(&instance_id).await.ok()?;
            tokio::task::spawn_blocking(move || {
                let (time, path) = std::fs::read_dir(dir.join("crash-reports"))
                    .ok()?
                    .flatten()
                    .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                    .max_by_key(|(t, _)| *t)?;
                (DateTime::<Utc>::from(time) >= since - chrono::Duration::seconds(5))
                    .then(|| std::fs::read_to_string(path).ok())
                    .flatten()
                    .map(|text| calcite_core::crash::analyze(&text))
            })
            .await
            .ok()
            .flatten()
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Some(d)) = task.await {
                let _ = this.update(cx, |this, cx| {
                    let harmless = d.cause == calcite_core::crash::Cause::HungWhileClosing;
                    this.notice = Some(Notice {
                        title: if harmless {
                            d.title.clone().into()
                        } else {
                            format!("{name} crashed: {}", d.title).into()
                        },
                        body: format!("{} Open the instance's Health tab for details.", d.advice)
                            .into(),
                        is_error: !harmless,
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Back up every singleplayer world played since `since`, keeping the
    /// newest five backups of each.
    fn backup_played_worlds(
        &mut self,
        instance_id: String,
        since: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        let task = Backend::global(cx).run(async move {
            let dir = theseus::instance::get_full_path(&instance_id).await.ok()?;
            let worlds = theseus::worlds::get_instance_worlds(&instance_id)
                .await
                .ok()?;
            let mut names = Vec::new();
            for world in worlds {
                let value = serde_json::to_value(&world).unwrap_or_default();
                let Some(folder) = value
                    .get("path")
                    .and_then(|p| p.as_str())
                    .map(str::to_string)
                else {
                    continue; // servers
                };
                // Allow a minute of clock slack between the game and Calcite.
                if world
                    .last_played
                    .is_none_or(|t| t < since - chrono::Duration::minutes(1))
                {
                    continue;
                }
                if theseus::worlds::backup_world(&dir, &folder).await.is_ok() {
                    calcite_core::backups::prune_world_backups(&dir.join("backups"), &folder, 5);
                    names.push(world.name.clone());
                }
            }
            Some(names)
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Some(names)) = task.await
                && !names.is_empty()
            {
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(Notice {
                        title: if names.len() == 1 {
                            format!("Backed up {}", names[0]).into()
                        } else {
                            format!("Backed up {} worlds", names.len()).into()
                        },
                        body: "Saved in the instance's backups folder. Calcite keeps the last five of each world.".into(),
                        is_error: false,
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Follow an install job the UI started (progress card, completion).
    pub fn track_job(&mut self, snapshot: InstallJobSnapshot, cx: &mut Context<Self>) {
        self.handle_event(AppEvent::InstallJob(snapshot.into()), cx);
    }

    pub fn is_installing(&self, instance_id: &str, project_id: &str) -> bool {
        self.installing
            .get(instance_id)
            .is_some_and(|l| l.iter().any(|p| p == project_id))
    }

    pub fn set_default_account(&mut self, id: uuid::Uuid, cx: &mut Context<Self>) {
        self.default_account = Some(id);
        cx.notify();
        let set = Backend::global(cx)
            .call(async move { theseus::minecraft_auth::set_default_user(id).await });
        cx.spawn(async move |this, cx| {
            if let Err(error) = set.await {
                let _ = this.update(cx, |this, cx| {
                    this.notify_error("Couldn't switch accounts", error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn remove_account(&mut self, id: uuid::Uuid, cx: &mut Context<Self>) {
        let remove =
            Backend::global(cx).call(async move { theseus::minecraft_auth::remove_user(id).await });
        cx.spawn(async move |this, cx| {
            let result = remove.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notify_error("Couldn't remove the account", error);
                }
                this.refresh_all(cx);
            });
        })
        .detach();
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        cx.notify();
    }
}

fn log_line(event: &theseus::event::LogEvent) -> LogLine {
    match event {
        theseus::event::LogEvent::Log4j(e) => {
            let level = match e
                .level
                .as_deref()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str()
            {
                "WARN" | "WARNING" => LogLevel::Warn,
                "ERROR" | "FATAL" => LogLevel::Error,
                "DEBUG" | "TRACE" => LogLevel::Debug,
                _ => LogLevel::Info,
            };
            let time = e
                .timestamp_millis
                .and_then(DateTime::<Utc>::from_timestamp_millis)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%H:%M:%S")
                        .to_string()
                })
                .unwrap_or_default();
            let mut text = e.message.clone().unwrap_or_default();
            if let Some(thread) = e.thread_name.as_deref()
                && !thread.is_empty()
                && thread != "Render thread"
                && thread != "main"
            {
                text = format!("[{thread}] {text}");
            }
            if let Some(first) = e.throwable.as_deref().and_then(|t| t.lines().next()) {
                text = format!("{text} — {first}");
            }
            LogLine {
                time: time.into(),
                level,
                text: text.into(),
            }
        }
        theseus::event::LogEvent::Legacy { message } => parse_legacy_line(message),
    }
}

/// Plain-text lines look like `[14:02:11] [Render thread/WARN]: message`.
pub fn parse_legacy_line(message: &str) -> LogLine {
    let mut time = String::new();
    let mut level = LogLevel::Info;
    let mut text = message.trim_end().to_string();
    if let Some(rest) = text.strip_prefix('[')
        && let Some((t, rest)) = rest.split_once("] [")
        && let Some((thread_level, msg)) = rest.split_once("]: ")
    {
        time = t.to_string();
        let lvl = thread_level.rsplit('/').next().unwrap_or("");
        level = match lvl {
            "WARN" => LogLevel::Warn,
            "ERROR" | "FATAL" => LogLevel::Error,
            "DEBUG" | "TRACE" => LogLevel::Debug,
            _ => LogLevel::Info,
        };
        text = msg.to_string();
    } else if text.contains("Exception") || text.starts_with("\tat ") {
        level = LogLevel::Error;
    }
    LogLine {
        time: time.into(),
        level,
        text: text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_lines_are_parsed() {
        let line = parse_legacy_line("[14:02:14] [Render thread/WARN]: Missing sound for event");
        assert_eq!(line.time.as_ref(), "14:02:14");
        assert_eq!(line.level, LogLevel::Warn);
        assert_eq!(line.text.as_ref(), "Missing sound for event");

        let plain = parse_legacy_line("java.lang.IllegalStateException: boom");
        assert_eq!(plain.level, LogLevel::Error);
        assert_eq!(plain.time.as_ref(), "");
    }

    #[test]
    fn phases_have_friendly_names() {
        assert_eq!(
            phase_label("downloading_minecraft"),
            "Downloading Minecraft"
        );
        assert_eq!(phase_label("something_new"), "Working");
    }
}
