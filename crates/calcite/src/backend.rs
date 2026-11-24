//! The bridge between the GPUI interface and Modrinth's async launcher backend.
//!
//! The backend (`theseus`) is built on tokio. GPUI has its own executor. We
//! keep them apart: backend work runs on a small dedicated tokio runtime, and
//! the UI awaits its `JoinHandle`, which is executor-agnostic. Backend events
//! arrive through a channel and are drained by a single UI task, so nothing in
//! the interface ever polls.

use std::future::Future;
use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use futures::channel::mpsc;
use gpui_kit::{App, Global};
use theseus::AppEvent;

/// Name of the data folder under the platform's app-data directory.
/// Development builds use a separate folder so they never touch real data.
pub fn data_id() -> String {
    if let Ok(id) = std::env::var("CALCITE_DATA_ID")
        && !id.trim().is_empty()
    {
        return id;
    }
    // The 9/IB edition shares Calcite's folder on purpose: a 9/IB instance
    // made in either one is found by the other, never downloaded twice.
    if cfg!(debug_assertions) {
        "CalciteDev".into()
    } else {
        "Calcite".into()
    }
}

/// Calcite's data folder (where the backend keeps its database).
pub fn data_dir() -> Option<std::path::PathBuf> {
    theseus::data::DirectoryInfo::initial_settings_dir_path(&data_id())
}

/// Newest database migration the bundled backend understands.
pub const DB_SCHEMA: &str = env!("CALCITE_DB_SCHEMA");

#[derive(Clone)]
pub struct Backend {
    runtime: Arc<tokio::runtime::Runtime>,
}

impl Global for Backend {}

impl Backend {
    /// Start the runtime and route backend events into a channel that the
    /// UI drains. Must run before the backend state is initialized.
    pub fn start() -> anyhow::Result<(Self, mpsc::UnboundedReceiver<AppEvent>)> {
        theseus::install_crypto_provider();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("calcite-backend")
            .enable_all()
            .build()
            .context("starting the backend runtime")?;

        let (tx, rx) = mpsc::unbounded();
        theseus::EventState::set_native_sink(Box::new(move |event| {
            // The receiver only goes away when the app is quitting.
            let _ = tx.unbounded_send(event);
        }));

        Ok((
            Backend {
                runtime: Arc::new(runtime),
            },
            rx,
        ))
    }

    /// Run `future` on the backend runtime. The returned future can be
    /// awaited from any executor, including GPUI's.
    pub fn run<T, F>(&self, future: F) -> impl Future<Output = anyhow::Result<T>> + use<T, F>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let handle = self.runtime.spawn(future);
        async move {
            handle
                .await
                .map_err(|e| anyhow!("backend task failed: {e}"))
        }
    }

    /// Like [`Backend::run`] for backend calls that return `theseus::Result`.
    pub fn call<T, F>(&self, future: F) -> impl Future<Output = anyhow::Result<T>> + use<T, F>
    where
        T: Send + 'static,
        F: Future<Output = theseus::Result<T>> + Send + 'static,
    {
        let task = self.run(future);
        async move { task.await?.map_err(|e| anyhow!("{e}")) }
    }

    /// Initialize the backend state (database, settings, file watchers).
    /// A transfer staged by the Move-in flow is applied first, before the
    /// database is opened; its summary is returned.
    pub fn init_state(
        &self,
    ) -> impl Future<Output = anyhow::Result<Option<calcite_core::transfer::Manifest>>> + use<>
    {
        let id = data_id();
        let task = self.run(async move {
            let applied = match data_dir() {
                Some(dir) => calcite_core::transfer::apply_staged(&dir)?,
                None => None,
            };
            theseus::EventState::init()
                .await
                .map_err(|e| anyhow!("{e}"))?;
            if let Some(dir) = data_dir() {
                wait_for_store_lock(&dir, std::time::Duration::from_secs(10)).await;
            }
            theseus::State::init(id).await.map_err(|e| {
                let text = e.to_string();
                if text.contains("another Modrinth process") {
                    anyhow!(
                        "Another program is using this folder: most likely a Calcite that is \
                         still closing, or the Modrinth App set to the same folder. Close it, \
                         then try again."
                    )
                } else {
                    anyhow!("{text}")
                }
            })?;
            // Calcite's promise: no telemetry, no ad personalisation.
            if let Ok(mut settings) = theseus::settings::get().await
                && (settings.telemetry || settings.personalized_ads)
            {
                settings.telemetry = false;
                settings.personalized_ads = false;
                let _ = theseus::settings::set(settings).await;
            }
            Ok(applied)
        });
        async move { task.await? }
    }

    pub fn global(cx: &App) -> &Backend {
        cx.global::<Backend>()
    }
}

/// The backend holds `store.lock` for as long as it runs and gives up at
/// once if another process has it. A copy that is still closing lets go
/// within a moment, so wait for that instead of failing.
async fn wait_for_store_lock(dir: &std::path::Path, wait: std::time::Duration) {
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("store.lock"))
    else {
        return;
    };
    let deadline = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                return;
            }
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(_) => return,
        }
    }
}
