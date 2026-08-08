//! Deep **live patch** module — small interface for EXP and gold process writes.
//!
//! Callers (CLI) go through [`LivePatch`] only. Recipe details stay in `exp` / `gold`.

use crate::error::Result;
use crate::exp::{self, ExpPatchSites};
use crate::gold::{
    self, GoldAddOptions, GoldAddResult, GoldSites, SellGoldOptions, SellGoldResult,
};
use crate::maps::ProcessMaps;
use crate::memory::ProcessMemory;
use crate::process::GameProcess;

/// Session handle for reversible in-process patches.
///
/// Interface surface: open → apply / restore / status. Build fingerprint and
/// trampoline recipes are implementation details behind this seam.
pub struct LivePatch<'a> {
    process: &'a GameProcess,
    maps: &'a ProcessMaps,
    force_build: bool,
    verbose: bool,
}

impl<'a> LivePatch<'a> {
    pub fn open(
        process: &'a GameProcess,
        maps: &'a ProcessMaps,
        force_build: bool,
        verbose: bool,
    ) -> Self {
        Self {
            process,
            maps,
            force_build,
            verbose,
        }
    }

    /// Locate AddXP and report the live multiplier (`1` = original).
    pub fn exp_status(&self) -> Result<(ExpPatchSites, Option<u32>)> {
        let mem = ProcessMemory::new(self.process);
        let sites = exp::locate_addxp(&mem, self.maps)?;
        let mult = exp::detect_multiplier(&mem, &sites)?;
        Ok((sites, mult))
    }

    /// Apply EXP multiplier (`1` restores original).
    pub fn apply_exp(&self, multiplier: u32) -> Result<u32> {
        exp::apply_multiplier(
            self.process,
            self.maps,
            multiplier,
            self.force_build,
            self.verbose,
        )
    }

    /// Locate gold sites and whether sell-gold is currently armed.
    pub fn gold_status(&self) -> Result<(GoldSites, bool)> {
        let mem = ProcessMemory::new(self.process);
        let sites = gold::locate_gold_sites(&mem, self.maps)?;
        let armed = gold::sell_gold_is_armed(&mem, &sites)?;
        Ok((sites, armed))
    }

    /// Arm next-sale gold payout (see [`SellGoldOptions`]).
    pub fn apply_sell_gold(&self, mut opts: SellGoldOptions) -> Result<SellGoldResult> {
        opts.force_build = self.force_build;
        opts.verbose = self.verbose;
        gold::arm_sell_gold(self.process, self.maps, &opts)
    }

    /// Remove a previously armed sell-gold override.
    pub fn disarm_sell_gold(&self) -> Result<()> {
        gold::disarm_sell_gold(self.process, self.maps, self.verbose)
    }

    /// Experimental GetGold one-shot grant (caller must already opt in).
    pub fn apply_gold_unsafe(&self, mut opts: GoldAddOptions) -> Result<GoldAddResult> {
        opts.force_build = self.force_build;
        opts.verbose = self.verbose;
        gold::add_gold(self.process, self.maps, &opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::GameProcess;

    #[test]
    fn open_preserves_force_and_verbose_flags() {
        let process = GameProcess {
            pid: 1,
            cmdline: "dummy".into(),
        };
        let maps = ProcessMaps { regions: vec![] };
        let lp = LivePatch::open(&process, &maps, true, true);
        assert!(lp.force_build);
        assert!(lp.verbose);
        assert_eq!(lp.process.pid, 1);
    }
}
