use crate::hal::{Hal, HalError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockOp {
    Power(bool),
    Rst(bool),
    Command(u8),
    Data(Vec<u8>),
}

#[derive(Default)]
struct Inner {
    ops: Vec<MockOp>,
    dc: bool,
    stuck: bool,
    /// Simulated BUSY-low polls remaining after each command.
    busy_polls: u32,
}

/// Shared view of what the mock panel received; lets tests inject a stuck BUSY.
#[derive(Clone, Default)]
pub struct MockLog(Arc<Mutex<Inner>>);

impl MockLog {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn ops(&self) -> Vec<MockOp> {
        self.lock().ops.clone()
    }

    pub fn set_stuck(&self, stuck: bool) {
        self.lock().stuck = stuck;
    }

    pub fn clear(&self) {
        self.lock().ops.clear();
    }

    /// Concatenated data bytes following the last occurrence of `cmd`.
    pub fn data_after(&self, cmd: u8) -> Vec<u8> {
        let ops = self.ops();
        let Some(pos) = ops.iter().rposition(|o| *o == MockOp::Command(cmd)) else {
            return Vec::new();
        };
        ops[pos + 1..]
            .iter()
            .map_while(|o| match o {
                MockOp::Data(d) => Some(d.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }
}

/// A panel that never touches hardware. Delays are skipped.
pub struct MockHal {
    log: MockLog,
}

impl MockHal {
    pub fn new() -> Self {
        Self {
            log: MockLog::default(),
        }
    }

    pub fn log(&self) -> MockLog {
        self.log.clone()
    }
}

impl Default for MockHal {
    fn default() -> Self {
        Self::new()
    }
}

impl Hal for MockHal {
    fn write(&mut self, bytes: &[u8]) -> Result<(), HalError> {
        let mut g = self.log.lock();
        let op = if g.dc {
            MockOp::Data(bytes.to_vec())
        } else {
            MockOp::Command(bytes.first().copied().unwrap_or(0))
        };
        if !g.dc {
            g.busy_polls = 2;
        }
        g.ops.push(op);
        Ok(())
    }

    fn set_dc(&mut self, high: bool) -> Result<(), HalError> {
        self.log.lock().dc = high;
        Ok(())
    }

    fn set_rst(&mut self, high: bool) -> Result<(), HalError> {
        self.log.lock().ops.push(MockOp::Rst(high));
        Ok(())
    }

    fn set_power(&mut self, on: bool) -> Result<(), HalError> {
        self.log.lock().ops.push(MockOp::Power(on));
        Ok(())
    }

    fn busy_idle(&mut self) -> Result<bool, HalError> {
        let mut g = self.log.lock();
        if g.stuck {
            return Ok(false);
        }
        if g.busy_polls > 0 {
            g.busy_polls -= 1;
            return Ok(false);
        }
        Ok(true)
    }

    fn delay(&mut self, d: Duration) {
        // Keep the timeout path realistic without slowing the happy path.
        if self.log.lock().stuck {
            std::thread::sleep(d);
        }
    }
}
