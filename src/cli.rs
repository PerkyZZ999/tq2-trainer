//! Command-line interface.

use clap::{Parser, Subcommand, ValueEnum};

/// Linux-native Titan Quest II trainer (EXP multiplier + gold helpers).
///
/// Single-player / offline personal use only. Game updates may invalidate
/// signatures. The tool refuses to patch unknown builds.
#[derive(Debug, Parser)]
#[command(name = "tq2-trainer")]
#[command(
    version,
    about = "Linux-native Titan Quest II trainer (EXP + gold; single-player / offline)",
    long_about = None
)]
pub struct Cli {
    /// Enable verbose diagnostics (module bases, mapping detail, etc.).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Skip Shipping.exe SHA-256 build check (unsafe; for research only).
    #[arg(long, global = true)]
    pub force: bool,

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

    /// EXPERIMENTAL / UNSAFE: one-shot gold via GetGold trampoline (hidden; can crash).
    /// Prefer `sell-gold`. Requires `--unsafe-grant`.
    #[command(hide = true)]
    Gold {
        /// Amount of gold to add (must be positive).
        amount: i64,

        /// Exact on-screen gold right now (needed to detect success / restore).
        #[arg(long)]
        current: Option<i64>,

        /// Seconds to wait for GetGold to run and the balance to update.
        #[arg(long, default_value_t = 300)]
        timeout: u64,

        /// Acknowledge that this path crashed in live testing and may crash again.
        #[arg(long)]
        unsafe_grant: bool,
    },

    /// Force the gold payout of the next sold item, then restore.
    SellGold {
        /// Gold amount the next sale should grant (ignored with `--disarm`).
        amount: Option<i64>,

        /// Exact on-screen gold right now (needed unless `--no-wait` / `--disarm`).
        #[arg(long)]
        current: Option<i64>,

        /// Seconds to wait for the sale (default 300).
        #[arg(long, default_value_t = 300)]
        timeout: u64,

        /// Arm the override and return immediately (does not auto-restore).
        #[arg(long)]
        no_wait: bool,

        /// Remove a previously armed sell-gold override.
        #[arg(long)]
        disarm: bool,
    },

    /// Advanced: collaborative value scanning (snap / narrow / list / probe).
    Research {
        /// Which candidate file set to use (exp vs gold).
        #[arg(long, value_enum, default_value_t = ResearchTarget::Exp)]
        target: ResearchTarget,

        #[command(subcommand)]
        action: ResearchAction,
    },
}

/// Candidate dump set for collaborative research.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ResearchTarget {
    /// Experience value candidates (`research-dumps/exp-candidates.txt`).
    #[default]
    Exp,
    /// Gold / currency value candidates (`research-dumps/gold-candidates.txt`).
    Gold,
}

#[derive(Debug, Subcommand)]
pub enum ResearchAction {
    /// Initial scan: find all i32/i64 addresses equal to the given value.
    Snap {
        /// Exact on-screen value currently shown in-game.
        value: i64,
    },

    /// Filter previous candidates to those that now equal this value.
    Narrow {
        /// New value after it changed in-game.
        value: i64,
    },

    /// Show the current candidate list from the last snap/narrow.
    List,

    /// Guarded write: set the unique candidate to an absolute value (verify readback).
    ///
    /// For gold this only writes a memory mirror — it does **not** grant wallet gold.
    Probe {
        /// Absolute value to write (requires exactly one live candidate).
        value: i64,
    },
}
