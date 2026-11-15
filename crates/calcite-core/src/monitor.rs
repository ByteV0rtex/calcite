//! Live game monitor: memory and CPU of a running instance's Java process,
//! sampled once a second, only while the game runs.

use std::path::{Path, PathBuf};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Resident memory in bytes.
    pub memory: u64,
    /// Share of the whole machine's CPU, 0–100.
    pub cpu: f32,
}

pub struct GameMonitor {
    sys: System,
    pid: Option<Pid>,
    dir: PathBuf,
    cores: f32,
}

impl GameMonitor {
    pub fn new(game_dir: &Path) -> Self {
        GameMonitor {
            sys: System::new(),
            pid: None,
            dir: game_dir
                .canonicalize()
                .unwrap_or_else(|_| game_dir.to_path_buf()),
            cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1) as f32,
        }
    }

    /// Find the game: a Java process running in the instance folder, or
    /// launched with it as `--gameDir`. The newest one wins.
    fn find(&mut self) -> Option<Pid> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cwd(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        let dir = &self.dir;
        self.sys
            .processes()
            .iter()
            .filter(|(_, p)| {
                p.name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains("java")
            })
            .filter(|(_, p)| {
                let in_dir = p.cwd().is_some_and(|cwd| same_path(cwd, dir));
                let game_dir_arg = p
                    .cmd()
                    .windows(2)
                    .any(|w| w[0] == "--gameDir" && same_path(Path::new(&w[1]), dir));
                in_dir || game_dir_arg
            })
            .max_by_key(|(_, p)| p.start_time())
            .map(|(pid, _)| *pid)
    }

    /// One reading. `None` while the game process can't be found (still
    /// starting, or already gone). The first CPU reading is 0.
    pub fn sample(&mut self) -> Option<Sample> {
        if self.pid.is_none() {
            self.pid = self.find();
        }
        let pid = self.pid?;
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let Some(process) = self.sys.process(pid) else {
            self.pid = None;
            return None;
        };
        Some(Sample {
            memory: process.memory(),
            cpu: (process.cpu_usage() / self.cores).clamp(0.0, 100.0),
        })
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().is_ok_and(|a| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nothing_for_a_folder_no_game_runs_in() {
        let dir = std::env::temp_dir().join("calcite-monitor-test-nothing-here");
        std::fs::create_dir_all(&dir).unwrap();
        let mut monitor = GameMonitor::new(&dir);
        assert_eq!(monitor.sample(), None);
    }
}
