//! Keeps the most recent log lines in memory for the debug page (journald is
//! volatile and size-capped on the device, so this is the quick view).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

const CAPACITY: usize = 500;

#[derive(Clone, Default)]
pub struct LogBuffer(Arc<Mutex<VecDeque<String>>>);

impl LogBuffer {
    pub fn push(&self, line: String) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= CAPACITY {
            g.pop_front();
        }
        g.push_back(line);
    }

    pub fn tail(&self, n: usize) -> Vec<String> {
        let g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.iter().skip(g.len().saturating_sub(n)).cloned().collect()
    }
}

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}{}", self.0);
        } else {
            self.0.push_str(&format!(" {}={value:?}", field.name()));
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = format!("{value}{}", self.0);
        } else {
            self.0.push_str(&format!(" {}={value}", field.name()));
        }
    }
}

impl<S: Subscriber> Layer<S> for LogBuffer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut f = Fields(String::new());
        event.record(&mut f);
        let ts = chrono::Local::now().format("%m-%d %H:%M:%S");
        self.push(format!("{ts} {:5} {}", event.metadata().level(), f.0));
    }
}
