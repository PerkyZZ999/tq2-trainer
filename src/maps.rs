//! Parse `/proc/<pid>/maps` and select Titan Quest II module regions.

use std::fs;
use std::path::PathBuf;

use crate::error::{Result, TrainerError};
use crate::process::{GAME_EXE_NAME, GameProcess};

/// A single memory mapping from `/proc/<pid>/maps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub start: usize,
    pub end: usize,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub private: bool,
    pub offset: u64,
    pub pathname: Option<PathBuf>,
}

impl MemoryRegion {
    pub fn size(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn perms_string(&self) -> String {
        format!(
            "{}{}{}{}",
            if self.readable { 'r' } else { '-' },
            if self.writable { 'w' } else { '-' },
            if self.executable { 'x' } else { '-' },
            if self.private { 'p' } else { 's' },
        )
    }

    /// True if this mapping's pathname is exactly the TQ2 shipping executable.
    pub fn is_game_module(&self) -> bool {
        self.pathname
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .is_some_and(|name| name == GAME_EXE_NAME)
    }
}

/// All parsed mappings for a process.
#[derive(Debug, Clone)]
pub struct ProcessMaps {
    pub regions: Vec<MemoryRegion>,
}

impl ProcessMaps {
    /// Named file-backed mappings for `TQ2-Win64-Shipping.exe`.
    pub fn named_game_module_regions(&self) -> Vec<&MemoryRegion> {
        self.regions.iter().filter(|r| r.is_game_module()).collect()
    }

    /// Game module regions including Proton's anonymous PE sections.
    ///
    /// Under Wine/Proton the PE header is often a tiny named `r--p` mapping, while
    /// the large `.text` mapping immediately after is anonymous `r-xp`.
    pub fn game_module_regions(&self) -> Vec<&MemoryRegion> {
        let mut out: Vec<&MemoryRegion> = Vec::new();
        for (idx, region) in self.regions.iter().enumerate() {
            if region.is_game_module() {
                out.push(region);
                continue;
            }
            if region.pathname.is_some() {
                continue;
            }
            // Anonymous region contiguous with a named game-module mapping.
            let adjacent_to_named = self.regions.iter().enumerate().any(|(j, other)| {
                j != idx
                    && other.is_game_module()
                    && (other.end == region.start || region.end == other.start)
            });
            if adjacent_to_named {
                out.push(region);
            }
        }
        out
    }

    /// Executable game module regions (including Proton anonymous `.text`).
    pub fn game_executable_regions(&self) -> Vec<&MemoryRegion> {
        self.game_module_regions()
            .into_iter()
            .filter(|r| r.readable && r.executable)
            .collect()
    }
}

/// Read and parse `/proc/<pid>/maps` for the given game process.
pub fn read_maps(process: &GameProcess) -> Result<ProcessMaps> {
    process.ensure_alive()?;

    let path = process.proc_dir().join("maps");
    let contents = fs::read_to_string(&path).map_err(|e| TrainerError::io(&path, e))?;

    let mut regions = Vec::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        regions.push(parse_maps_line(process.pid, line)?);
    }

    Ok(ProcessMaps { regions })
}

/// Parse one `/proc/<pid>/maps` line.
///
/// Format: `address perms offset dev inode [pathname]`
/// Example: `55a1b2c00000-55a1b2d00000 r-xp 00000000 08:01 12345 /path/to/exe`
fn parse_maps_line(pid: u32, line: &str) -> Result<MemoryRegion> {
    let mut parts = line.split_whitespace();

    let range = parts.next().ok_or_else(|| TrainerError::MapsParse {
        pid,
        line: line.to_string(),
    })?;
    let perms = parts.next().ok_or_else(|| TrainerError::MapsParse {
        pid,
        line: line.to_string(),
    })?;
    let offset_str = parts.next().ok_or_else(|| TrainerError::MapsParse {
        pid,
        line: line.to_string(),
    })?;
    // Skip device and inode.
    let _dev = parts.next();
    let _inode = parts.next();
    let pathname = {
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            None
        } else {
            Some(PathBuf::from(rest.join(" ")))
        }
    };

    let (start_str, end_str) = range
        .split_once('-')
        .ok_or_else(|| TrainerError::MapsParse {
            pid,
            line: line.to_string(),
        })?;

    let start = usize::from_str_radix(start_str, 16).map_err(|_| TrainerError::MapsParse {
        pid,
        line: line.to_string(),
    })?;
    let end = usize::from_str_radix(end_str, 16).map_err(|_| TrainerError::MapsParse {
        pid,
        line: line.to_string(),
    })?;
    let offset = u64::from_str_radix(offset_str, 16).map_err(|_| TrainerError::MapsParse {
        pid,
        line: line.to_string(),
    })?;

    let perms_bytes = perms.as_bytes();
    if perms_bytes.len() < 4 {
        return Err(TrainerError::MapsParse {
            pid,
            line: line.to_string(),
        });
    }

    Ok(MemoryRegion {
        start,
        end,
        readable: perms_bytes[0] == b'r',
        writable: perms_bytes[1] == b'w',
        executable: perms_bytes[2] == b'x',
        private: perms_bytes[3] == b'p',
        offset,
        pathname,
    })
}

