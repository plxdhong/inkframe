//! Driver for the Waveshare 7.3" e-Paper HAT (E), Spectra 6, 800x480.
//!
//! The command sequence mirrors Waveshare's `EPD_7in3e.c`; check it line by line
//! against the official source when bringing up real hardware.

mod hal;
#[cfg(all(target_os = "linux", feature = "hw"))]
mod linux;
mod mock;

pub use hal::{Hal, HalError};
#[cfg(all(target_os = "linux", feature = "hw"))]
pub use linux::LinuxHal;
pub use mock::{MockHal, MockLog, MockOp};

use common::Frame;
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum EpdError {
    #[error("hal: {0}")]
    Hal(#[from] HalError),
    #[error("BUSY did not release within {0:?} during {1}")]
    BusyTimeout(Duration, &'static str),
}

pub struct Epd<H: Hal> {
    hal: H,
    busy_timeout: Duration,
}

impl<H: Hal> Epd<H> {
    pub fn new(hal: H, busy_timeout: Duration) -> Self {
        Self { hal, busy_timeout }
    }

    pub fn hal(&self) -> &H {
        &self.hal
    }

    /// One complete refresh: power, init, write, refresh, deep sleep, power off.
    /// Power is cut on every exit path, including errors.
    pub fn display(&mut self, frame: &Frame) -> Result<(), EpdError> {
        self.hal.set_power(true)?;
        self.hal.delay(Duration::from_millis(10));
        let result = self.display_powered(frame);
        let sleep = if result.is_ok() { self.sleep() } else { Ok(()) };
        let off = self.hal.set_power(false);
        result?;
        sleep?;
        off?;
        Ok(())
    }

    fn display_powered(&mut self, frame: &Frame) -> Result<(), EpdError> {
        self.init()?;
        self.command(0x10)?; // DATA_START_TRANSMISSION
        self.data(&frame.pack())?;
        self.turn_on_display()
    }

    /// Hardware reset pulse. Also used to recover a wedged controller.
    pub fn reset(&mut self) -> Result<(), EpdError> {
        self.hal.set_rst(true)?;
        self.hal.delay(Duration::from_millis(20));
        self.hal.set_rst(false)?;
        self.hal.delay(Duration::from_millis(2));
        self.hal.set_rst(true)?;
        self.hal.delay(Duration::from_millis(20));
        Ok(())
    }

    fn init(&mut self) -> Result<(), EpdError> {
        self.reset()?;
        self.wait_busy("reset")?;
        self.hal.delay(Duration::from_millis(30));

        self.cmd_data(0xAA, &[0x49, 0x55, 0x20, 0x08, 0x09, 0x18])?; // CMDH
        self.cmd_data(0x01, &[0x3F])?;
        self.cmd_data(0x00, &[0x5F, 0x69])?;
        self.cmd_data(0x03, &[0x00, 0x54, 0x00, 0x44])?;
        self.cmd_data(0x05, &[0x40, 0x1F, 0x1F, 0x2C])?;
        self.cmd_data(0x06, &[0x6F, 0x1F, 0x17, 0x49])?;
        self.cmd_data(0x08, &[0x6F, 0x1F, 0x1F, 0x22])?;
        self.cmd_data(0x30, &[0x03])?;
        self.cmd_data(0x50, &[0x3F])?;
        self.cmd_data(0x60, &[0x02, 0x00])?;
        self.cmd_data(0x61, &[0x03, 0x20, 0x01, 0xE0])?; // 800 x 480
        self.cmd_data(0x84, &[0x01])?;
        self.cmd_data(0xE3, &[0x2F])?;
        self.command(0x04)?; // POWER_ON
        self.wait_busy("init power on")
    }

    fn turn_on_display(&mut self) -> Result<(), EpdError> {
        self.command(0x04)?; // POWER_ON
        self.wait_busy("power on")?;
        self.cmd_data(0x06, &[0x6F, 0x1F, 0x17, 0x49])?; // second booster setting
        self.cmd_data(0x12, &[0x00])?; // DISPLAY_REFRESH
        self.wait_busy("refresh")?;
        self.cmd_data(0x02, &[0x00])?; // POWER_OFF
        self.wait_busy("power off")
    }

    fn sleep(&mut self) -> Result<(), EpdError> {
        self.cmd_data(0x07, &[0xA5])?; // DEEP_SLEEP
        self.hal.delay(Duration::from_millis(100));
        Ok(())
    }

    /// BUSY is low while the controller works. Bounded wait instead of the
    /// unbounded loop in the vendor code.
    fn wait_busy(&mut self, stage: &'static str) -> Result<(), EpdError> {
        let start = Instant::now();
        loop {
            if self.hal.busy_idle()? {
                return Ok(());
            }
            if start.elapsed() >= self.busy_timeout {
                return Err(EpdError::BusyTimeout(self.busy_timeout, stage));
            }
            self.hal.delay(Duration::from_millis(5));
        }
    }

    fn command(&mut self, c: u8) -> Result<(), EpdError> {
        self.hal.set_dc(false)?;
        self.hal.write(&[c])?;
        Ok(())
    }

    fn data(&mut self, d: &[u8]) -> Result<(), EpdError> {
        self.hal.set_dc(true)?;
        // spidev's default buffer is 4096 bytes.
        for chunk in d.chunks(4096) {
            self.hal.write(chunk)?;
        }
        Ok(())
    }

    fn cmd_data(&mut self, c: u8, d: &[u8]) -> Result<(), EpdError> {
        self.command(c)?;
        self.data(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::Ink;

    #[test]
    fn display_sends_full_frame_and_powers_down() {
        let hal = MockHal::new();
        let log = hal.log();
        let mut epd = Epd::new(hal, Duration::from_secs(1));
        assert!(epd.display(&Frame::filled(Ink::Red)).is_ok());
        let ops = log.ops();
        assert_eq!(ops.first(), Some(&MockOp::Power(true)));
        assert_eq!(ops.last(), Some(&MockOp::Power(false)));
        assert_eq!(log.data_after(0x10).len(), 192_000);
        assert!(log.data_after(0x10).iter().all(|&b| b == 0x33));
        assert_eq!(log.data_after(0x07), vec![0xA5]);
        assert_eq!(log.data_after(0x61), vec![0x03, 0x20, 0x01, 0xE0]);
    }

    #[test]
    fn stuck_busy_times_out_and_still_cuts_power() {
        let hal = MockHal::new();
        hal.log().set_stuck(true);
        let log = hal.log();
        let mut epd = Epd::new(hal, Duration::from_millis(50));
        let err = epd.display(&Frame::filled(Ink::White));
        assert!(matches!(err, Err(EpdError::BusyTimeout(_, "reset"))));
        assert_eq!(log.ops().last(), Some(&MockOp::Power(false)));
    }
}
