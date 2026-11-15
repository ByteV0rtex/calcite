//! Command-line mode, for scripts, shortcuts and testing:
//!
//! ```text
//! calcite list
//! calcite create <name> <vanilla|fabric|quilt|forge|neoforge> <game-version> [loader-version]
//! calcite play <name-or-id>
//! ```

use futures::StreamExt as _;
use theseus::AppEvent;
use theseus::data::ModLoader;
use theseus::event::ProcessPayloadType;
use theseus::install::InstallJobStatus;

use crate::backend::Backend;
use crate::state::{loader_name, parse_legacy_line};

/// Returns `Some(exit code)` if `args` was a command, `None` to open the app.
pub fn run(args: &[String]) -> Option<i32> {
    let command = args.first()?.as_str();
    if !matches!(
        command,
        "list" | "create" | "play" | "move-in" | "help" | "--help"
    ) {
        return None;
    }
    if matches!(command, "help" | "--help") {
        println!(
            "calcite list\ncalcite create <name> <vanilla|fabric|quilt|forge|neoforge> <game-version> [loader-version]\ncalcite play <name-or-id>"
        );
        return Some(0);
    }
    let (backend, mut events) = match Backend::start() {
        Ok(started) => started,
        Err(error) => {
            eprintln!("error: {error:#}");
            return Some(1);
        }
    };
    let args = args.to_vec();
    let result: anyhow::Result<()> = futures::executor::block_on(async move {
        if let Some(moved) = backend.init_state().await? {
            println!(
                "Moved in {} instances from {}.",
                moved.instances, moved.source
            );
        }
        match args[0].as_str() {
            "move-in" => {
                let dir = calcite_core::transfer::modrinth_app_dir()
                    .ok_or_else(|| anyhow::anyhow!("Modrinth App isn't installed"))?;
                let preview = backend
                    .run(async move { calcite_core::transfer::preview_modrinth(&dir).await })
                    .await??;
                println!(
                    "Modrinth App: {} instances, {} accounts, {}",
                    preview.instances,
                    preview.accounts,
                    calcite_core::transfer::human_bytes(preview.size.bytes)
                );
                let calcite_dir =
                    crate::backend::data_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
                let schema: i64 = crate::backend::DB_SCHEMA.parse().unwrap_or(0);
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let manifest = backend
                    .run(async move {
                        calcite_core::transfer::stage_modrinth(
                            &preview,
                            &calcite_dir,
                            schema,
                            cancel,
                            |_| {},
                        )
                        .await
                    })
                    .await??;
                println!(
                    "Staged {} files ({} shared, not copied). Start Calcite to finish.",
                    manifest.files,
                    calcite_core::transfer::human_bytes(manifest.cloned_bytes)
                );
                Ok(())
            }
            "list" => {
                let instances = backend.call(theseus::instance::list()).await?;
                if instances.is_empty() {
                    println!("No instances yet.");
                }
                for meta in instances {
                    let set = &meta.applied_content_set;
                    println!(
                        "{:<32} {:<9} {:<16} {}",
                        meta.instance.name,
                        loader_name(set.loader),
                        set.game_version,
                        meta.instance.id
                    );
                }
                Ok(())
            }
            "create" => {
                let (Some(name), Some(loader), Some(game_version)) =
                    (args.get(1), args.get(2), args.get(3))
                else {
                    anyhow::bail!(
                        "usage: calcite create <name> <loader> <game-version> [loader-version]"
                    );
                };
                let loader = match loader.to_ascii_lowercase().as_str() {
                    "vanilla" => ModLoader::Vanilla,
                    "fabric" => ModLoader::Fabric,
                    "quilt" => ModLoader::Quilt,
                    "forge" => ModLoader::Forge,
                    "neoforge" => ModLoader::NeoForge,
                    other => anyhow::bail!("unknown loader '{other}'"),
                };
                let (name, game_version, loader_version) =
                    (name.clone(), game_version.clone(), args.get(4).cloned());
                let job = backend
                    .call(async move {
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
                    })
                    .await?;
                println!("Installing… (job {})", job.job_id);
                let mut last_phase = String::new();
                while let Some(event) = events.next().await {
                    if let AppEvent::InstallJob(s) = event {
                        if s.job_id != job.job_id {
                            continue;
                        }
                        let phase = crate::state::phase_key(&s.phase);
                        if phase != last_phase {
                            println!("  {phase}");
                            last_phase = phase;
                        }
                        match s.status {
                            InstallJobStatus::Succeeded => {
                                println!("Done: {}", s.instance_id.clone().unwrap_or_default());
                                return Ok(());
                            }
                            InstallJobStatus::Failed
                            | InstallJobStatus::Canceled
                            | InstallJobStatus::Interrupted => {
                                anyhow::bail!(
                                    "install {:?}: {}",
                                    s.status,
                                    s.error
                                        .as_ref()
                                        .and_then(|e| serde_json::to_string(e).ok())
                                        .unwrap_or_default()
                                )
                            }
                            _ => {}
                        }
                    }
                }
                Ok(())
            }
            "play" => {
                let target = args.get(1).cloned().unwrap_or_default();
                let instances = backend.call(theseus::instance::list()).await?;
                let Some(meta) = instances.into_iter().find(|m| {
                    m.instance.id == target || m.instance.name.eq_ignore_ascii_case(&target)
                }) else {
                    anyhow::bail!("no instance named '{target}' (see `calcite list`)");
                };
                let id = meta.instance.id.clone();
                let run_id = id.clone();
                backend
                    .call(async move {
                        theseus::instance::run(&run_id, theseus::instance::QuickPlayType::None)
                            .await
                    })
                    .await?;
                println!("Launched {}", meta.instance.name);
                while let Some(event) = events.next().await {
                    match event {
                        AppEvent::Log(log) if log.instance_id == id => {
                            if let theseus::event::LogEvent::Legacy { message } = &log.event {
                                println!("{}", parse_legacy_line(message).text);
                            } else if let theseus::event::LogEvent::Log4j(e) = &log.event {
                                println!("{}", e.message.clone().unwrap_or_default());
                            }
                        }
                        AppEvent::Process(p)
                            if p.instance_id == id
                                && matches!(p.event, ProcessPayloadType::Finished) =>
                        {
                            println!("Game exited.");
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    });
    match result {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("error: {error:#}");
            Some(1)
        }
    }
}
