//! Collaborative value research (snap / narrow / list / probe).

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::{Result, TrainerError};
use crate::maps::{MemoryRegion, ProcessMaps};
use crate::memory::ProcessMemory;
use crate::process::GameProcess;

const EXP_STATE_PATH: &str = "research-dumps/exp-candidates.txt";
const GOLD_STATE_PATH: &str = "research-dumps/gold-candidates.txt";
const CHUNK_SIZE: usize = 1024 * 1024;
const MAX_LIST: usize = 64;

/// Which on-disk candidate set to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchKind {
    Exp,
    Gold,
}

impl ResearchKind {
    pub fn path(self) -> PathBuf {
        PathBuf::from(match self {
            Self::Exp => EXP_STATE_PATH,
            Self::Gold => GOLD_STATE_PATH,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Exp => "EXP",
            Self::Gold => "gold",
        }
    }
}

/// Integer width for a value candidate hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueWidth {
    I32,
    I64,
}

impl ValueWidth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            _ => None,
        }
    }
}

/// One address that currently holds the searched value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueHit {
    pub address: usize,
    pub width: ValueWidth,
}

/// Persistent candidate set between collaborative snap/narrow rounds.
#[derive(Debug, Clone)]
pub struct CandidateSet {
    pub pid: u32,
    pub last_value: i64,
    pub hits: Vec<ValueHit>,
}

impl CandidateSet {
    pub fn save_labeled(&self, path: &Path, kind_label: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| TrainerError::io(parent, e))?;
        }

        let mut file = File::create(path).map_err(|e| TrainerError::io(path, e))?;
        writeln!(file, "# tq2-trainer {kind_label} research candidates")
            .map_err(|e| TrainerError::io(path, e))?;
        writeln!(file, "pid {}", self.pid).map_err(|e| TrainerError::io(path, e))?;
        writeln!(file, "value {}", self.last_value).map_err(|e| TrainerError::io(path, e))?;
        for hit in &self.hits {
            writeln!(file, "{} 0x{:x}", hit.width.as_str(), hit.address)
                .map_err(|e| TrainerError::io(path, e))?;
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| TrainerError::io(path, e))?;
        let reader = BufReader::new(file);

        let mut pid: Option<u32> = None;
        let mut last_value: Option<i64> = None;
        let mut hits = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(|e| TrainerError::io(path, e))?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("");
            match key {
                "pid" => {
                    let v = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| {
                        TrainerError::Other(format!("bad pid line in {}", path.display()))
                    })?;
                    pid = Some(v);
                }
                "value" => {
                    let v = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| {
                        TrainerError::Other(format!("bad value line in {}", path.display()))
                    })?;
                    last_value = Some(v);
                }
                "i32" | "i64" => {
                    let width = ValueWidth::parse(key).expect("checked");
                    let addr_str = parts.next().ok_or_else(|| {
                        TrainerError::Other(format!("bad hit line in {}", path.display()))
                    })?;
                    let address = parse_address(addr_str).ok_or_else(|| {
                        TrainerError::Other(format!(
                            "bad address `{addr_str}` in {}",
                            path.display()
                        ))
                    })?;
                    hits.push(ValueHit { address, width });
                }
                other => {
                    return Err(TrainerError::Other(format!(
                        "unknown token `{other}` in {}",
                        path.display()
                    )));
                }
            }
        }

        Ok(Self {
            pid: pid
                .ok_or_else(|| TrainerError::Other(format!("missing pid in {}", path.display())))?,
            last_value: last_value.ok_or_else(|| {
                TrainerError::Other(format!("missing value in {}", path.display()))
            })?,
            hits,
        })
    }
}

fn parse_address(s: &str) -> Option<usize> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    usize::from_str_radix(s, 16).ok()
}

/// Initial whole-process scan for an exact value (i32 + i64 LE).
pub fn snap_value(process: &GameProcess, maps: &ProcessMaps, value: i64) -> Result<CandidateSet> {
    let mem = ProcessMemory::new(process);
    let regions = scannable_regions(maps);
    let mut hits = Vec::new();
    let start = Instant::now();

    let i32_bytes = (value as i32).to_le_bytes();
    let i64_bytes = value.to_le_bytes();
    let search_i32 = value >= i32::MIN as i64 && value <= i32::MAX as i64;

    for region in &regions {
        scan_region(
            &mem,
            region,
            search_i32.then_some(i32_bytes.as_slice()),
            Some(i64_bytes.as_slice()),
            &mut hits,
        )?;
    }

    eprintln!(
        "snap scanned {} writable regions in {:.1}s -> {} hits",
        regions.len(),
        start.elapsed().as_secs_f64(),
        hits.len()
    );

    Ok(CandidateSet {
        pid: process.pid,
        last_value: value,
        hits,
    })
}

