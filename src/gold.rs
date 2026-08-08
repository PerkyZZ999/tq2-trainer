//! Gold helpers: next-sale payout override (`sell-gold`) and one-shot wallet grant (`gold`).
//!
//! Value-scan writes do **not** update the Currencies UI. Both commands go through the
//! game's inventory grant path (validated 2026-08-08).

use std::thread;
use std::time::Duration;

use crate::balance_watch::wait_for_balance;
use crate::error::{Result, TrainerError};
use crate::exp::find_module_base;
use crate::fingerprint::assert_supported_build;
use crate::maps::ProcessMaps;
use crate::memory::ProcessMemory;
use crate::patch::{MemoryPatch, PatchState};
use crate::process::GameProcess;
use crate::profile::bundled;
use crate::research::{CandidateSet, ResearchKind, ValueHit, filter_live, read_hit, snap_value};
use crate::x86::{call_rel, encode_i32_le, jmp_rel};

fn gold_layout() -> &'static crate::profile::GoldLayout {
    &bundled().gold
}

fn sell_payout_original() -> &'static [u8] {
    gold_layout().sell_payout_original.as_slice()
}

fn sell_cave_len() -> usize {
    gold_layout().sell_cave_len
}

fn getgold_hook_original() -> &'static [u8] {
    gold_layout().getgold_hook_original.as_slice()
}

fn construct_rva() -> usize {
    gold_layout().construct_rva
}

fn validate_rva() -> usize {
    gold_layout().validate_rva
}

fn container_add_rva() -> usize {
    gold_layout().container_add_rva
}

fn sum_helper_rva() -> usize {
    gold_layout().sum_helper_rva
}

const DEFAULT_WAIT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct GoldSites {
    pub module_base: usize,
    pub sell_entry: usize,
    pub sell_cave: usize,
    pub getgold: usize,
    pub getgold_hook: usize,
    pub gold_cave: usize,
}

impl GoldSites {
    pub fn from_module_base(module_base: usize) -> Self {
        let g = gold_layout();
        Self {
            module_base,
            sell_entry: module_base + g.sell_payout_rva,
            sell_cave: module_base + g.sell_cave_rva,
            getgold: module_base + g.getgold_rva,
            getgold_hook: module_base + g.getgold_rva + g.getgold_hook_offset,
            gold_cave: module_base + g.gold_cave_rva,
        }
    }
}

pub fn locate_gold_sites(mem: &ProcessMemory<'_>, maps: &ProcessMaps) -> Result<GoldSites> {
    let base = find_module_base(maps)?;
    let sites = GoldSites::from_module_base(base);

    let sell = mem.read_vec(sites.sell_entry, sell_payout_original().len())?;
    let sell_ok = sell.as_slice() == sell_payout_original()
        || sell[0] == 0xE9
        || sell == inline_sell_patch_bytes(1)
        || looks_like_inline_sell(&sell);
    if !sell_ok {
        return Err(TrainerError::Other(format!(
            "SellItem payout site unexpected bytes at 0x{:x}: {sell:02x?}. Unsupported build?",
            sites.sell_entry
        )));
    }

    let cave = mem.read_vec(sites.sell_cave, sell_cave_len())?;
    let cave_ok = cave.iter().all(|&b| b == 0xCC) || looks_like_sell_cave(&cave);
    if !cave_ok {
        return Err(TrainerError::Other(format!(
            "SellItem cave unexpected at 0x{:x}: {cave:02x?}",
            sites.sell_cave
        )));
    }

    let hook = mem.read_vec(sites.getgold_hook, getgold_hook_original().len())?;
    let hook_ok = hook.as_slice() == getgold_hook_original() || hook[0] == 0xE9;
    if !hook_ok {
        return Err(TrainerError::Other(format!(
            "GetGold hook site unexpected at 0x{:x}: {hook:02x?}",
            sites.getgold_hook
        )));
    }

    Ok(sites)
}