/// Format a human-readable summary of game module mappings.
pub fn format_game_module_summary(maps: &ProcessMaps) -> String {
    let regions = maps.game_module_regions();
    if regions.is_empty() {
        return format!("No mappings found for {GAME_EXE_NAME}");
    }

    let mut out = String::new();
    out.push_str(&format!("{GAME_EXE_NAME} mappings ({}):\n", regions.len()));
    out.push_str(&format!(
        "  {:18}  {:18}  {:5}  {:>12}  {}\n",
        "START", "END", "PERMS", "SIZE", "PATH"
    ));

    for r in regions {
        let path = r
            .pathname
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[anonymous]".to_string());
        out.push_str(&format!(
            "  0x{:016x}  0x{:016x}  {:5}  {:>12}  {}\n",
            r.start,
            r.end,
            r.perms_string(),
            format_size(r.size()),
            path,
        ));
    }
    out
}

fn format_size(size: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * 1024;
    if size >= MIB {
        format!("{:.1} MiB", size as f64 / MIB as f64)
    } else if size >= KIB {
        format!("{:.1} KiB", size as f64 / KIB as f64)
    } else {
        format!("{size} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_executable_mapping() {
        let line = "7f1234560000-7f1234580000 r-xp 00001000 08:01 99999 /path/to/steamapps/common/Titan Quest II/TQ2/Binaries/Win64/TQ2-Win64-Shipping.exe";
        let region = parse_maps_line(1, line).unwrap();
        assert_eq!(region.start, 0x7f1234560000);
        assert_eq!(region.end, 0x7f1234580000);
        assert!(region.readable);
        assert!(!region.writable);
        assert!(region.executable);
        assert!(region.private);
        assert_eq!(region.offset, 0x1000);
        assert!(region.is_game_module());
        assert_eq!(region.size(), 0x20000);
    }

    #[test]
    fn parse_anonymous_mapping() {
        let line = "7fff00000000-7fff00021000 rw-p 00000000 00:00 0";
        let region = parse_maps_line(1, line).unwrap();
        assert!(region.readable);
        assert!(region.writable);
        assert!(!region.executable);
        assert!(region.pathname.is_none());
        assert!(!region.is_game_module());
    }

    #[test]
    fn parse_pathname_with_spaces() {
        let line = "1000-2000 r--p 00000000 08:01 1 /path/with spaces/TQ2-Win64-Shipping.exe";
        let region = parse_maps_line(1, line).unwrap();
        assert_eq!(
            region.pathname.as_deref(),
            Some(Path::new("/path/with spaces/TQ2-Win64-Shipping.exe"))
        );
        assert!(region.is_game_module());
    }

    #[test]
    fn reject_malformed_line() {
        assert!(parse_maps_line(1, "not-a-maps-line").is_err());
    }

    #[test]
    fn proton_anonymous_text_adjacent_to_pe_header() {
        let header = parse_maps_line(
            1,
            "6ffff1a60000-6ffff1a61000 r--p 00000000 08:02 1 /game/TQ2-Win64-Shipping.exe",
        )
        .unwrap();
        let text = parse_maps_line(1, "6ffff1a61000-6ffff9595000 r-xp 00000000 00:00 0").unwrap();
        let unrelated =
            parse_maps_line(1, "700000000000-700000001000 r-xp 00000000 00:00 0").unwrap();
        let maps = ProcessMaps {
            regions: vec![header, text, unrelated],
        };
        let modules = maps.game_module_regions();
        assert_eq!(modules.len(), 2);
        assert!(modules.iter().any(|r| r.executable && r.pathname.is_none()));
        assert_eq!(maps.game_executable_regions().len(), 1);
        assert_eq!(maps.named_game_module_regions().len(), 1);
    }
}
