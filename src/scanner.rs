//! Wildcard-capable byte pattern scanning.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternByte {
    Exact(u8),
    Wildcard,
}

/// Parse an IDA-style pattern: `48 8B ?? FA`.
pub fn parse_pattern(text: &str) -> Result<Vec<PatternByte>, String> {
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        if tok == "??" || tok == "?" {
            out.push(PatternByte::Wildcard);
        } else {
            let b =
                u8::from_str_radix(tok, 16).map_err(|_| format!("invalid pattern byte `{tok}`"))?;
            out.push(PatternByte::Exact(b));
        }
    }
    if out.is_empty() {
        return Err("empty pattern".into());
    }
    Ok(out)
}

/// Find all offsets of `pattern` within `haystack`.
pub fn scan_pattern(haystack: &[u8], pattern: &[PatternByte]) -> Vec<usize> {
    if pattern.is_empty() || haystack.len() < pattern.len() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let last = haystack.len() - pattern.len();
    'outer: for i in 0..=last {
        for (j, pb) in pattern.iter().enumerate() {
            match *pb {
                PatternByte::Wildcard => {}
                PatternByte::Exact(b) if haystack[i + j] == b => {}
                PatternByte::Exact(_) => continue 'outer,
            }
        }
        hits.push(i);
    }
    hits
}

/// Chunk size for scanning large executable mappings without loading them whole.
pub const SCAN_CHUNK_SIZE: usize = 1024 * 1024;

/// Scan a logical region in overlapping chunks via a read callback.
///
/// `read(offset, buf) -> bytes_read` fills `buf` from the logical region starting at `offset`.
/// Overlap is `pattern.len().saturating_sub(1)` so matches spanning chunk edges are not missed.
pub fn scan_pattern_chunked<F>(
    region_size: usize,
    pattern: &[PatternByte],
    mut read: F,
) -> Result<Vec<usize>, String>
where
    F: FnMut(usize, &mut [u8]) -> Result<usize, String>,
{
    if pattern.is_empty() || region_size < pattern.len() {
        return Ok(Vec::new());
    }

    let overlap = pattern.len().saturating_sub(1);
    let mut hits = Vec::new();
    let mut offset = 0usize;
    let mut chunk = vec![0u8; SCAN_CHUNK_SIZE.min(region_size)];

    while offset < region_size {
        let len = (region_size - offset).min(chunk.len());
        let n = read(offset, &mut chunk[..len])?;
        if n == 0 {
            break;
        }
        for rel in scan_pattern(&chunk[..n], pattern) {
            hits.push(offset + rel);
        }
        if offset + n >= region_size {
            break;
        }
        offset += n.saturating_sub(overlap);
        if n <= overlap {
            break;
        }
    }

    hits.sort_unstable();
    hits.dedup();
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_scan_with_wildcard() {
        let pat = parse_pattern("48 8B ?? FA").unwrap();
        let mem = [0x48, 0x8B, 0x12, 0xFA, 0x90, 0x48, 0x8B, 0x00, 0xFA];
        assert_eq!(scan_pattern(&mem, &pat), vec![0, 5]);
    }

    #[test]
    fn addxp_prologue_unique_fixture() {
        let pat = parse_pattern("48 89 5C 24 20 55 56 41 57 48 81 EC C0 00 00 00").unwrap();
        let mut mem = vec![0x90; 64];
        let bytes = [
            0x48, 0x89, 0x5C, 0x24, 0x20, 0x55, 0x56, 0x41, 0x57, 0x48, 0x81, 0xEC, 0xC0, 0x00,
            0x00, 0x00,
        ];
        mem[10..10 + bytes.len()].copy_from_slice(&bytes);
        assert_eq!(scan_pattern(&mem, &pat), vec![10]);
    }

    #[test]
    fn chunked_scan_finds_pattern_across_chunk_edge() {
        let pat = parse_pattern("DE AD BE EF").unwrap();
        let mut region = vec![0u8; SCAN_CHUNK_SIZE + 8];
        let start = SCAN_CHUNK_SIZE - 2;
        region[start..start + 4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let hits = scan_pattern_chunked(region.len(), &pat, |off, buf| {
            let end = (off + buf.len()).min(region.len());
            let n = end - off;
            buf[..n].copy_from_slice(&region[off..end]);
            Ok(n)
        })
        .unwrap();
        assert_eq!(hits, vec![start]);
    }
}
