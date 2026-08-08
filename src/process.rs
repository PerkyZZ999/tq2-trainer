//! Discover the running Titan Quest II process via `/proc`.

use std::fs;
use std::path::PathBuf;

use crate::error::{CandidateProcess, Result, TrainerError};

/// Substring that uniquely identifies the game shipping binary under Proton.
pub const GAME_EXE_NAME: &str = "TQ2-Win64-Shipping.exe";

/// A validated Titan Quest II process.
#[derive(Debug, Clone)]
pub struct GameProcess {
    pub pid: u32,
    pub cmdline: String,
}

impl GameProcess {
    /// Path to `/proc/<pid>`.
    pub fn proc_dir(&self) -> PathBuf {
        PathBuf::from(format!("/proc/{}", self.pid))
    }

    /// Returns true if the process still exists.
    pub fn is_alive(&self) -> bool {
        self.proc_dir().exists()
    }

    /// Ensure the process still exists, or return [`TrainerError::ProcessGone`].
    pub fn ensure_alive(&self) -> Result<()> {
        if self.is_alive() {
            Ok(())
        } else {
            Err(TrainerError::ProcessGone(self.pid))
        }
    }
}

/// Enumerate `/proc` and locate exactly one Titan Quest II process.
///
/// A match requires a cmdline argument whose path ends with [`GAME_EXE_NAME`]
/// (the Proton/Wine shipping binary). Mentions of that string inside shell
/// scripts or unrelated command lines are ignored.
pub fn find_game_process() -> Result<GameProcess> {
    let mut matches = Vec::new();

    let proc_dir = fs::read_dir("/proc").map_err(|e| TrainerError::io("/proc", e))?;

    for entry in proc_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_name = entry.file_name();
        let pid_str = match file_name.to_str() {
            Some(s) => s,
            None => continue,
        };

        // Only numeric PIDs.
        let pid: u32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let cmdline_path = entry.path().join("cmdline");
        let cmdline_raw = match fs::read(&cmdline_path) {
            Ok(bytes) => bytes,
            Err(_) => continue, // process may have exited
        };

        if !cmdline_runs_game_exe(&cmdline_raw) {
            continue;
        }

        let cmdline = decode_cmdline(&cmdline_raw);
        matches.push(CandidateProcess { pid, cmdline });
    }

    match matches.len() {
        0 => Err(TrainerError::GameNotRunning),
        1 => {
            let c = matches.into_iter().next().expect("len checked");
            Ok(GameProcess {
                pid: c.pid,
                cmdline: c.cmdline,
            })
        }
        _ => Err(TrainerError::MultipleGames(matches)),
    }
}

/// True if any null-separated cmdline argument is the TQ2 shipping executable.
fn cmdline_runs_game_exe(raw: &[u8]) -> bool {
    for part in raw.split(|&b| b == 0) {
        if part.is_empty() {
            continue;
        }
        let arg = String::from_utf8_lossy(part);
        // Wine path: `...\TQ2-Win64-Shipping.exe` or bare exe name.
        if arg == GAME_EXE_NAME
            || arg.ends_with(&format!("\\{GAME_EXE_NAME}"))
            || arg.ends_with(&format!("/{GAME_EXE_NAME}"))
        {
            return true;
        }
    }
    false
}

/// Convert a null-separated `/proc/<pid>/cmdline` blob into a readable string.
fn decode_cmdline(raw: &[u8]) -> String {
    let mut parts = Vec::new();
    for part in raw.split(|&b| b == 0) {
        if part.is_empty() {
            continue;
        }
        parts.push(String::from_utf8_lossy(part).into_owned());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_cmdline_joins_null_separated_args() {
        let raw = b"foo\0bar\0baz\0";
        assert_eq!(decode_cmdline(raw), "foo bar baz");
    }

    #[test]
    fn decode_cmdline_handles_empty() {
        assert_eq!(decode_cmdline(b""), "");
        assert_eq!(decode_cmdline(b"\0\0"), "");
    }

    #[test]
    fn game_exe_name_is_specific() {
        assert!(GAME_EXE_NAME.contains("Shipping"));
        assert!(!GAME_EXE_NAME.eq_ignore_ascii_case("TQ2"));
    }

    #[test]
    fn cmdline_matches_wine_shipping_path() {
        let raw =
            b"S:\\common\\Titan Quest II\\TQ2\\Binaries\\Win64\\TQ2-Win64-Shipping.exe\0TQ2\0";
        assert!(cmdline_runs_game_exe(raw));
    }

    #[test]
    fn cmdline_ignores_shell_script_mentioning_exe() {
        let raw = b"/usr/bin/zsh\0-c\0python check for TQ2-Win64-Shipping.exe in maps\0";
        assert!(!cmdline_runs_game_exe(raw));
    }
}
