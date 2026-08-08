//! Process memory access via `process_vm_readv` and `/proc/<pid>/mem` writes.

use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::mem;
use std::path::PathBuf;

use libc::{c_void, iovec, process_vm_readv};

use crate::error::{Result, TrainerError};
use crate::process::GameProcess;

/// Safe wrapper around Linux process memory access.
pub struct ProcessMemory<'a> {
    process: &'a GameProcess,
}

impl<'a> ProcessMemory<'a> {
    pub fn new(process: &'a GameProcess) -> Self {
        Self { process }
    }

    /// Read `buf.len()` bytes from `address` in the target process.
    pub fn read(&self, address: usize, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }

        self.process.ensure_alive()?;

        let local = iovec {
            iov_base: buf.as_mut_ptr().cast::<c_void>(),
            iov_len: buf.len(),
        };
        let remote = iovec {
            iov_base: address as *mut c_void,
            iov_len: buf.len(),
        };

        // SAFETY: local points at a valid mutable buffer of `buf.len()` bytes.
        // remote is an address in another process; the kernel validates it.
        let n_read = unsafe { process_vm_readv(self.process.pid as i32, &local, 1, &remote, 1, 0) };

        if n_read < 0 {
            let err = io::Error::last_os_error();
            return match err.raw_os_error() {
                Some(libc::EPERM) | Some(libc::EACCES) => Err(TrainerError::PermissionDenied),
                Some(libc::ESRCH) => Err(TrainerError::ProcessGone(self.process.pid)),
                _ => Err(TrainerError::MemoryRead {
                    address,
                    source: err,
                }),
            };
        }

        if (n_read as usize) != buf.len() {
            return Err(TrainerError::MemoryRead {
                address,
                source: io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("short read: expected {} bytes, got {n_read}", buf.len()),
                ),
            });
        }

        Ok(())
    }

    /// Write bytes using `/proc/<pid>/mem` (FOLL_FORCE — works on r-xp `.text`).
    pub fn write(&self, address: usize, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.process.ensure_alive()?;

        let path = PathBuf::from(format!("/proc/{}/mem", self.process.pid));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| match e.raw_os_error() {
                Some(libc::EPERM) | Some(libc::EACCES) => TrainerError::PermissionDenied,
                _ => TrainerError::io(&path, e),
            })?;

        file.seek(SeekFrom::Start(address as u64))
            .map_err(|e| TrainerError::MemoryWrite { address, source: e })?;

        file.write_all(data).map_err(|e| match e.raw_os_error() {
            Some(libc::EPERM) | Some(libc::EACCES) => TrainerError::PermissionDenied,
            _ => TrainerError::MemoryWrite { address, source: e },
        })?;

        Ok(())
    }

    /// Write then read-back verify.
    pub fn write_verified(&self, address: usize, data: &[u8]) -> Result<()> {
        // Read-before is the caller's responsibility (patch state checks).
        self.write(address, data)?;

        let mut actual = vec![0u8; data.len()];
        self.read(address, &mut actual)?;
        if actual.as_slice() != data {
            return Err(TrainerError::WriteVerify {
                address,
                expected: data.to_vec(),
                actual,
            });
        }
        Ok(())
    }

    /// Probe whether memory access to this process is permitted.
    pub fn probe_access(&self, address: usize) -> Result<()> {
        let mut buf = [0u8; 16];
        self.read(address, &mut buf)?;
        std::hint::black_box(buf);
        Ok(())
    }

    /// Convenience: read a fixed-size vector.
    pub fn read_vec(&self, address: usize, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.read(address, &mut buf)?;
        Ok(buf)
    }
}

#[allow(dead_code)]
fn _iovec_size_check() {
    let _ = mem::size_of::<iovec>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::GameProcess;

    #[test]
    fn empty_read_succeeds_without_syscall() {
        let process = GameProcess {
            pid: 1,
            cmdline: "dummy".into(),
        };
        let mem = ProcessMemory::new(&process);
        let mut buf = [];
        assert!(mem.read(0, &mut buf).is_ok());
    }
}
