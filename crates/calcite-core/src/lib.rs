//! Calcite's UI-independent logic. Everything here is testable without a
//! window and is shared by the app and the command line.

pub mod backups;
pub mod bisect;
pub mod clone;
pub mod crash;
pub mod health;
pub mod launchers;
pub mod markup;
pub mod modrinth;
pub mod modrinth_auth;
pub mod monitor;
pub mod motd;
pub mod paste;
pub mod skin3d;
pub mod snapshots;
pub mod storage;
pub mod transfer;

/// Calcite's version, as reported to services and shown in the app.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The User-Agent Calcite sends to Modrinth.
pub fn user_agent() -> String {
    format!(
        "calcite-launcher/{} ({}; based on modrinth/theseus)",
        version(),
        std::env::consts::OS
    )
}
