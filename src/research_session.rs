//! **Research session** module — snap / narrow / list / probe behind one interface.
//!
//! CLI is a thin adapter; all collaborative value-scan flow lives here.

use anyhow::Context;

use crate::cli::{ResearchAction, ResearchTarget};
use crate::maps::read_maps;
use crate::process::find_game_process;
use crate::research::{
    CandidateSet, ResearchKind, format_candidates_for, narrow_value, probe_write, snap_value,
};

fn kind_from_target(target: ResearchTarget) -> ResearchKind {
    match target {
        ResearchTarget::Exp => ResearchKind::Exp,
        ResearchTarget::Gold => ResearchKind::Gold,
    }
}

/// Run one research action to completion (prints progress to stdout).
pub fn run(target: ResearchTarget, action: ResearchAction, verbose: bool) -> anyhow::Result<()> {
    let kind = kind_from_target(target);
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
                println!("Use `sell-gold` for real wallet grants.");
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
