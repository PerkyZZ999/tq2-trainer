//! Titan Quest II native EXP trainer (Linux).

mod cli;
mod error;
mod exp;
mod fingerprint;
mod gold;
mod live_patch;
mod maps;
mod memory;
mod patch;
mod process;
mod profile;
mod research;
mod scanner;
mod x86;

use anyhow::Context;
use clap::Parser;

use std::time::Duration;

use crate::cli::{Cli, Commands, ResearchAction, ResearchTarget};
use crate::exp::find_module_base;
use crate::gold::{GoldAddOptions, SellGoldOptions, default_wait_timeout};
use crate::live_patch::LivePatch;
use crate::maps::{format_game_module_summary, read_maps};
use crate::memory::ProcessMemory;
use crate::process::{GAME_EXE_NAME, find_game_process};
use crate::research::{
    CandidateSet, ResearchKind, format_candidates_for, narrow_value, probe_write, snap_value,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => cmd_status(cli.verbose)?,
        Commands::Scan => cmd_scan(cli.verbose)?,
        Commands::Research { target, action } => cmd_research(target, action, cli.verbose)?,
        Commands::Xp { multiplier } => cmd_xp(multiplier, cli.force, cli.verbose)?,
        Commands::Restore => cmd_xp(1, cli.force, cli.verbose)?,
        Commands::Gold {
            amount,
            current,
            timeout,
            unsafe_grant,
        } => cmd_gold(
            amount,
            current,
            timeout,
            unsafe_grant,
            cli.force,
            cli.verbose,
        )?,
        Commands::SellGold {
            amount,
            current,
            timeout,
            no_wait,
            disarm,
        } => cmd_sell_gold(
            amount,
            current,
            timeout,
            no_wait,
            disarm,
            cli.force,
            cli.verbose,
        )?,
    }

    Ok(())
}

fn research_kind(target: ResearchTarget) -> ResearchKind {
    match target {
        ResearchTarget::Exp => ResearchKind::Exp,
        ResearchTarget::Gold => ResearchKind::Gold,
    }
}

fn cmd_status(verbose: bool) -> anyhow::Result<()> {
    let game = find_game_process().context("process discovery")?;

    println!("Titan Quest II found");
    println!("Process: {GAME_EXE_NAME}");
    println!("PID: {}", game.pid);

    if verbose {
        println!("Cmdline: {}", game.cmdline);
    }

    let maps = read_maps(&game).context("reading memory maps")?;
    let module_regions = maps.game_module_regions();
    let exec_regions = maps.game_executable_regions();

    println!("Executable/module mappings: {}", module_regions.len());
    if verbose {
        println!("Executable (r-x) module regions: {}", exec_regions.len());
        print!("{}", format_game_module_summary(&maps));
    }

    let probe_region = module_regions
        .iter()
        .find(|r| r.readable)
        .copied()
        .or_else(|| maps.regions.iter().find(|r| r.readable));

    match probe_region {
        Some(region) => {
            let mem = ProcessMemory::new(&game);
            match mem.probe_access(region.start) {
                Ok(()) => {
                    println!("Memory access: OK");
                    if verbose {
                        println!(
                            "Probe read 16 bytes at 0x{:x} ({})",
                            region.start,
                            region.perms_string()
                        );
                    }
                }
                Err(e) => {
                    println!("Memory access: FAILED");
                    return Err(e.into());
                }
            }
        }
        None => {
            println!("Memory access: SKIPPED (no readable mapping found)");
        }
    }

    let patch = LivePatch::open(&game, &maps, false, verbose);
    match patch.exp_status() {
        Ok((sites, mult)) => {
            println!("Build: supported (AddXP signature matched)");
            if verbose {
                println!("Module base: 0x{:x}", sites.module_base);
                println!("AddXP:       0x{:x}", sites.addxp);
            }
            match mult {
                Some(1) => println!("Current multiplier: 1x (original)"),
                Some(m) => println!("Current multiplier: {m}x"),
                None => println!("Current multiplier: unknown patch state"),
            }
        }
        Err(e) => {
            println!("Build: unsupported / signature miss");
            if verbose {
                println!("Detail: {e}");
            }
            println!("EXP patch: not available");
        }
    }

    match patch.gold_status() {
        Ok((sites, armed)) => {
            println!("Gold sites: supported");
            if armed {
                println!("sell-gold: ARMED (next sale payout overridden)");
            } else {
                println!("sell-gold: idle");
            }
            if verbose {
                println!("Sell payout: 0x{:x}", sites.sell_entry);
                println!("GetGold:     0x{:x}", sites.getgold);
            }
        }
        Err(e) => {
            println!("Gold sites: unsupported / signature miss");
            if verbose {
                println!("Detail: {e}");
            }
        }
    }

    let gold_path = ResearchKind::Gold.path();
    if gold_path.exists() {
        match CandidateSet::load(&gold_path) {
            Ok(set) if set.pid == game.pid => {
                println!(
                    "Gold mirrors: {} candidate(s) for this PID (last value {})",
                    set.hits.len(),
                    set.last_value
                );
            }
            Ok(set) => {
                println!(
                    "Gold mirrors: candidate file is for PID {} (game is {}) — re-snap",
                    set.pid, game.pid
                );
            }
            Err(_) => println!("Gold mirrors: candidate file unreadable"),
        }
    } else {
        println!("Gold mirrors: none yet (optional; used to detect grants)");
    }

    Ok(())
}

