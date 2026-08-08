//! EXP multiplier patch based on the validated AddXP signature.

use crate::error::{Result, TrainerError};
use crate::fingerprint::assert_supported_build;
use crate::maps::ProcessMaps;
use crate::memory::ProcessMemory;
use crate::patch::{MemoryPatch, PatchState};
use crate::process::GameProcess;
use crate::profile::{bundled, exp_prologue_pattern};
use crate::scanner::{scan_pattern, scan_pattern_chunked};
use crate::x86::{encode_i32_le, jmp_rel, rel32};

/// PE-relative RVA of the AddXP native body (from the bundled build profile).
pub fn addxp_rva() -> usize {
    bundled().exp.addxp_rva
}

fn entry_patch_offset() -> usize {
    bundled().exp.entry_patch_offset
}

fn continue_offset() -> usize {
    bundled().exp.continue_offset
}

fn cave_offset() -> usize {
    bundled().exp.cave_offset
}

fn entry_original() -> &'static [u8] {
    bundled().exp.entry_original.as_slice()
}

const CAVE_LEN: usize = 14;

/// Supported v1 presets (also listed in the build profile).
pub fn validate_multiplier(mult: u32) -> Result<u32> {
    if bundled().exp.supported_multipliers.contains(&mult) {
        Ok(mult)
    } else {
        Err(TrainerError::Other(format!(
            "unsupported multiplier {mult}x (supported: {:?})",
            bundled().exp.supported_multipliers
        )))
    }
}

#[derive(Debug, Clone)]
pub struct ExpPatchSites {
    pub module_base: usize,
    pub addxp: usize,
    pub entry: usize,
    pub cave: usize,
    pub continue_at: usize,
}

impl ExpPatchSites {
    pub fn from_module_base(module_base: usize) -> Self {
        Self::from_addxp(module_base, module_base + addxp_rva())
    }

    /// Build patch sites from a discovered AddXP prologue address.
    pub fn from_addxp(module_base: usize, addxp: usize) -> Self {
        Self {
            module_base,
            addxp,
            entry: addxp + entry_patch_offset(),
            cave: addxp + cave_offset(),
            continue_at: addxp + continue_offset(),
        }
    }
}

/// Resolve the Proton-mapped PE image base (named `r--p` header of Shipping.exe).
pub fn find_module_base(maps: &ProcessMaps) -> Result<usize> {
    maps.named_game_module_regions()
        .into_iter()
        .filter(|r| r.readable && !r.writable && !r.executable)
        .map(|r| r.start)
        .min()
        .ok_or_else(|| {
            TrainerError::Other("PE header mapping for TQ2-Win64-Shipping.exe not found".into())
        })
}

fn validate_addxp_surroundings(mem: &ProcessMemory<'_>, sites: &ExpPatchSites) -> Result<()> {
    let cave = mem.read_vec(sites.cave, CAVE_LEN)?;
    let entry = mem.read_vec(sites.entry, 6)?;
    let cave_ok = cave.iter().all(|&b| b == 0xCC) || looks_like_our_cave(&cave);
    let entry_ok = entry.as_slice() == entry_original() || entry[0] == 0xE9;
    if !cave_ok || !entry_ok {
        return Err(TrainerError::Other(format!(
            "AddXP site found at 0x{:x}, but surrounding bytes are unexpected.\n\
             entry={:02x?} cave={:02x?}\n\
             Refusing to write memory.",
            sites.addxp, entry, cave
        )));
    }
    Ok(())
}

fn scan_addxp_in_executable(
    mem: &ProcessMemory<'_>,
    maps: &ProcessMaps,
    pattern: &[crate::scanner::PatternByte],
) -> Result<Vec<usize>> {
    let mut found = Vec::new();
    for region in maps.game_executable_regions() {
        let hits = scan_pattern_chunked(region.size(), pattern, |offset, buf| {
            match mem.read(region.start + offset, buf) {
                Ok(()) => Ok(buf.len()),
                Err(e) => Err(e.to_string()),
            }
        })
        .map_err(TrainerError::Other)?;
        for off in hits {
            found.push(region.start + off);
        }
    }
    Ok(found)
}

