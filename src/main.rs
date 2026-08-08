//! Titan Quest II native EXP trainer (Linux).

mod cli;
mod error;
mod exp;
mod maps;
mod memory;
mod patch;
mod process;
mod research;
mod scanner;

use anyhow::Context;
use clap::Parser;

use crate::cli::{Cli, Commands, ResearchAction};
use crate::exp::{apply_multiplier, detect_multiplier, find_module_base, locate_addxp};
use crate::maps::{format_game_module_summary, read_maps};
use crate::memory::ProcessMemory;
use crate::process::{GAME_EXE_NAME, find_game_process};
use crate::research::{CandidateSet, format_candidates, narrow_value, snap_value};

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
        Commands::Research { action } => cmd_research(action, cli.verbose)?,
        Commands::Xp { multiplier } => cmd_xp(multiplier, cli.verbose)?,
        Commands::Restore => cmd_xp(1, cli.verbose)?,
    }

    Ok(())
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

    let mem = ProcessMemory::new(&game);
    match locate_addxp(&mem, &maps) {
        Ok(sites) => {
            println!("Build: supported (AddXP signature matched)");
            if verbose {
                println!("Module base: 0x{:x}", sites.module_base);
                println!("AddXP:       0x{:x}", sites.addxp);
            }
            match detect_multiplier(&mem, &sites)? {
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

fn cmd_xp(multiplier: u32, verbose: bool) -> anyhow::Result<()> {
    let game = find_game_process().context("process discovery")?;
    let maps = read_maps(&game).context("reading memory maps")?;
    let mem = ProcessMemory::new(&game);

    println!("Titan Quest II found");
    println!("PID: {}", game.pid);

    let sites = locate_addxp(&mem, &maps).context("AddXP signature")?;
    println!("Supported build detected");
    if verbose {
        println!("AddXP at 0x{:x}", sites.addxp);
    }

    let before = detect_multiplier(&mem, &sites)?.unwrap_or(0);
    let after = apply_multiplier(&game, &maps, multiplier, verbose).context("apply EXP patch")?;

    if multiplier == 1 {
        println!("EXP multiplier: {before}x -> 1x (restored)");
    } else {
        println!("EXP multiplier: {before}x -> {after}x");
    }
    println!("Done.");
    Ok(())
}

fn cmd_research(action: ResearchAction, verbose: bool) -> anyhow::Result<()> {
    let path = CandidateSet::default_path();

    match action {
        ResearchAction::Snap { value } => {
            let game = find_game_process().context("process discovery")?;
            println!("Titan Quest II found (PID {})", game.pid);
            println!("Snapping exact value {value} (i32 + i64, writable private memory)...");
            println!("Stay in-game; do not gain EXP until this finishes.");

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
            set.save(&path).context("saving candidates")?;
            println!();
            print!("{}", format_candidates(&set));
            println!("Saved: {}", path.display());
        }
        ResearchAction::Narrow { value } => {
            let game = find_game_process().context("process discovery")?;
            let previous = CandidateSet::load(&path).context("loading previous candidates")?;
            println!("Titan Quest II found (PID {})", game.pid);
            println!(
                "Narrowing {} candidates -> value {value} ...",
                previous.hits.len()
            );

            let set = narrow_value(&game, &previous, value).context("narrow")?;
            set.save(&path).context("saving candidates")?;
            println!();
            print!("{}", format_candidates(&set));
            println!("Saved: {}", path.display());
        }
        ResearchAction::List => {
            let set = CandidateSet::load(&path).context("loading candidates")?;
            print!("{}", format_candidates(&set));
            println!("File: {}", path.display());
        }
    }

    Ok(())
}
