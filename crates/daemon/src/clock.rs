use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use std::sync::atomic::{AtomicI64, Ordering};

/// Wall-clock time in the configured timezone. The offset exists so the debug
/// API can move time forward and exercise the schedule without waiting.
pub struct Clock {
    tz: Tz,
    offset_secs: AtomicI64,
}

impl Clock {
    pub fn new(tz: Tz) -> Self {
        Self {
            tz,
            offset_secs: AtomicI64::new(0),
        }
    }

    pub fn now(&self) -> DateTime<Tz> {
        (Utc::now() + Duration::seconds(self.offset_secs.load(Ordering::Relaxed)))
            .with_timezone(&self.tz)
    }

    pub fn local(&self) -> NaiveDateTime {
        self.now().naive_local()
    }

    pub fn tz(&self) -> Tz {
        self.tz
    }

    pub fn set_offset(&self, secs: i64) {
        self.offset_secs.store(secs, Ordering::Relaxed);
    }

    pub fn offset(&self) -> i64 {
        self.offset_secs.load(Ordering::Relaxed)
    }
}

/// Whether the system clock has been set by NTP. The Pi has no RTC: until
/// timesyncd syncs, it boots with the time of the last shutdown, which can be
/// weeks old after a long outage. Hosts without timesyncd count as synced.
pub fn system_synced() -> bool {
    let dir = std::path::Path::new("/run/systemd/timesync");
    !dir.exists() || dir.join("synchronized").exists()
}