/// Locate and validate AddXP in the live process.
pub fn locate_addxp(mem: &ProcessMemory<'_>, maps: &ProcessMaps) -> Result<ExpPatchSites> {
    let base = find_module_base(maps)?;
    let pattern = exp_prologue_pattern()?;
    let prologue_len = pattern.len();

    let expected = ExpPatchSites::from_module_base(base);
    let bytes = mem.read_vec(expected.addxp, prologue_len)?;
    let sites = if scan_pattern(&bytes, &pattern) == [0] {
        expected
    } else {
        let found = scan_addxp_in_executable(mem, maps, &pattern)?;
        match found.as_slice() {
            &[addr] => ExpPatchSites::from_addxp(base, addr),
            [] => {
                return Err(TrainerError::Other(
                    "Known EXP signature was not found.\n\
                     This Titan Quest II build is currently unsupported.\n\
                     No memory was modified."
                        .into(),
                ));
            }
            _ => {
                return Err(TrainerError::Other(format!(
                    "AddXP signature matched {} times; refusing to patch",
                    found.len()
                )));
            }
        }
    };

    validate_addxp_surroundings(mem, &sites)?;
    Ok(sites)
}

fn looks_like_our_cave(cave: &[u8]) -> bool {
    // 6B D2 xx | 44 8B FA | 48 8B F1 | E9 ..
    cave.len() == CAVE_LEN
        && cave[0] == 0x6B
        && cave[1] == 0xD2
        && cave[3..9] == [0x44, 0x8B, 0xFA, 0x48, 0x8B, 0xF1]
        && cave[9] == 0xE9
}

/// Build the entry + cave patches for a multiplier (>=2).
pub fn build_multiplier_patches(
    sites: &ExpPatchSites,
    multiplier: u32,
) -> Result<(MemoryPatch, MemoryPatch)> {
    if !(2..=127).contains(&multiplier) {
        return Err(TrainerError::Other(
            "cave imm8 multiplier out of range".into(),
        ));
    }

    let mut cave = Vec::with_capacity(CAVE_LEN);
    cave.extend_from_slice(&[0x6B, 0xD2, multiplier as u8]); // imul edx, edx, imm8
    cave.extend_from_slice(&[0x44, 0x8B, 0xFA]); // mov r15d, edx
    cave.extend_from_slice(&[0x48, 0x8B, 0xF1]); // mov rsi, rcx
    cave.push(0xE9); // jmp continue
    let cave_jmp_from = sites.cave + 9 + 5;
    cave.extend_from_slice(&encode_i32_le(rel32(cave_jmp_from, sites.continue_at)?));
    debug_assert_eq!(cave.len(), CAVE_LEN);

    let mut entry = Vec::with_capacity(6);
    entry.extend_from_slice(&jmp_rel(sites.entry, sites.cave)?);
    entry.push(0x90); // nop
    debug_assert_eq!(entry.len(), 6);

    let entry_patch = MemoryPatch::new(sites.entry, entry_original().to_vec(), entry)?;
    let cave_patch = MemoryPatch::new(sites.cave, vec![0xCC; CAVE_LEN], cave)?;
    Ok((entry_patch, cave_patch))
}

/// Detect currently applied multiplier, if our patch is present.
pub fn detect_multiplier(mem: &ProcessMemory<'_>, sites: &ExpPatchSites) -> Result<Option<u32>> {
    let entry = mem.read_vec(sites.entry, 6)?;
    if entry.as_slice() == entry_original() {
        return Ok(Some(1));
    }
    if entry[0] != 0xE9 {
        return Ok(None);
    }
    let cave = mem.read_vec(sites.cave, CAVE_LEN)?;
    if !looks_like_our_cave(&cave) {
        return Ok(None);
    }
    Ok(Some(u32::from(cave[2])))
}

