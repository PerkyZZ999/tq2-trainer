//! Reversible memory patches with verify-before / verify-after writes.

use crate::error::{Result, TrainerError};
use crate::memory::ProcessMemory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchState {
    Original,
    Patched,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct MemoryPatch {
    pub address: usize,
    pub original: Vec<u8>,
    pub replacement: Vec<u8>,
}

impl MemoryPatch {
    pub fn new(address: usize, original: Vec<u8>, replacement: Vec<u8>) -> Result<Self> {
        if original.len() != replacement.len() {
            return Err(TrainerError::Other(
                "patch original/replacement length mismatch".into(),
            ));
        }
        if original.is_empty() {
            return Err(TrainerError::Other("empty patch".into()));
        }
        Ok(Self {
            address,
            original,
            replacement,
        })
    }

    pub fn detect_state(&self, mem: &ProcessMemory<'_>) -> Result<PatchState> {
        let mut cur = vec![0u8; self.original.len()];
        mem.read(self.address, &mut cur)?;
        if cur == self.original {
            Ok(PatchState::Original)
        } else if cur == self.replacement {
            Ok(PatchState::Patched)
        } else {
            Ok(PatchState::Unknown)
        }
    }

    pub fn apply(&self, mem: &ProcessMemory<'_>) -> Result<()> {
        match self.detect_state(mem)? {
            PatchState::Patched => Ok(()),
            PatchState::Original => {
                mem.write_verified(self.address, &self.replacement)?;
                Ok(())
            }
            PatchState::Unknown => Err(TrainerError::Other(format!(
                "Patch location 0x{:x} bytes do not match expected original or patched states. Refusing to write.",
                self.address
            ))),
        }
    }

    pub fn restore(&self, mem: &ProcessMemory<'_>) -> Result<()> {
        match self.detect_state(mem)? {
            PatchState::Original => Ok(()),
            PatchState::Patched => {
                mem.write_verified(self.address, &self.original)?;
                Ok(())
            }
            PatchState::Unknown => Err(TrainerError::Other(format!(
                "Patch location 0x{:x} bytes do not match expected original or patched states. Refusing to write.",
                self.address
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Patch logic length checks only; process I/O covered elsewhere.
    #[test]
    fn rejects_length_mismatch() {
        assert!(MemoryPatch::new(0, vec![1, 2], vec![1]).is_err());
    }
}