/// Re-read previous hits and keep only those that now equal `value`.
pub fn narrow_value(
    process: &GameProcess,
    previous: &CandidateSet,
    value: i64,
) -> Result<CandidateSet> {
    if previous.pid != process.pid {
        return Err(TrainerError::Other(format!(
            "candidate set is for PID {}, but game is now PID {} — start a new snap",
            previous.pid, process.pid
        )));
    }

    let mem = ProcessMemory::new(process);
    let mut hits = Vec::new();

    for hit in &previous.hits {
        if address_holds(&mem, hit, value)? {
            hits.push(hit.clone());
        }
    }

    Ok(CandidateSet {
        pid: process.pid,
        last_value: value,
        hits,
    })
}

/// Keep hits whose live value still equals `value` (same as narrow, shared name).
pub fn filter_live(
    process: &GameProcess,
    previous: &CandidateSet,
    value: i64,
) -> Result<CandidateSet> {
    narrow_value(process, previous, value)
}

/// Read the integer currently stored at a hit.
pub fn read_hit(mem: &ProcessMemory<'_>, hit: &ValueHit) -> Result<i64> {
    match hit.width {
        ValueWidth::I32 => {
            let mut buf = [0u8; 4];
            mem.read(hit.address, &mut buf)?;
            Ok(i32::from_le_bytes(buf) as i64)
        }
        ValueWidth::I64 => {
            let mut buf = [0u8; 8];
            mem.read(hit.address, &mut buf)?;
            Ok(i64::from_le_bytes(buf))
        }
    }
}

/// Write an absolute integer to a hit and verify readback.
pub fn write_hit(mem: &ProcessMemory<'_>, hit: &ValueHit, value: i64) -> Result<()> {
    match hit.width {
        ValueWidth::I32 => {
            if value < i32::MIN as i64 || value > i32::MAX as i64 {
                return Err(TrainerError::Other(format!(
                    "value {value} does not fit in i32 at 0x{:x}",
                    hit.address
                )));
            }
            let bytes = (value as i32).to_le_bytes();
            mem.write_verified(hit.address, &bytes)?;
        }
        ValueWidth::I64 => {
            let bytes = value.to_le_bytes();
            mem.write_verified(hit.address, &bytes)?;
        }
    }
    Ok(())
}

/// Guarded probe: require exactly one candidate that still holds `expected`, then write `new_value`.
pub fn probe_write(
    process: &GameProcess,
    set: &CandidateSet,
    expected: i64,
    new_value: i64,
) -> Result<ValueHit> {
    let live = filter_live(process, set, expected)?;
    if live.hits.is_empty() {
        return Err(TrainerError::Other(
            "no candidates still hold the expected value — re-snap / narrow first".into(),
        ));
    }
    if live.hits.len() > 1 {
        return Err(TrainerError::Other(format!(
            "refusing probe write: {} candidates still match (need exactly 1) — narrow further",
            live.hits.len()
        )));
    }

    let hit = live.hits[0].clone();
    let mem = ProcessMemory::new(process);
    write_hit(&mem, &hit, new_value)?;
    Ok(hit)
}

pub fn address_holds(mem: &ProcessMemory<'_>, hit: &ValueHit, value: i64) -> Result<bool> {
    match hit.width {
        ValueWidth::I32 => {
            if value < i32::MIN as i64 || value > i32::MAX as i64 {
                return Ok(false);
            }
            let mut buf = [0u8; 4];
            match mem.read(hit.address, &mut buf) {
                Ok(()) => Ok(i32::from_le_bytes(buf) as i64 == value),
                Err(TrainerError::MemoryRead { .. }) | Err(TrainerError::PermissionDenied) => {
                    Ok(false)
                }
                Err(e) => Err(e),
            }
        }
        ValueWidth::I64 => {
            let mut buf = [0u8; 8];
            match mem.read(hit.address, &mut buf) {
                Ok(()) => Ok(i64::from_le_bytes(buf) == value),
                Err(TrainerError::MemoryRead { .. }) | Err(TrainerError::PermissionDenied) => {
                    Ok(false)
                }
                Err(e) => Err(e),
            }
        }
    }
}