/// Restore entry + cave to the original shipping bytes.
fn restore_sites(mem: &ProcessMemory<'_>, sites: &ExpPatchSites) -> Result<u32> {
    let entry = mem.read_vec(sites.entry, 6)?;
    let cave = mem.read_vec(sites.cave, CAVE_LEN)?;
    let cave_clean = cave.iter().all(|&b| b == 0xCC);
    let cave_ours = looks_like_our_cave(&cave);

    if entry.as_slice() == entry_original() {
        if cave_clean {
            return Ok(1);
        }
        if cave_ours {
            let cave_patch = MemoryPatch::new(sites.cave, vec![0xCC; CAVE_LEN], cave)?;
            cave_patch.restore(mem)?;
            return Ok(1);
        }
        return Err(TrainerError::Other(
            "entry looks original but cave bytes are unexpected; refusing to write".into(),
        ));
    }

    if entry[0] == 0xE9 && cave_ours {
        let entry_patch = MemoryPatch::new(sites.entry, entry_original().to_vec(), entry)?;
        let cave_patch = MemoryPatch::new(sites.cave, vec![0xCC; CAVE_LEN], cave)?;
        entry_patch.restore(mem)?;
        cave_patch.restore(mem)?;
        return Ok(1);
    }

    Err(TrainerError::Other(
        "EXP patch site is in an unknown state; refusing to write".into(),
    ))
}

/// Apply multiplier (1 == restore).
pub fn apply_multiplier(
    process: &GameProcess,
    maps: &ProcessMaps,
    multiplier: u32,
    force_build: bool,
    verbose: bool,
) -> Result<u32> {
    let mult = validate_multiplier(multiplier)?;
    let hash = assert_supported_build(maps, force_build)?;
    let mem = ProcessMemory::new(process);
    let sites = locate_addxp(&mem, maps)?;

    if verbose {
        println!("Shipping.exe SHA-256: {hash}");
        println!("Module base: 0x{:x}", sites.module_base);
        println!("AddXP:       0x{:x}", sites.addxp);
        println!("Entry patch: 0x{:x}", sites.entry);
        println!("Cave:        0x{:x}", sites.cave);
    }

    if mult == 1 {
        return restore_sites(&mem, &sites);
    }

    let previous = detect_multiplier(&mem, &sites)?.unwrap_or(0);
    let (entry_patch, cave_patch) = build_multiplier_patches(&sites, mult)?;

    match entry_patch.detect_state(&mem)? {
        PatchState::Original => {
            cave_patch.apply(&mem)?;
            entry_patch.apply(&mem)?;
        }
        PatchState::Patched => {
            let current = detect_multiplier(&mem, &sites)?;
            if current == Some(mult) {
                return Ok(mult);
            }
            restore_sites(&mem, &sites)?;
            cave_patch.apply(&mem)?;
            entry_patch.apply(&mem)?;
        }
        PatchState::Unknown => {
            return Err(TrainerError::Other(
                "EXP patch site is in an unknown state; refusing to write".into(),
            ));
        }
    }

    let now = detect_multiplier(&mem, &sites)?;
    if now != Some(mult) {
        return Err(TrainerError::Other(format!(
            "patch applied but detector reads {now:?}, expected Some({mult})"
        )));
    }

    if verbose {
        println!("Previous multiplier: {previous}x");
    }

    Ok(mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_presets() {
        assert_eq!(validate_multiplier(10).unwrap(), 10);
        assert!(validate_multiplier(7).is_err());
    }

    #[test]
    fn cave_and_entry_lengths() {
        let sites = ExpPatchSites::from_module_base(0x1000_0000);
        let (entry, cave) = build_multiplier_patches(&sites, 10).unwrap();
        assert_eq!(entry.original.len(), 6);
        assert_eq!(entry.replacement.len(), 6);
        assert_eq!(cave.original.len(), 14);
        assert_eq!(cave.replacement.len(), 14);
        assert_eq!(cave.replacement[0..3], [0x6B, 0xD2, 10]);
        assert_eq!(entry.replacement[0], 0xE9);
        assert_eq!(entry.replacement[5], 0x90);
    }

    #[test]
    fn from_addxp_uses_discovered_address_not_stale_rva() {
        let base = 0x1000_0000;
        let moved = base + addxp_rva() + 0x1000;
        let sites = ExpPatchSites::from_addxp(base, moved);
        assert_eq!(sites.module_base, base);
        assert_eq!(sites.addxp, moved);
        assert_eq!(sites.entry, moved + entry_patch_offset());
        assert_eq!(sites.cave, moved + cave_offset());
    }
}