pub fn validate_gold_amount(amount: i64) -> Result<i32> {
    if amount <= 0 {
        return Err(TrainerError::Other("gold amount must be positive".into()));
    }
    i32::try_from(amount)
        .map_err(|_| TrainerError::Other(format!("gold amount {amount} does not fit in i32")))
}

fn inline_sell_patch_bytes(amount: u8) -> [u8; 5] {
    // push imm8 ; pop r9 ; nop
    [0x6A, amount, 0x41, 0x59, 0x90]
}

fn looks_like_inline_sell(bytes: &[u8]) -> bool {
    bytes.len() == 5 && bytes[0] == 0x6A && bytes[2] == 0x41 && bytes[3] == 0x59 && bytes[4] == 0x90
}

fn looks_like_sell_cave(cave: &[u8]) -> bool {
    // mov r9d, imm32 ; jmp back
    cave.len() == sell_cave_len() && cave[0] == 0x41 && cave[1] == 0xB9 && cave[6] == 0xE9
}

/// Build reversible patches that force the next SellItem gold payout to `amount`.
pub fn build_sell_gold_patches(
    sites: &GoldSites,
    amount: i32,
) -> Result<(MemoryPatch, Option<MemoryPatch>)> {
    if amount <= 0 {
        return Err(TrainerError::Other(
            "sell-gold amount must be positive".into(),
        ));
    }

    if let Ok(imm8) = u8::try_from(amount) {
        let entry = MemoryPatch::new(
            sites.sell_entry,
            sell_payout_original().to_vec(),
            inline_sell_patch_bytes(imm8).to_vec(),
        )?;
        return Ok((entry, None));
    }

    // mov r9d, imm32 ; jmp sell_entry+5
    let mut cave = Vec::with_capacity(sell_cave_len());
    cave.extend_from_slice(&[0x41, 0xB9]);
    cave.extend_from_slice(&encode_i32_le(amount));
    cave.extend_from_slice(&jmp_rel(
        sites.sell_cave + 6,
        sites.sell_entry + sell_payout_original().len(),
    )?);
    debug_assert_eq!(cave.len(), sell_cave_len());

    let mut entry = Vec::with_capacity(5);
    entry.extend_from_slice(&jmp_rel(sites.sell_entry, sites.sell_cave)?);

    let entry_patch = MemoryPatch::new(sites.sell_entry, sell_payout_original().to_vec(), entry)?;
    let cave_patch = MemoryPatch::new(sites.sell_cave, vec![0xCC; sell_cave_len()], cave)?;
    Ok((entry_patch, Some(cave_patch)))
}

fn restore_sell_sites(mem: &ProcessMemory<'_>, sites: &GoldSites) -> Result<()> {
    let entry = mem.read_vec(sites.sell_entry, 5)?;
    let cave = mem.read_vec(sites.sell_cave, sell_cave_len())?;

    if entry.as_slice() == sell_payout_original() {
        if cave.iter().all(|&b| b == 0xCC) {
            return Ok(());
        }
        if looks_like_sell_cave(&cave) {
            let patch = MemoryPatch::new(sites.sell_cave, vec![0xCC; sell_cave_len()], cave)?;
            return patch.restore(mem);
        }
        return Err(TrainerError::Other(
            "sell-gold entry looks original but cave is unexpected; refusing to write".into(),
        ));
    }

    if looks_like_inline_sell(&entry) {
        let patch = MemoryPatch::new(sites.sell_entry, sell_payout_original().to_vec(), entry)?;
        return patch.restore(mem);
    }

    if entry[0] == 0xE9 && looks_like_sell_cave(&cave) {
        let entry_patch =
            MemoryPatch::new(sites.sell_entry, sell_payout_original().to_vec(), entry)?;
        let cave_patch = MemoryPatch::new(sites.sell_cave, vec![0xCC; sell_cave_len()], cave)?;
        entry_patch.restore(mem)?;
        cave_patch.restore(mem)?;
        return Ok(());
    }

    Err(TrainerError::Other(
        "sell-gold site is in an unknown state; refusing to write".into(),
    ))
}

