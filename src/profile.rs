//! **Build profile** module — Shipping.exe fingerprint + patch site layout.
//!
//! The bundled `signatures/tq2.toml` is the single source of truth. Adapters:
//! - prod: [`bundled`] (`include_str!`)
//! - tests: [`parse_profile`] on fixture TOML

use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{Result, TrainerError};
use crate::scanner::{PatternByte, parse_pattern};

const BUNDLED_TOML: &str = include_str!("../signatures/tq2.toml");

static BUNDLED: OnceLock<BuildProfile> = OnceLock::new();

/// Full researched build: SHA-256 + EXP / gold site layouts.
#[derive(Debug, Clone)]
pub struct BuildProfile {
    pub executable_sha256: String,
    pub exp: ExpLayout,
    pub gold: GoldLayout,
}

#[derive(Debug, Clone)]
pub struct ExpLayout {
    #[allow(dead_code)] // retained for status / tooling
    pub id: String,
    pub addxp_rva: usize,
    pub entry_patch_offset: usize,
    pub continue_offset: usize,
    pub cave_offset: usize,
    pub prologue: String,
    pub entry_original: Vec<u8>,
    pub supported_multipliers: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct GoldLayout {
    #[allow(dead_code)] // retained for status / tooling
    pub id: String,
    pub sell_payout_rva: usize,
    pub sell_payout_original: Vec<u8>,
    pub sell_cave_rva: usize,
    pub sell_cave_len: usize,
    pub getgold_rva: usize,
    pub getgold_hook_offset: usize,
    pub getgold_hook_original: Vec<u8>,
    pub gold_cave_rva: usize,
    pub construct_rva: usize,
    pub validate_rva: usize,
    pub container_add_rva: usize,
    pub sum_helper_rva: usize,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    exp_patch: Vec<RawExp>,
    gold_patch: Vec<RawGold>,
}

#[derive(Debug, Deserialize)]
struct RawExp {
    id: String,
    addxp_rva: u64,
    entry_patch_offset: u64,
    continue_offset: u64,
    cave_offset: u64,
    prologue: String,
    entry_original: String,
    supported_multipliers: Vec<u32>,
    executable_sha256: String,
}

#[derive(Debug, Deserialize)]
struct RawGold {
    id: String,
    sell_payout_rva: u64,
    sell_payout_original: String,
    sell_cave_rva: u64,
    sell_cave_len: u64,
    getgold_rva: u64,
    getgold_hook_offset: u64,
    getgold_hook_original: String,
    gold_cave_rva: u64,
    construct_rva: u64,
    validate_rva: u64,
    container_add_rva: u64,
    sum_helper_rva: u64,
    executable_sha256: String,
}

/// Parse a profile document (test / alternate adapter).
pub fn parse_profile(toml_text: &str) -> Result<BuildProfile> {
    let raw: RawFile = toml::from_str(toml_text)
        .map_err(|e| TrainerError::Other(format!("signature profile TOML: {e}")))?;
    let exp_raw = raw
        .exp_patch
        .into_iter()
        .next()
        .ok_or_else(|| TrainerError::Other("signature profile missing [[exp_patch]]".into()))?;
    let gold_raw =
        raw.gold_patch.into_iter().next().ok_or_else(|| {
            TrainerError::Other("signature profile missing [[gold_patch]]".into())
        })?;
    if exp_raw.executable_sha256 != gold_raw.executable_sha256 {
        return Err(TrainerError::Other(
            "exp_patch and gold_patch executable_sha256 disagree".into(),
        ));
    }
    Ok(BuildProfile {
        executable_sha256: exp_raw.executable_sha256.clone(),
        exp: ExpLayout {
            id: exp_raw.id,
            addxp_rva: exp_raw.addxp_rva as usize,
            entry_patch_offset: exp_raw.entry_patch_offset as usize,
            continue_offset: exp_raw.continue_offset as usize,
            cave_offset: exp_raw.cave_offset as usize,
            prologue: exp_raw.prologue,
            entry_original: parse_hex_bytes(&exp_raw.entry_original)?,
            supported_multipliers: exp_raw.supported_multipliers,
        },
        gold: GoldLayout {
            id: gold_raw.id,
            sell_payout_rva: gold_raw.sell_payout_rva as usize,
            sell_payout_original: parse_hex_bytes(&gold_raw.sell_payout_original)?,
            sell_cave_rva: gold_raw.sell_cave_rva as usize,
            sell_cave_len: gold_raw.sell_cave_len as usize,
            getgold_rva: gold_raw.getgold_rva as usize,
            getgold_hook_offset: gold_raw.getgold_hook_offset as usize,
            getgold_hook_original: parse_hex_bytes(&gold_raw.getgold_hook_original)?,
            gold_cave_rva: gold_raw.gold_cave_rva as usize,
            construct_rva: gold_raw.construct_rva as usize,
            validate_rva: gold_raw.validate_rva as usize,
            container_add_rva: gold_raw.container_add_rva as usize,
            sum_helper_rva: gold_raw.sum_helper_rva as usize,
        },
    })
}

/// Bundled profile compiled into the binary (`signatures/tq2.toml`).
pub fn bundled() -> &'static BuildProfile {
    BUNDLED.get_or_init(|| {
        parse_profile(BUNDLED_TOML).unwrap_or_else(|e| {
            panic!("bundled signatures/tq2.toml failed to parse: {e}");
        })
    })
}

pub fn exp_prologue_pattern() -> Result<Vec<PatternByte>> {
    parse_pattern(&bundled().exp.prologue).map_err(TrainerError::Other)
}

fn parse_hex_bytes(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        let b = u8::from_str_radix(tok, 16).map_err(|_| {
            TrainerError::Other(format!("invalid hex byte `{tok}` in signature profile"))
        })?;
        out.push(b);
    }
    if out.is_empty() {
        return Err(TrainerError::Other(
            "empty byte string in signature profile".into(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_profile_parses_and_matches_known_sha() {
        let p = bundled();
        assert_eq!(
            p.executable_sha256,
            "79392aa1ed71e8ea01a77a3b40cc15d2f87a58a645b8a86f95cd361276ed73b0"
        );
        assert_eq!(p.exp.addxp_rva, 0x6B3_A890);
        assert_eq!(
            p.exp.entry_original,
            vec![0x44, 0x8B, 0xFA, 0x48, 0x8B, 0xF1]
        );
        assert_eq!(p.gold.sell_payout_rva, 0x6D_21FE3);
        assert_eq!(p.gold.sell_cave_len, 11);
        assert!(p.exp.supported_multipliers.contains(&10));
    }
}
