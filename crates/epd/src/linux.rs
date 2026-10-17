use crate::hal::{Hal, HalError};
use common::config::DisplayConfig;
use gpiocdev::line::Value;
use gpiocdev::Request;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io::Write;

fn err<E: std::fmt::Display>(ctx: &'static str) -> impl Fn(E) -> HalError {
    move |e| HalError(format!("{ctx}: {e}"))
}

pub struct LinuxHal {
    spi: Spidev,
    outputs: Request,
    busy: Request,
    rst: u32,
    dc: u32,
    pwr: Option<u32>,
    busy_pin: u32,
}

impl LinuxHal {
    pub fn open(cfg: &DisplayConfig) -> Result<Self, HalError> {
        let mut spi = Spidev::open(&cfg.spi_device).map_err(err("open spi"))?;
        let opts = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(10_000_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spi.configure(&opts).map_err(err("configure spi"))?;

        let p = &cfg.pins;
        let mut out = Request::builder();
        out.on_chip(&cfg.gpio_chip).with_consumer("inkframe");
        out.with_lines(&[p.rst, p.dc]).as_output(Value::Inactive);
        if p.has_pwr {
            out.with_line(p.pwr).as_output(Value::Inactive);
        }
        let outputs = out.request().map_err(err("request output lines"))?;
        let busy = Request::builder()
            .on_chip(&cfg.gpio_chip)
            .with_consumer("inkframe-busy")
            .with_line(p.busy)
            .as_input()
            .request()
            .map_err(err("request busy line"))?;
        Ok(Self {
            spi,
            outputs,
            busy,
            rst: p.rst,
            dc: p.dc,
            pwr: p.has_pwr.then_some(p.pwr),
            busy_pin: p.busy,
        })
    }

    fn set(&self, line: u32, high: bool) -> Result<(), HalError> {
        let v = if high { Value::Active } else { Value::Inactive };
        self.outputs
            .set_value(line, v)
            .map(|_| ())
            .map_err(err("set gpio"))
    }
}

impl Hal for LinuxHal {
    fn write(&mut self, bytes: &[u8]) -> Result<(), HalError> {
        self.spi.write_all(bytes).map_err(err("spi write"))
    }

    fn set_dc(&mut self, high: bool) -> Result<(), HalError> {
        self.set(self.dc, high)
    }

    fn set_rst(&mut self, high: bool) -> Result<(), HalError> {
        self.set(self.rst, high)
    }

    fn set_power(&mut self, on: bool) -> Result<(), HalError> {
        match self.pwr {
            Some(p) => self.set(p, on),
            None => Ok(()),
        }
    }

    fn busy_idle(&mut self) -> Result<bool, HalError> {
        let v = self.busy.value(self.busy_pin).map_err(err("read busy"))?;
        Ok(v == Value::Active)
    }
}