/// Whether a sell-gold override is currently armed.
pub fn sell_gold_is_armed(mem: &ProcessMemory<'_>, sites: &GoldSites) -> Result<bool> {
    let entry = mem.read_vec(sites.sell_entry, 5)?;
    Ok(looks_like_inline_sell(&entry) || entry[0] == 0xE9)
}

fn locate_balance_mirrors(
    process: &GameProcess,
    maps: &ProcessMaps,
    current: Option<i64>,
    verbose: bool,
) -> Result<(i64, Vec<ValueHit>)> {
    let path = ResearchKind::Gold.path();

    if let Ok(previous) = CandidateSet::load(&path)
        && previous.pid == process.pid
        && !previous.hits.is_empty()
    {
        let expected = current.unwrap_or(previous.last_value);
        let live = filter_live(process, &previous, expected)?;
        if !live.hits.is_empty() {
            if verbose {
                eprintln!(
                    "using {} live gold mirror(s) at value {expected}",
                    live.hits.len()
                );
            }
            return Ok((expected, live.hits));
        }
        if current.is_none() {
            return Err(TrainerError::Other(
                "gold candidates are stale — pass --current <on-screen gold>".into(),
            ));
        }
    }

    let Some(current) = current else {
        return Err(TrainerError::Other(
            "need --current <on-screen gold> so the trainer can detect when the grant lands".into(),
        ));
    };

    if verbose {
        eprintln!("snapping gold mirrors for {current}...");
    }
    let set = snap_value(process, maps, current)?;
    set.save_labeled(&path, ResearchKind::Gold.label())?;
    if set.hits.is_empty() {
        return Err(TrainerError::Other(format!(
            "no memory locations hold gold value {current}"
        )));
    }
    Ok((current, set.hits))
}

#[derive(Debug, Clone)]
pub struct SellGoldResult {
    pub amount: i32,
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub armed_only: bool,
}

/// Options for [`arm_sell_gold`].
#[derive(Debug, Clone)]
pub struct SellGoldOptions {
    pub amount: i64,
    pub current: Option<i64>,
    pub timeout: Duration,
    pub no_wait: bool,
    pub force_build: bool,
    pub verbose: bool,
}

/// Arm next-sale gold payout. By default waits for the sale, then restores.
pub fn arm_sell_gold(
    process: &GameProcess,
    maps: &ProcessMaps,
    opts: &SellGoldOptions,
) -> Result<SellGoldResult> {
    let amount = validate_gold_amount(opts.amount)?;
    let hash = assert_supported_build(maps, opts.force_build)?;
    let mem = ProcessMemory::new(process);
    let sites = locate_gold_sites(&mem, maps)?;

    if opts.verbose {
        println!("Shipping.exe SHA-256: {hash}");
        println!("Sell payout site: 0x{:x}", sites.sell_entry);
        println!("Sell cave:        0x{:x}", sites.sell_cave);
    }

    // Always start from a clean sell site.
    restore_sell_sites(&mem, &sites)?;

    let (entry_patch, cave_patch) = build_sell_gold_patches(&sites, amount)?;
    if let Some(cave) = &cave_patch {
        cave.apply(&mem)?;
    }
    entry_patch.apply(&mem)?;

    if opts.no_wait {
        return Ok(SellGoldResult {
            amount,
            before: opts.current,
            after: None,
            armed_only: true,
        });
    }

    let (before, hits) = locate_balance_mirrors(process, maps, opts.current, opts.verbose)?;
    let after = before
        .checked_add(i64::from(amount))
        .ok_or_else(|| TrainerError::Other("gold add overflow".into()))?;

    match wait_for_balance(process, before, after, &hits, opts.timeout) {
        Ok(_) => {
            thread::sleep(Duration::from_millis(150));
            restore_sell_sites(&mem, &sites)?;
            let path = ResearchKind::Gold.path();
            let _ = CandidateSet {
                pid: process.pid,
                last_value: after,
                hits: hits
                    .into_iter()
                    .filter(|h| read_hit(&mem, h).ok() == Some(after))
                    .collect(),
            }
            .save_labeled(&path, ResearchKind::Gold.label());
            Ok(SellGoldResult {
                amount,
                before: Some(before),
                after: Some(after),
                armed_only: false,
            })
        }
        Err(e) => {
            if let Err(restore_err) = restore_sell_sites(&mem, &sites) {
                return Err(TrainerError::Other(format!(
                    "{e}\nAlso failed to restore sell-gold patch: {restore_err}\n\
                     Run `tq2-trainer sell-gold --disarm` immediately."
                )));
            }
            Err(e)
        }
    }
}