fn scannable_regions(maps: &ProcessMaps) -> Vec<&MemoryRegion> {
    maps.regions
        .iter()
        .filter(|r| {
            r.readable
                && r.writable
                && r.private
                && !r.executable
                && r.size() >= 4
                // Skip absurdly huge mappings (often GPU/Wine special).
                && r.size() <= 512 * 1024 * 1024
        })
        .collect()
}

fn scan_region(
    mem: &ProcessMemory<'_>,
    region: &MemoryRegion,
    i32_pat: Option<&[u8]>,
    i64_pat: Option<&[u8]>,
    hits: &mut Vec<ValueHit>,
) -> Result<()> {
    let mut offset = 0usize;
    let mut chunk = vec![0u8; CHUNK_SIZE.min(region.size())];

    while offset < region.size() {
        let len = (region.size() - offset).min(chunk.len());
        let addr = region.start + offset;
        match mem.read(addr, &mut chunk[..len]) {
            Ok(()) => {}
            Err(TrainerError::MemoryRead { .. }) | Err(TrainerError::PermissionDenied) => {
                // Skip unreadable pages; advance by page.
                offset += 0x1000;
                continue;
            }
            Err(e) => return Err(e),
        }

        if let Some(pat) = i32_pat {
            for rel in find_all(&chunk[..len], pat) {
                hits.push(ValueHit {
                    address: addr + rel,
                    width: ValueWidth::I32,
                });
            }
        }
        if let Some(pat) = i64_pat {
            for rel in find_all(&chunk[..len], pat) {
                hits.push(ValueHit {
                    address: addr + rel,
                    width: ValueWidth::I64,
                });
            }
        }

        // Overlap by 7 bytes so patterns spanning chunk boundaries are not missed.
        if len <= 7 {
            break;
        }
        offset += len - 7;
    }

    Ok(())
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let last = haystack.len() - needle.len();
    for i in 0..=last {
        if haystack[i..i + needle.len()] == *needle {
            out.push(i);
        }
    }
    out
}

/// Print a concise candidate summary.
pub fn format_candidates_for(set: &CandidateSet, kind_label: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "PID {} | last value {} | {} candidates\n",
        set.pid,
        set.last_value,
        set.hits.len()
    ));

    let i32_count = set
        .hits
        .iter()
        .filter(|h| h.width == ValueWidth::I32)
        .count();
    let i64_count = set
        .hits
        .iter()
        .filter(|h| h.width == ValueWidth::I64)
        .count();
    out.push_str(&format!("  i32: {i32_count}  i64: {i64_count}\n"));

    if set.hits.is_empty() {
        out.push_str(&format!(
            "  (none — try a fresh snap, or confirm the on-screen {kind_label})\n"
        ));
        return out;
    }

    let show = set.hits.len().min(MAX_LIST);
    out.push_str(&format!("  first {show}:\n"));
    for hit in set.hits.iter().take(show) {
        out.push_str(&format!(
            "    0x{:016x}  {}\n",
            hit.address,
            hit.width.as_str()
        ));
    }
    if set.hits.len() > MAX_LIST {
        out.push_str(&format!("  ... {} more\n", set.hits.len() - MAX_LIST));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_all_overlapping() {
        let hay = [1, 2, 3, 1, 2, 3];
        assert_eq!(find_all(&hay, &[1, 2]), vec![0, 3]);
    }

    #[test]
    fn parse_address_hex() {
        assert_eq!(parse_address("0x10"), Some(16));
        assert_eq!(parse_address("ff"), Some(255));
    }

    #[test]
    fn research_kind_paths() {
        assert!(ResearchKind::Exp.path().ends_with("exp-candidates.txt"));
        assert!(ResearchKind::Gold.path().ends_with("gold-candidates.txt"));
    }

    #[test]
    fn candidate_set_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tq2-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cands.txt");
        let set = CandidateSet {
            pid: 42,
            last_value: 12345,
            hits: vec![
                ValueHit {
                    address: 0x1000,
                    width: ValueWidth::I32,
                },
                ValueHit {
                    address: 0x2000,
                    width: ValueWidth::I64,
                },
            ],
        };
        set.save_labeled(&path, "gold").unwrap();
        let loaded = CandidateSet::load(&path).unwrap();
        assert_eq!(loaded.pid, 42);
        assert_eq!(loaded.last_value, 12345);
        assert_eq!(loaded.hits, set.hits);
        let _ = fs::remove_dir_all(&dir);
    }
}
