//! Small x86-64 encoding helpers shared by EXP / gold trampolines.

use crate::error::{Result, TrainerError};

pub fn encode_i32_le(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}

pub fn rel32(from_next_ip: usize, to: usize) -> Result<i32> {
    let delta = to as isize - from_next_ip as isize;
    i32::try_from(delta).map_err(|_| {
        TrainerError::Other(format!(
            "rel32 out of range: from_next=0x{from_next_ip:x} to=0x{to:x}"
        ))
    })
}

pub fn jmp_rel(from: usize, target: usize) -> Result<[u8; 5]> {
    let mut out = [0u8; 5];
    out[0] = 0xE9;
    out[1..].copy_from_slice(&encode_i32_le(rel32(from + 5, target)?));
    Ok(out)
}

pub fn call_rel(from: usize, target: usize) -> Result<[u8; 5]> {
    let mut out = [0u8; 5];
    out[0] = 0xE8;
    out[1..].copy_from_slice(&encode_i32_le(rel32(from + 5, target)?));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jmp_rel_encodes_displacement() {
        let bytes = jmp_rel(0x1000, 0x1010).unwrap();
        assert_eq!(bytes[0], 0xE9);
        // next IP = 0x1005, target 0x1010 → +0xB
        assert_eq!(&bytes[1..], &0xBi32.to_le_bytes());
    }
}