/// Disarm a previously armed sell-gold override.
pub fn disarm_sell_gold(process: &GameProcess, maps: &ProcessMaps, verbose: bool) -> Result<()> {
    let mem = ProcessMemory::new(process);
    let sites = locate_gold_sites(&mem, maps)?;
    if verbose {
        println!("Sell payout site: 0x{:x}", sites.sell_entry);
    }
    restore_sell_sites(&mem, &sites)
}

/// Build the GetGold one-shot grant cave + entry hook.
///
/// At the hook point `rbx` is the currency component and `rax` is the gold item
/// description. The cave constructs a descriptor, calls container-add once, then
/// continues into the normal GetGold sum helper.
pub fn build_one_shot_gold_patches(
    sites: &GoldSites,
    amount: i32,
) -> Result<(MemoryPatch, MemoryPatch)> {
    if amount <= 0 {
        return Err(TrainerError::Other("gold amount must be positive".into()));
    }

    let base = sites.module_base;
    let construct = base + construct_rva();
    let validate = base + validate_rva();
    let container_add = base + container_add_rva();
    let sum_helper = base + sum_helper_rva();

    // Cave layout (position-independent via rel32 calls/jmps):
    //   push rdi; push rsi; push r12; push r13; push r14
    //   sub rsp, 0xA0
    //   mov r14, rax                  ; gold description
    //   mov r13, [rbx+0x348]          ; owner
    //   test r13, r13 / jz skip
    //   lea rdx, [rsp+0x50]
    //   mov rcx, r14
    //   mov r8, r13
    //   call construct
    //   lea rdx, [rsp+0x40]
    //   mov qword [rsp+0x40], 0
    //   mov byte  [rsp+0x48], 0
    //   lea rcx, [rsp+0x50]
    //   call validate
    //   test al, al / jnz skip_add
    //   lea rdx, [rsp+0x50]
    //   mov r9d, -1
    //   mov dword [rsp+0x20], 0x0C
    //   mov r8d, amount
    //   mov rcx, [r13+0x518]
    //   add rcx, 0x28
    //   call container_add
    // skip_add:
    //   ; resume GetGold
    //   mov rcx, [rbx+0x348]
    //   mov rdx, r14
    //   mov rcx, [rcx+0x518]
    //   add rcx, 0x28
    //   add rsp, 0xA0
    //   pop r14; pop r13; pop r12; pop rsi; pop rdi
    //   add rsp, 0x20                 ; GetGold's own frame
    //   pop rbx
    //   jmp sum_helper
    // skip (owner null): same resume without grant

    let mut cave = Vec::with_capacity(160);
    let cave_addr = sites.gold_cave;

    let push_regs = |cave: &mut Vec<u8>| {
        cave.extend_from_slice(&[0x57, 0x56, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56]); // rdi rsi r12 r13 r14
    };
    push_regs(&mut cave);
    cave.extend_from_slice(&[0x48, 0x81, 0xEC, 0xA0, 0x00, 0x00, 0x00]); // sub rsp, 0xA0
    cave.extend_from_slice(&[0x49, 0x89, 0xC6]); // mov r14, rax
    cave.extend_from_slice(&[0x4C, 0x8B, 0xAB, 0x48, 0x03, 0x00, 0x00]); // mov r13, [rbx+0x348]
    cave.extend_from_slice(&[0x4D, 0x85, 0xED]); // test r13, r13

    // jz skip_grant — patch displacement after we know resume offset
    let jz_at = cave.len();
    cave.extend_from_slice(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00]);

    cave.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x50]); // lea rdx, [rsp+0x50]
    cave.extend_from_slice(&[0x4C, 0x89, 0xF1]); // mov rcx, r14
    cave.extend_from_slice(&[0x4D, 0x89, 0xE8]); // mov r8, r13
    let call_construct_at = cave_addr + cave.len();
    cave.extend_from_slice(&call_rel(call_construct_at, construct)?);

    cave.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x40]); // lea rdx, [rsp+0x40]
    cave.extend_from_slice(&[0x48, 0xC7, 0x44, 0x24, 0x40, 0x00, 0x00, 0x00, 0x00]);
    cave.extend_from_slice(&[0xC6, 0x44, 0x24, 0x48, 0x00]);
    cave.extend_from_slice(&[0x48, 0x8D, 0x4C, 0x24, 0x50]); // lea rcx, [rsp+0x50]
    let call_validate_at = cave_addr + cave.len();
    cave.extend_from_slice(&call_rel(call_validate_at, validate)?);
    cave.extend_from_slice(&[0x84, 0xC0]); // test al, al
    let jnz_skip_add_at = cave.len();
    cave.extend_from_slice(&[0x75, 0x00]); // jnz rel8 placeholder

    cave.extend_from_slice(&[0x48, 0x8D, 0x54, 0x24, 0x50]); // lea rdx, [rsp+0x50]
    cave.extend_from_slice(&[0x41, 0xB9, 0xFF, 0xFF, 0xFF, 0xFF]); // mov r9d, -1
    cave.extend_from_slice(&[0xC7, 0x44, 0x24, 0x20, 0x0C, 0x00, 0x00, 0x00]);
    cave.extend_from_slice(&[0x41, 0xB8]); // mov r8d, imm32
    cave.extend_from_slice(&encode_i32_le(amount));
    cave.extend_from_slice(&[0x49, 0x8B, 0x8D, 0x18, 0x05, 0x00, 0x00]); // mov rcx, [r13+0x518]
    cave.extend_from_slice(&[0x48, 0x83, 0xC1, 0x28]); // add rcx, 0x28
    let call_add_at = cave_addr + cave.len();
    cave.extend_from_slice(&call_rel(call_add_at, container_add)?);

    let skip_add = cave.len();
    let jnz_rel = i8::try_from(skip_add as isize - (jnz_skip_add_at as isize + 2))
        .map_err(|_| TrainerError::Other("jnz skip_add out of rel8 range".into()))?;
    cave[jnz_skip_add_at + 1] = jnz_rel as u8;

    // resume / skip_grant
    let skip_grant = cave.len();
    let jz_rel = i32::try_from(skip_grant as isize - (jz_at as isize + 6))
        .map_err(|_| TrainerError::Other("jz skip_grant out of range".into()))?;
    cave[jz_at + 2..jz_at + 6].copy_from_slice(&encode_i32_le(jz_rel));

    // Original GetGold tail (description still in r14).
    cave.extend_from_slice(&[0x48, 0x8B, 0x8B, 0x48, 0x03, 0x00, 0x00]); // mov rcx, [rbx+0x348]
    cave.extend_from_slice(&[0x4C, 0x89, 0xF2]); // mov rdx, r14
    cave.extend_from_slice(&[0x48, 0x8B, 0x89, 0x18, 0x05, 0x00, 0x00]); // mov rcx, [rcx+0x518]
    cave.extend_from_slice(&[0x48, 0x83, 0xC1, 0x28]); // add rcx, 0x28
    cave.extend_from_slice(&[0x48, 0x81, 0xC4, 0xA0, 0x00, 0x00, 0x00]); // add rsp, 0xA0
    cave.extend_from_slice(&[0x41, 0x5E, 0x41, 0x5D, 0x41, 0x5C, 0x5E, 0x5F]); // pop r14 r13 r12 rsi rdi
    // Finish GetGold frame that was open at the hook point.
    cave.extend_from_slice(&[0x48, 0x83, 0xC4, 0x20]); // add rsp, 0x20
    cave.push(0x5B); // pop rbx
    let jmp_sum_at = cave_addr + cave.len();
    cave.extend_from_slice(&jmp_rel(jmp_sum_at, sum_helper)?);

    // Ensure cave region was INT3 (or already our cave) before apply.
    let cave_len = cave.len();
    if cave_len > 0x200 {
        return Err(TrainerError::Other(format!(
            "gold cave unexpectedly large ({cave_len} bytes)"
        )));
    }

    let mut entry = Vec::with_capacity(7);
    entry.extend_from_slice(&jmp_rel(sites.getgold_hook, sites.gold_cave)?);
    entry.extend_from_slice(&[0x90, 0x90]); // pad to 7 bytes

    let entry_patch =
        MemoryPatch::new(sites.getgold_hook, getgold_hook_original().to_vec(), entry)?;
    let cave_patch = MemoryPatch::new(sites.gold_cave, vec![0xCC; cave_len], cave)?;
    Ok((entry_patch, cave_patch))
}

