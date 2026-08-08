//! **Balance watch** — wait until value mirrors transition `before → after`.
//!
//! Deep interface: [`wait_for_transition`]. Mirror I/O sits behind [`MirrorSource`]
//! so production uses process memory and tests use an in-memory adapter.

use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Result, TrainerError};
use crate::memory::ProcessMemory;
use crate::process::GameProcess;
use crate::research::{ValueHit, read_hit};

const DEFAULT_POLL: Duration = Duration::from_millis(100);

/// Seam for reading candidate mirror values.
pub trait MirrorSource {
    fn read_value(&self, hit: &ValueHit) -> Result<i64>;
}

/// Production adapter: live process memory.
pub struct ProcessMirrors<'a> {
    mem: ProcessMemory<'a>,
}

impl<'a> ProcessMirrors<'a> {
    pub fn new(process: &'a GameProcess) -> Self {
        Self {
            mem: ProcessMemory::new(process),
        }
    }
}

impl MirrorSource for ProcessMirrors<'_> {
    fn read_value(&self, hit: &ValueHit) -> Result<i64> {
        read_hit(&self.mem, hit)
    }
}

/// Test / local-substitutable adapter: fixed addresses → values.
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub struct MapMirrors {
    pub values: std::collections::HashMap<usize, i64>,
}

#[cfg(test)]
impl MirrorSource for MapMirrors {
    fn read_value(&self, hit: &ValueHit) -> Result<i64> {
        self.values
            .get(&hit.address)
            .copied()
            .ok_or_else(|| TrainerError::Other(format!("map mirror missing 0x{:x}", hit.address)))
    }
}

/// Parameters for a transition wait (keeps the polled interface small).
pub struct TransitionRequest<'a> {
    pub hits: &'a [ValueHit],
    pub before: i64,
    pub after: i64,
    pub timeout: Duration,
    pub poll: Duration,
}

/// Wait until at least one mirror that held `before` at arm-time now holds `after`.
pub fn wait_for_transition<S, A>(
    source: &S,
    hits: &[ValueHit],
    before: i64,
    after: i64,
    timeout: Duration,
    mut ensure_alive: A,
) -> Result<usize>
where
    S: MirrorSource,
    A: FnMut() -> Result<()>,
{
    let req = TransitionRequest {
        hits,
        before,
        after,
        timeout,
        poll: DEFAULT_POLL,
    };
    wait_for_transition_polled(source, &req, &mut ensure_alive, thread::sleep)
}

/// Same as [`wait_for_transition`] with injectable poll sleep (tests).
pub fn wait_for_transition_polled<S, A, Sleep>(
    source: &S,
    req: &TransitionRequest<'_>,
    ensure_alive: &mut A,
    mut sleep_fn: Sleep,
) -> Result<usize>
where
    S: MirrorSource,
    A: FnMut() -> Result<()>,
    Sleep: FnMut(Duration),
{
    let watchers: Vec<&ValueHit> = req
        .hits
        .iter()
        .filter(|h| source.read_value(h).ok() == Some(req.before))
        .collect();
    if watchers.is_empty() {
        return Err(TrainerError::Other(format!(
            "no live mirrors currently hold {}; pass an exact --current value",
            req.before
        )));
    }

    let deadline = Instant::now() + req.timeout;
    while Instant::now() < deadline {
        ensure_alive()?;
        let mut transitioned = 0usize;
        for hit in &watchers {
            match source.read_value(hit) {
                Ok(v) if v == req.after => transitioned += 1,
                Ok(_)
                | Err(TrainerError::MemoryRead { .. })
                | Err(TrainerError::PermissionDenied) => {}
                Err(e) => return Err(e),
            }
        }
        if transitioned >= 1 {
            return Ok(transitioned);
        }
        sleep_fn(req.poll);
    }
    Err(TrainerError::Other(format!(
        "timed out after {}s waiting for value {} -> {} \
         (need a watched mirror to update; run `sell-gold --disarm` if still armed)",
        req.timeout.as_secs(),
        req.before,
        req.after
    )))
}

/// Convenience used by gold commands.
pub fn wait_for_balance(
    process: &GameProcess,
    before: i64,
    after: i64,
    hits: &[ValueHit],
    timeout: Duration,
) -> Result<usize> {
    let source = ProcessMirrors::new(process);
    wait_for_transition(&source, hits, before, after, timeout, || {
        process.ensure_alive()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::ValueWidth;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    struct CellMirrors {
        values: Rc<RefCell<HashMap<usize, i64>>>,
    }
    impl MirrorSource for CellMirrors {
        fn read_value(&self, hit: &ValueHit) -> Result<i64> {
            self.values
                .borrow()
                .get(&hit.address)
                .copied()
                .ok_or_else(|| TrainerError::Other("missing".into()))
        }
    }

    #[test]
    fn transition_succeeds_when_watcher_flips_after_poll() {
        let values = Rc::new(RefCell::new(HashMap::from([(0x10usize, 50i64)])));
        let source = CellMirrors {
            values: Rc::clone(&values),
        };
        let hits = [ValueHit {
            address: 0x10,
            width: ValueWidth::I64,
        }];
        let req = TransitionRequest {
            hits: &hits,
            before: 50,
            after: 150,
            timeout: Duration::from_secs(1),
            poll: Duration::from_millis(1),
        };
        let values_for_sleep = Rc::clone(&values);
        let mut polls = 0u32;
        let n = wait_for_transition_polled(&source, &req, &mut || Ok(()), |_| {
            polls += 1;
            if polls == 1 {
                values_for_sleep.borrow_mut().insert(0x10, 150);
            }
        })
        .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn ignores_stray_after_that_was_not_at_before() {
        let map = MapMirrors {
            values: HashMap::from([(0x1usize, 999i64), (0x2usize, 50i64)]),
        };
        let hits = [
            ValueHit {
                address: 0x1,
                width: ValueWidth::I64,
            },
            ValueHit {
                address: 0x2,
                width: ValueWidth::I64,
            },
        ];
        let req = TransitionRequest {
            hits: &hits,
            before: 50,
            after: 60,
            timeout: Duration::from_millis(20),
            poll: Duration::from_millis(5),
        };
        let err = wait_for_transition_polled(&map, &req, &mut || Ok(()), |_| {});
        assert!(err.is_err());
    }
}
