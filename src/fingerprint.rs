//! Build fingerprint checks for the supported Shipping.exe.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Result, TrainerError};
use crate::maps::ProcessMaps;
use crate::process::GAME_EXE_NAME;

/// SHA-256 of the researched `TQ2-Win64-Shipping.exe` (see `signatures/tq2.toml`).
pub const EXPECTED_SHIPPING_SHA256: &str =
    "79392aa1ed71e8ea01a77a3b40cc15d2f87a58a645b8a86f95cd361276ed73b0";

/// Resolve the on-disk Linux path for the mapped Shipping.exe, if present.
pub fn shipping_exe_path(maps: &ProcessMaps) -> Result<PathBuf> {
    maps.named_game_module_regions()
        .into_iter()
        .filter_map(|r| r.pathname.clone())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == GAME_EXE_NAME)
        })
        .filter(|p| p.is_file())
        .ok_or_else(|| {
            TrainerError::Other(
                "could not resolve on-disk TQ2-Win64-Shipping.exe path from /proc maps".into(),
            )
        })
}

/// Hex-encoded SHA-256 of a file (streaming).
pub fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|e| TrainerError::io(path, e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| TrainerError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Refuse memory writes unless the on-disk Shipping.exe matches the researched build.
///
/// Pass `force = true` to skip (research / intentional override).
pub fn assert_supported_build(maps: &ProcessMaps, force: bool) -> Result<String> {
    let path = shipping_exe_path(maps)?;
    let hash = sha256_file(&path)?;
    if force {
        return Ok(hash);
    }
    if hash != EXPECTED_SHIPPING_SHA256 {
        return Err(TrainerError::Other(format!(
            "unsupported Titan Quest II build.\n\
             Shipping.exe SHA-256: {hash}\n\
             Expected:             {EXPECTED_SHIPPING_SHA256}\n\
             No memory was modified.\n\
             Re-research signatures after a game update, or pass --force to override (unsafe)."
        )));
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_hash_is_lowercase_hex() {
        assert_eq!(EXPECTED_SHIPPING_SHA256.len(), 64);
        assert!(
            EXPECTED_SHIPPING_SHA256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}
