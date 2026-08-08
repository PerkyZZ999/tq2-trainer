//! Error types for the trainer.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors produced by process discovery, map parsing, or memory access.
#[derive(Debug, Error)]
pub enum TrainerError {
    #[error("Titan Quest II is not running (no process matching TQ2-Win64-Shipping.exe)")]
    GameNotRunning,

    #[error(
        "multiple Titan Quest II processes found; refusing to guess:\n{}",
        format_candidates(.0)
    )]
    MultipleGames(Vec<CandidateProcess>),

    #[error("process {0} disappeared during operation")]
    ProcessGone(u32),

    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse /proc/{pid}/maps line: {line}")]
    MapsParse { pid: u32, line: String },

    #[error(
        "Linux denied access to the game process (EPERM).\n\
         \n\
         The trainer requires permission to inspect the process.\n\
         No memory was changed.\n\
         \n\
         Supported fixes:\n\
           1. Preferred: grant CAP_SYS_PTRACE to the binary:\n\
                sudo setcap cap_sys_ptrace=ep target/release/tq2-trainer\n\
           2. Temporary (dev only): relax Yama ptrace scope for this session:\n\
                sudo sysctl kernel.yama.ptrace_scope=0\n\
         \n\
         The trainer will not modify kernel.yama.ptrace_scope itself."
    )]
    PermissionDenied,

    #[error("memory read at 0x{address:x} failed: {source}")]
    MemoryRead {
        address: usize,
        #[source]
        source: io::Error,
    },

    #[error("memory write at 0x{address:x} failed: {source}")]
    MemoryWrite {
        address: usize,
        #[source]
        source: io::Error,
    },

    #[error(
        "write verification failed at 0x{address:x}: expected {expected:02x?}, read back {actual:02x?}"
    )]
    WriteVerify {
        address: usize,
        expected: Vec<u8>,
        actual: Vec<u8>,
    },

    #[error("{0}")]
    Other(String),
}

/// A process that matched the game executable name during discovery.
#[derive(Debug, Clone)]
pub struct CandidateProcess {
    pub pid: u32,
    pub cmdline: String,
}

fn format_candidates(candidates: &[CandidateProcess]) -> String {
    candidates
        .iter()
        .map(|c| format!("  PID {}: {}", c.pid, c.cmdline))
        .collect::<Vec<_>>()
        .join("\n")
}

impl TrainerError {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, TrainerError>;