fn looks_like_gold_cave(cave: &[u8]) -> bool {
    // Starts with push rdi (0x57) from our prologue.
    cave.len() >= 16 && cave[0] == 0x57 && cave[1] == 0x56
}

fn restore_one_shot_sites(
    mem: &ProcessMemory<'_>,
    sites: &GoldSites,
    cave_len: usize,
) -> Result<()> {
    let hook = mem.read_vec(sites.getgold_hook, 7)?;
    let cave = mem.read_vec(sites.gold_cave, cave_len)?;

    if hook.as_slice() == getgold_hook_original() {
        if cave.iter().all(|&b| b == 0xCC) {
            return Ok(());
        }
        if looks_like_gold_cave(&cave) {
            let patch = MemoryPatch::new(sites.gold_cave, vec![0xCC; cave_len], cave)?;
            return patch.restore(mem);
        }
        return Err(TrainerError::Other(
            "GetGold hook looks original but gold cave is unexpected".into(),
        ));
    }

    if hook[0] == 0xE9 && looks_like_gold_cave(&cave) {
        let entry_patch =
            MemoryPatch::new(sites.getgold_hook, getgold_hook_original().to_vec(), hook)?;
        let cave_patch = MemoryPatch::new(sites.gold_cave, vec![0xCC; cave_len], cave)?;
        entry_patch.restore(mem)?;
        cave_patch.restore(mem)?;
        return Ok(());
    }

    Err(TrainerError::Other(
        "one-shot gold site is in an unknown state; refusing to write".into(),
    ))
}

