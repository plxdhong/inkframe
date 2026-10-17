//! Turns the configured timetable into concrete events. Everything is computed
//! from wall-clock time on every call, so missed events, reboots and clock jumps
//! are handled by asking "what should be on screen now?" again.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use common::calendar::HolidayCalendar;
use common::config::{DaySchedule, ScheduleConfig};
use common::time::parse_hhmm;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    /// Dashboard, today vs yesterday.
    Morning,
    Photo,
    /// Dashboard, tomorrow vs today.
    Evening,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub at: NaiveDateTime,
    pub slot: Slot,
}

impl Event {
    /// Stable identity persisted to disk to know whether an event was handled.
    pub fn key(&self) -> String {
        format!("{}-{:?}", self.at.format("%Y-%m-%dT%H:%M"), self.slot).to_lowercase()
    }
}

fn day_schedule<'a>(
    cfg: &'a ScheduleConfig,
    cal: &HolidayCalendar,
    date: NaiveDate,
) -> &'a DaySchedule {
    if cal.is_workday(date) {
        &cfg.workday
    } else {
        &cfg.offday
    }
}

/// The day's events in time order.
pub fn events_on(cfg: &ScheduleConfig, cal: &HolidayCalendar, date: NaiveDate) -> Vec<Event> {
    let day = day_schedule(cfg, cal, date);
    let mut out: Vec<Event> = Vec::new();
    let mut push = |t: &str, slot| {
        if let Some(time) = parse_hhmm(t) {
            out.push(Event {
                at: date.and_time(time),
                slot,
            });
        }
    };
    push(&day.morning, Slot::Morning);
    for t in &day.photos {
        push(t, Slot::Photo);
    }
    push(&day.evening, Slot::Evening);
    out.sort_by_key(|e| e.at);
    out.dedup_by_key(|e| e.at);
    out
}

/// The most recent event at or before `now`: what the screen should show.
pub fn current(cfg: &ScheduleConfig, cal: &HolidayCalendar, now: NaiveDateTime) -> Option<Event> {
    (0..=2)
        .filter_map(|back| now.date().checked_sub_signed(Duration::days(back)))
        .flat_map(|d| events_on(cfg, cal, d).into_iter().rev())
        .find(|e| e.at <= now)
}

/// The first event strictly after `now`.
pub fn next(cfg: &ScheduleConfig, cal: &HolidayCalendar, now: NaiveDateTime) -> Option<Event> {
    (0..=2)
        .filter_map(|ahead| now.date().checked_add_signed(Duration::days(ahead)))
        .flat_map(|d| events_on(cfg, cal, d))
        .find(|e| e.at > now)
}

/// The next dashboard event after `now` (for weather prefetch).
pub fn next_dashboard(
    cfg: &ScheduleConfig,
    cal: &HolidayCalendar,
    now: NaiveDateTime,
) -> Option<Event> {
    (0..=2)
        .filter_map(|ahead| now.date().checked_add_signed(Duration::days(ahead)))
        .flat_map(|d| events_on(cfg, cal, d))
        .find(|e| e.at > now && e.slot != Slot::Photo)
}

pub fn in_alert_window(cfg: &ScheduleConfig, now: NaiveDateTime) -> bool {
    match (
        parse_hhmm(&cfg.alert_window[0]),
        parse_hhmm(&cfg.alert_window[1]),
    ) {
        (Some(a), Some(b)) => (a..=b).contains(&now.time()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|d| d.and_hms_opt(h, min, 0))
            .unwrap_or_default()
    }

    fn cfg() -> (ScheduleConfig, HolidayCalendar) {
        (ScheduleConfig::default(), HolidayCalendar::new())
    }

    #[test]
    fn workday_timeline() {
        let (c, cal) = cfg();
        // Thu 2026-10-15 is a workday.
        let ev = events_on(&c, &cal, at(2026, 10, 15, 0, 0).date());
        let times: Vec<String> = ev
            .iter()
            .map(|e| e.at.format("%H:%M").to_string())
            .collect();
        assert_eq!(
            times,
            ["07:00", "18:00", "19:00", "20:00", "21:00", "22:30"]
        );
        assert_eq!(
            current(&c, &cal, at(2026, 10, 15, 12, 0)).map(|e| e.slot),
            Some(Slot::Morning)
        );
        assert_eq!(
            current(&c, &cal, at(2026, 10, 15, 18, 30)).map(|e| e.slot),
            Some(Slot::Photo)
        );
    }

    #[test]
    fn evening_view_lasts_until_next_morning() {
        let (c, cal) = cfg();
        let e = current(&c, &cal, at(2026, 10, 16, 6, 59));
        assert_eq!(
            e,
            Some(Event {
                at: at(2026, 10, 15, 22, 30),
                slot: Slot::Evening
            })
        );
        // Saturday morning starts at 08:00, not 07:00.
        let e = current(&c, &cal, at(2026, 10, 17, 7, 30));
        assert_eq!(e.map(|e| e.at), Some(at(2026, 10, 16, 22, 30)));
        assert_eq!(
            next(&c, &cal, at(2026, 10, 17, 7, 30)).map(|e| e.at),
            Some(at(2026, 10, 17, 8, 0))
        );
    }

    #[test]
    fn offday_photos_every_two_hours() {
        let (c, cal) = cfg();
        let ev = events_on(&c, &cal, at(2026, 10, 17, 0, 0).date());
        assert_eq!(ev.iter().filter(|e| e.slot == Slot::Photo).count(), 7);
        assert_eq!(ev.first().map(|e| e.at), Some(at(2026, 10, 17, 8, 0)));
    }

    #[test]
    fn national_day_is_offday_and_makeup_day_is_workday() {
        let (c, cal) = cfg();
        // 2026-10-01 holiday (Thursday); 2026-10-10 make-up workday (Saturday).
        assert_eq!(
            events_on(&c, &cal, at(2026, 10, 1, 0, 0).date())[0].at,
            at(2026, 10, 1, 8, 0)
        );
        assert_eq!(
            events_on(&c, &cal, at(2026, 10, 10, 0, 0).date())[0].at,
            at(2026, 10, 10, 7, 0)
        );
    }

    #[test]
    fn dashboard_prefetch_skips_photos() {
        let (c, cal) = cfg();
        let e = next_dashboard(&c, &cal, at(2026, 10, 15, 17, 0));
        assert_eq!(
            e.map(|e| (e.at, e.slot)),
            Some((at(2026, 10, 15, 22, 30), Slot::Evening))
        );
    }

    #[test]
    fn alert_window() {
        let c = ScheduleConfig::default();
        assert!(in_alert_window(&c, at(2026, 10, 15, 12, 0)));
        assert!(!in_alert_window(&c, at(2026, 10, 15, 23, 0)));
        assert!(!in_alert_window(&c, at(2026, 10, 15, 6, 0)));
    }
}