fn cmd_scan(verbose: bool) -> anyhow::Result<()> {
    let game = find_game_process().context("process discovery")?;

    println!("Titan Quest II found");
    println!("PID: {}", game.pid);
    println!();

    let maps = read_maps(&game).context("reading memory maps")?;
    print!("{}", format_game_module_summary(&maps));

    let exec = maps.game_executable_regions();
    println!();
    println!(
        "Scan targets (readable+executable module regions): {}",
        exec.len()
    );
    for r in &exec {
        println!(
            "  0x{:016x}-0x{:016x}  {}  {} bytes",
            r.start,
            r.end,
            r.perms_string(),
            r.size()
        );
    }

    if verbose {
        println!();
        println!("Total process mappings: {}", maps.regions.len());
        println!(
            "Named file-backed module regions: {}",
            maps.named_game_module_regions().len()
        );
        if let Ok(base) = find_module_base(&maps) {
            println!("PE header / module base: 0x{base:x}");
        }
    }

    println!();
    println!("Scan complete.");

    Ok(())
}

fn cmd_xp(multiplier: u32, force: bool, verbose: bool) -> anyhow::Result<()> {
    let game = find_game_process().context("process discovery")?;
    let maps = read_maps(&game).context("reading memory maps")?;
    let patch = LivePatch::open(&game, &maps, force, verbose);

    println!("Titan Quest II found");
    println!("PID: {}", game.pid);

    let (sites, before) = patch.exp_status().context("AddXP signature")?;
    println!("Supported build detected");
    if verbose {
        println!("AddXP at 0x{:x}", sites.addxp);
    }

    let before = before.unwrap_or(0);
    let after = patch.apply_exp(multiplier).context("apply EXP patch")?;

    if multiplier == 1 {
        println!("EXP multiplier: {before}x -> 1x (restored)");
    } else {
        println!("EXP multiplier: {before}x -> {after}x");
    }
    println!("Done.");
    Ok(())
}

fn cmd_gold(
    amount: i64,
    current: Option<i64>,
    timeout_secs: u64,
    unsafe_grant: bool,
    force: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    if !unsafe_grant {
        anyhow::bail!(
            "gold is experimental and crashed the game in live testing — prefer `sell-gold`. \
             Re-run with --unsafe-grant only if you accept that risk (save first)."
        );
    }

    let game = find_game_process().context("process discovery")?;
    let maps = read_maps(&game).context("reading memory maps")?;
    let patch = LivePatch::open(&game, &maps, force, verbose);
    let timeout = if timeout_secs == 0 {
        default_wait_timeout()
    } else {
        Duration::from_secs(timeout_secs)
    };

    println!("Titan Quest II found");
    println!("PID: {}", game.pid);
    println!("Arming UNSAFE one-shot gold grant (+{amount})...");
    println!("Open Currencies / inventory (so GetGold runs), then wait.");

    let result = patch
        .apply_gold_unsafe(GoldAddOptions {
            amount,
            current,
            timeout,
            force_build: force,
            verbose,
        })
        .context("add gold")?;

    println!(
        "Gold: {} -> {} (+{})",
        result.before, result.after, result.amount
    );
    println!("Done. Patch restored.");
    Ok(())
}