#[derive(Debug, Clone)]
pub struct GoldAddResult {
    pub amount: i32,
    pub before: i64,
    pub after: i64,
}

/// Options for [`add_gold`].
#[derive(Debug, Clone)]
pub struct GoldAddOptions {
    pub amount: i64,
    pub current: Option<i64>,
    pub timeout: Duration,
    pub force_build: bool,
    pub verbose: bool,
}

/// One-shot wallet grant via a temporary GetGold trampoline.
///
/// Open the Currencies / inventory UI (or otherwise cause GetGold to run) while
/// the command waits; the patch auto-restores after the balance updates.
pub fn add_gold(
    process: &GameProcess,
    maps: &ProcessMaps,
    opts: &GoldAddOptions,
) -> Result<GoldAddResult> {
    let amount = validate_gold_amount(opts.amount)?;
    let hash = assert_supported_build(maps, opts.force_build)?;
    let mem = ProcessMemory::new(process);
    let sites = locate_gold_sites(&mem, maps)?;

    let (before, hits) = locate_balance_mirrors(process, maps, opts.current, opts.verbose)?;
    let after = before
        .checked_add(i64::from(amount))
        .ok_or_else(|| TrainerError::Other("gold add overflow".into()))?;

    let (entry_patch, cave_patch) = build_one_shot_gold_patches(&sites, amount)?;
    let cave_len = cave_patch.replacement.len();

    // Ensure clean slate.
    restore_one_shot_sites(&mem, &sites, cave_len)?;

    if opts.verbose {
        println!("Shipping.exe SHA-256: {hash}");
        println!("GetGold hook: 0x{:x}", sites.getgold_hook);
        println!("Gold cave:    0x{:x} ({cave_len} bytes)", sites.gold_cave);
        println!("Target: {before} -> {after}");
    }

    // Verify cave is pristine INT3 before writing.
    let cave_now = mem.read_vec(sites.gold_cave, cave_len)?;
    if !cave_now.iter().all(|&b| b == 0xCC) {
        return Err(TrainerError::Other(format!(
            "gold cave at 0x{:x} is not free INT3 padding",
            sites.gold_cave
        )));
    }

    match entry_patch.detect_state(&mem)? {
        PatchState::Original => {
            cave_patch.apply(&mem)?;
            entry_patch.apply(&mem)?;
        }
        PatchState::Patched => {
            restore_one_shot_sites(&mem, &sites, cave_len)?;
            cave_patch.apply(&mem)?;
            entry_patch.apply(&mem)?;
        }
        PatchState::Unknown => {
            return Err(TrainerError::Other(
                "GetGold hook is in an unknown state; refusing to write".into(),
            ));
        }
    }

    match wait_for_balance(process, before, after, &hits, opts.timeout) {
        Ok(_) => {
            thread::sleep(Duration::from_millis(150));
            restore_one_shot_sites(&mem, &sites, cave_len)?;
            let path = ResearchKind::Gold.path();
            let _ = CandidateSet {
                pid: process.pid,
                last_value: after,
                hits: hits
                    .into_iter()
                    .filter(|h| read_hit(&mem, h).ok() == Some(after))
                    .collect(),
            }
            .save_labeled(&path, ResearchKind::Gold.label());
            Ok(GoldAddResult {
                amount,
                before,
                after,
            })
        }
        Err(e) => {
            if let Err(restore_err) = restore_one_shot_sites(&mem, &sites, cave_len) {
                return Err(TrainerError::Other(format!(
                    "{e}\nAlso failed to restore GetGold patch: {restore_err}"
                )));
            }
            Err(e)
        }
    }
}

