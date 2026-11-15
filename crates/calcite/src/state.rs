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
        }
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
    pub default_account: Option<uuid::Uuid>,
    pub logs: HashMap<String, VecDeque<LogLine>>,
    /// A message for the user that hasn't been dismissed yet.
    pub notice: Option<Notice>,
    /// Game folders to bring into an instance once its install job succeeds
    /// (official launcher imports), keyed by job id.
    pending_copies: HashMap<String, (PathBuf, Option<PathBuf>)>,
    /// Bumped whenever an instance's content changes, so open pages reload.
    pub content_revision: HashMap<String, u64>,
    /// Project ids being installed per instance (for "Installing…" buttons).
    pub installing: HashMap<String, Vec<String>>,
    refresh_task: Option<Task<()>>,
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub title: SharedString,
    pub body: SharedString,
    pub is_error: bool,
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
            default_account: None,
            logs: HashMap::new(),
            notice: None,
            pending_copies: HashMap::new(),
            content_revision: HashMap::new(),
            installing: HashMap::new(),
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
            AppEvent::Process(payload) => match payload.event {
                ProcessPayloadType::Launched => {
                    self.logs.remove(&payload.instance_id);
                    self.running.remove(&payload.instance_id);
                    self.mark_running(payload.instance_id, Utc::now(), cx);
                }
                ProcessPayloadType::Finished => {
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
                    for p in processes {
                        this.mark_running(p.instance_id, p.start_time, cx);
                    }
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