fn cmd_sell_gold(
    amount: Option<i64>,
    current: Option<i64>,
    timeout_secs: u64,
    no_wait: bool,
    disarm: bool,
    force: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    let game = find_game_process().context("process discovery")?;
    let maps = read_maps(&game).context("reading memory maps")?;
    let patch = LivePatch::open(&game, &maps, force, verbose);

    println!("Titan Quest II found");
    println!("PID: {}", game.pid);

    if disarm {
        patch.disarm_sell_gold().context("disarm sell-gold")?;
        println!("sell-gold disarmed (original payout restored).");
        return Ok(());
    }

    let Some(amount) = amount else {
        anyhow::bail!("sell-gold requires <amount>, or pass --disarm");
    };

    let timeout = if timeout_secs == 0 {
        default_wait_timeout()
    } else {
        Duration::from_secs(timeout_secs)
    };

    if no_wait {
        println!("Arming sell-gold (+{amount}) without waiting...");
    } else {
        println!("Arming sell-gold (+{amount})...");
        println!(
            "Sell exactly one item now. Waiting up to {}s...",
            timeout.as_secs()
        );
    }

    let result = patch
        .apply_sell_gold(SellGoldOptions {
            amount,
            current,
            timeout,
            no_wait,
            force_build: force,
            verbose,
        })
        .context("sell-gold")?;

    if result.armed_only {
        println!(
            "Armed: next sold item pays {} gold. Run `sell-gold --disarm` to cancel.",
            result.amount
        );
    } else {
        println!(
            "Gold: {} -> {} (sale payout forced to +{})",
            result.before.unwrap_or(-1),
            result.after.unwrap_or(-1),
            result.amount
        );
        println!("Done. Patch restored.");
    }
    Ok(())
}

fn cmd_research(
    target: ResearchTarget,
    action: ResearchAction,
    verbose: bool,
) -> anyhow::Result<()> {
    let kind = research_kind(target);
    let path = kind.path();
    let label = kind.label();

    match action {
        ResearchAction::Snap { value } => {
            let game = find_game_process().context("process discovery")?;
            println!("Titan Quest II found (PID {})", game.pid);
            println!(
                "Snapping exact {label} value {value} (i32 + i64, writable private memory)..."
            );
            println!("Stay in-game; do not change {label} until this finishes.");

            let maps = read_maps(&game).context("reading memory maps")?;
            if verbose {
                println!(
                    "Writable private regions to scan: {}",
                    maps.regions
                        .iter()
                        .filter(|r| r.readable && r.writable && r.private && !r.executable)
                        .count()
                );
            }

            let set = snap_value(&game, &maps, value).context("snap scan")?;
            set.save_labeled(&path, label)
                .context("saving candidates")?;
            println!();
            print!("{}", format_candidates_for(&set, label));
            println!("Saved: {}", path.display());
        }
        ResearchAction::Narrow { value } => {
            let game = find_game_process().context("process discovery")?;
            let previous = CandidateSet::load(&path).context("loading previous candidates")?;
            println!("Titan Quest II found (PID {})", game.pid);
            println!(
                "Narrowing {} {label} candidates -> value {value} ...",
                previous.hits.len()
            );

            let set = narrow_value(&game, &previous, value).context("narrow")?;
            set.save_labeled(&path, label)
                .context("saving candidates")?;
            println!();
            print!("{}", format_candidates_for(&set, label));
            println!("Saved: {}", path.display());
        }
        ResearchAction::List => {
            let set = CandidateSet::load(&path).context("loading candidates")?;
            print!("{}", format_candidates_for(&set, label));
            println!("File: {}", path.display());
        }
        ResearchAction::Probe { value } => {
            let game = find_game_process().context("process discovery")?;
            let previous = CandidateSet::load(&path).context("loading previous candidates")?;
            println!("Titan Quest II found (PID {})", game.pid);
            println!(
                "Probe write: unique {label} candidate {} -> {value} ...",
                previous.last_value
            );

            let hit =
                probe_write(&game, &previous, previous.last_value, value).context("probe write")?;
            let updated = CandidateSet {
                pid: game.pid,
                last_value: value,
                hits: vec![hit.clone()],
            };
            updated
                .save_labeled(&path, label)
                .context("saving candidates")?;

            if matches!(kind, ResearchKind::Gold) {
                println!(
                    "Wrote {value} at 0x{:x} ({}) — memory mirror only; Currencies UI will NOT update.",
                    hit.address,
                    hit.width.as_str()
                );
                println!("Use `gold` / `sell-gold` for real wallet grants.");
            } else {
                println!(
                    "Wrote {value} at 0x{:x} ({}) — confirm the in-game {label} UI.",
                    hit.address,
                    hit.width.as_str()
                );
            }
            println!("Saved: {}", path.display());
        }
    }

    Ok(())
}
