//! Lunar dates, solar terms, statutory holidays and the workday rule.

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tyme4rs::tyme::holiday::LegalHoliday;
use tyme4rs::tyme::solar::SolarDay;
use tyme4rs::tyme::{Culture, Tyme};

fn solar_day(date: NaiveDate) -> SolarDay {
    SolarDay::from_ymd(
        date.year() as isize,
        date.month() as usize,
        date.day() as usize,
    )
}

/// "九月初五", "闰六月十五", ...
pub fn lunar_text(date: NaiveDate) -> String {
    let lunar = solar_day(date).get_lunar_day();
    format!("{}{}", lunar.get_lunar_month().get_name(), lunar.get_name())
}

/// The next solar term strictly after `date`, with the number of days until it.
pub fn next_solar_term(date: NaiveDate) -> (String, i64) {
    let today = solar_day(date);
    let mut term = today.get_term();
    loop {
        let day = term.get_solar_day();
        let days = day.subtract(today) as i64;
        if days > 0 {
            return (term.get_name(), days);
        }
        term = term.next(1);
    }
}

/// The solar term that starts on `date`, if any.
pub fn solar_term_today(date: NaiveDate) -> Option<String> {
    let today = solar_day(date);
    let term = today.get_term();
    (term.get_solar_day().subtract(today) == 0).then(|| term.get_name())
}

/// One entry of a holiday-cn year file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HolidayDay {
    pub name: String,
    pub date: NaiveDate,
    pub is_off_day: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HolidayYear {
    pub year: i32,
    pub days: Vec<HolidayDay>,
}

/// Statutory holidays and make-up workdays. Fetched holiday-cn data wins; the
/// tables built into tyme4rs fill years that have not been fetched.
#[derive(Debug, Clone, Default)]
pub struct HolidayCalendar {
    days: BTreeMap<NaiveDate, HolidayDay>,
}

impl HolidayCalendar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_year(&mut self, year: HolidayYear) {
        for d in year.days {
            self.days.insert(d.date, d);
        }
    }

    pub fn add_year_json(&mut self, json: &str) -> Result<i32, serde_json::Error> {
        let year: HolidayYear = serde_json::from_str(json)?;
        let y = year.year;
        self.add_year(year);
        Ok(y)
    }

    fn lookup(&self, date: NaiveDate) -> Option<HolidayDay> {
        if let Some(d) = self.days.get(&date) {
            return Some(d.clone());
        }
        LegalHoliday::from_ymd(
            date.year() as isize,
            date.month() as usize,
            date.day() as usize,
        )
        .map(|h| HolidayDay {
            name: h.get_name(),
            date,
            is_off_day: !h.is_work(),
        })
    }

    pub fn is_workday(&self, date: NaiveDate) -> bool {
        match self.lookup(date) {
            Some(d) => !d.is_off_day,
            None => !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
        }
    }

    /// First day of the next holiday block after `date`. Falls back to New Year's
    /// Day when no data covers the coming months.
    pub fn next_holiday(&self, date: NaiveDate) -> (String, i64) {
        let mut d = date;
        let mut prev_off_name: Option<String> =
            self.lookup(date).filter(|h| h.is_off_day).map(|h| h.name);
        for _ in 0..366 {
            d = match d.succ_opt() {
                Some(n) => n,
                None => break,
            };
            match self.lookup(d) {
                Some(h) if h.is_off_day => {
                    if prev_off_name.as_deref() != Some(h.name.as_str()) {
                        return (h.name, (d - date).num_days());
                    }
                    prev_off_name = Some(h.name);
                }
                _ => prev_off_name = None,
            }
            if d.month() == 1 && d.day() == 1 {
                return ("元旦".into(), (d - date).num_days());
            }
        }
        ("元旦".into(), 0)
    }
}

pub fn weekday_cn(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "星期一",
        Weekday::Tue => "星期二",
        Weekday::Wed => "星期三",
        Weekday::Thu => "星期四",
        Weekday::Fri => "星期五",
        Weekday::Sat => "星期六",
        Weekday::Sun => "星期日",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap_or_default()
    }

    #[test]
    fn lunar_and_terms_match_design_fixture() {
        // 2026-10-15: new moon fell late on Oct 10 (UTC+8), so 九月初六 (the mockup text said 初五).
        assert_eq!(lunar_text(d(2026, 10, 15)), "九月初六");
        assert_eq!(next_solar_term(d(2026, 10, 15)), ("霜降".to_string(), 8));
        assert_eq!(
            HolidayCalendar::new().next_holiday(d(2026, 10, 15)),
            ("元旦".to_string(), 78)
        );
    }

    #[test]
    fn term_day_itself_is_not_next() {
        assert_eq!(solar_term_today(d(2026, 10, 23)).as_deref(), Some("霜降"));
        assert_eq!(next_solar_term(d(2026, 10, 23)).0, "立冬");
    }

    #[test]
    fn workday_rules() {
        let mut cal = HolidayCalendar::new();
        let json = r#"{"year":2030,"days":[
            {"name":"国庆节","date":"2030-10-01","isOffDay":true},
            {"name":"国庆节","date":"2030-10-02","isOffDay":true},
            {"name":"国庆节","date":"2030-09-29","isOffDay":false}]}"#;
        assert_eq!(cal.add_year_json(json).ok(), Some(2030));
        assert!(!cal.is_workday(d(2030, 10, 1))); // Tuesday holiday
        assert!(cal.is_workday(d(2030, 9, 29))); // Sunday make-up day
        assert!(!cal.is_workday(d(2030, 10, 5))); // plain Saturday
        assert!(cal.is_workday(d(2030, 10, 8))); // plain Tuesday
        assert_eq!(cal.next_holiday(d(2030, 9, 20)), ("国庆节".to_string(), 11));
        // During the holiday, the next one is not the same block.
        assert_ne!(cal.next_holiday(d(2030, 10, 1)).0, "国庆节");
    }

    #[test]
    fn builtin_tables_cover_2026() {
        // National Day 2026 is in tyme4rs's built-in table.
        assert!(!HolidayCalendar::new().is_workday(d(2026, 10, 1)));
    }
}
