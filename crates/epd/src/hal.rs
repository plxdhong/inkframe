use std::time::Duration;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HalError(pub String);

/// The handful of pin and bus operations the panel needs.
pub trait Hal: Send {
    fn write(&mut self, bytes: &[u8]) -> Result<(), HalError>;
    fn set_dc(&mut self, high: bool) -> Result<(), HalError>;
    fn set_rst(&mut self, high: bool) -> Result<(), HalError>;
    /// Panel supply switch; a no-op on HATs without a PWR pin.
    fn set_power(&mut self, on: bool) -> Result<(), HalError>;
    /// True when the controller is idle (BUSY high).
    fn busy_idle(&mut self) -> Result<bool, HalError>;
    fn delay(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

impl<H: Hal + ?Sized> Hal for Box<H> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), HalError> {
        (**self).write(bytes)
    }
    fn set_dc(&mut self, high: bool) -> Result<(), HalError> {
        (**self).set_dc(high)
    }
    fn set_rst(&mut self, high: bool) -> Result<(), HalError> {
        (**self).set_rst(high)
    }
    fn set_power(&mut self, on: bool) -> Result<(), HalError> {
        (**self).set_power(on)
    }
    fn busy_idle(&mut self) -> Result<bool, HalError> {
        (**self).busy_idle()
    }
    fn delay(&mut self, d: Duration) {
        (**self).delay(d)
    }
}