pub fn default_wait_timeout() -> Duration {
    DEFAULT_WAIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_positive_amount() {
        assert!(validate_gold_amount(0).is_err());
        assert!(validate_gold_amount(-1).is_err());
        assert_eq!(validate_gold_amount(10).unwrap(), 10);
    }

    #[test]
    fn inline_sell_patch_for_small_amounts() {
        let sites = GoldSites::from_module_base(0x1000_0000);
        let (entry, cave) = build_sell_gold_patches(&sites, 123).unwrap();
        assert!(cave.is_none());
        assert_eq!(entry.replacement, inline_sell_patch_bytes(123));
        assert_eq!(entry.original, sell_payout_original());
    }

    #[test]
    fn trampoline_sell_patch_for_large_amounts() {
        let sites = GoldSites::from_module_base(0x1000_0000);
        let (entry, cave) = build_sell_gold_patches(&sites, 12_345).unwrap();
        let cave = cave.expect("cave required");
        assert_eq!(entry.replacement[0], 0xE9);
        assert_eq!(cave.replacement[0..2], [0x41, 0xB9]);
        assert_eq!(
            i32::from_le_bytes(cave.replacement[2..6].try_into().unwrap()),
            12_345
        );
        assert_eq!(cave.replacement[6], 0xE9);
    }

    #[test]
    fn one_shot_cave_encodes_amount_and_calls() {
        let sites = GoldSites::from_module_base(0x1000_0000);
        let (entry, cave) = build_one_shot_gold_patches(&sites, 50_000).unwrap();
        assert_eq!(entry.replacement[0], 0xE9);
        assert_eq!(entry.replacement.len(), 7);
        assert!(
            cave.replacement
                .windows(4)
                .any(|w| { w == 50_000i32.to_le_bytes() })
        );
        // Contains call opcodes to construct / validate / add.
        assert!(cave.replacement.contains(&0xE8));
        assert_eq!(cave.replacement[0], 0x57); // push rdi
    }
}
