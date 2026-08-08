//! Command-line interface.

use clap::{Parser, Subcommand};

/// Linux-native Titan Quest II experience multiplier trainer.
///
/// Single-player / offline personal use only. Game updates may invalidate
/// signatures. The tool refuses to patch unknown builds.
#[derive(Debug, Parser)]
#[command(name = "tq2-trainer")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose diagnostics (module bases, mapping detail, etc.).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Show game process, signature support, and current EXP multiplier.
    Status,

    /// List memory mappings for TQ2-Win64-Shipping.exe.
    Scan,

    /// Apply an experience multiplier preset (1 restores original).
    Xp {
        /// Multiplier preset: 1, 2, 3, 5, or 10.
        multiplier: u32,
    },

    /// Restore original EXP behavior (same as `xp 1`).
    Restore,

    /// Advanced: collaborative EXP value scanning (snap / narrow / list).
    Research {
        #[command(subcommand)]
        action: ResearchAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum ResearchAction {
    /// Initial scan: find all i32/i64 addresses equal to the on-screen EXP.
    Snap {
        /// Exact experience value currently shown in-game.
        value: i64,
    },

    /// Filter previous candidates to those that now equal this EXP value.
    Narrow {
        /// New experience value after gaining EXP.
        value: i64,
    },

    /// Show the current candidate list from the last snap/narrow.
    List,
}
