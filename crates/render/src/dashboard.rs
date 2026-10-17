//! The calendar + weather dashboard, laid out after the "AB 融合" design board.
//!
//! Left: red calendar page. Right top: blue weather panel and yellow
//! change-vs-reference card. Right bottom: two advice tiles.

use crate::canvas::Canvas;
use crate::icons;
use crate::text::{Face, Fonts, Style};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use common::calendar::{self, HolidayCalendar};
use common::config::Thresholds;
use common::weather::{DailyForecast, Warning};
use common::{Frame, Ink};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    /// Today compared with yesterday.
    Morning,
    /// Tomorrow compared with today.
    Evening,
}

pub struct DashboardInput<'a> {
    pub view: View,
    pub now: NaiveDateTime,
    pub city: &'a str,
    /// Today's forecast for the morning view, tomorrow's for the evening view.
    pub target: Option<&'a DailyForecast>,
    /// Yesterday's record for the morning view, today's for the evening view.
    pub reference: Option<&'a DailyForecast>,
    pub fetched_at: Option<NaiveDateTime>,
    pub air: Option<&'a str>,
    pub warnings: &'a [Warning],
    pub holidays: &'a HolidayCalendar,
    pub thresholds: &'a Thresholds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WeatherIcon {
    Sunny,
    Partly,
    Cloudy,
    Rain,
    Thunder,
    Snow,
    Fog,
}

impl WeatherIcon {
    /// QWeather icon codes: https://dev.qweather.com/docs/resource/icons/
    pub fn from_qweather(code: &str) -> Self {
        let n: u32 = code.parse().unwrap_or(104);
        match n {
            100 | 150 => Self::Sunny,
            101..=103 | 151..=153 => Self::Partly,
            302..=304 => Self::Thunder,
            300..=399 => Self::Rain,
            400..=499 => Self::Snow,
            500..=515 => Self::Fog,
            _ => Self::Cloudy,
        }
    }

    fn svg(self) -> &'static str {
        match self {
            Self::Sunny => icons::SUN,
            Self::Partly => icons::PARTLY,
            Self::Cloudy => icons::CLOUDY,
            Self::Rain => icons::RAIN,
            Self::Thunder => icons::THUNDER,
            Self::Snow => icons::SNOW,
            Self::Fog => icons::FOG,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TipKind {
    Alert,
    Umbrella,
    NoUmbrella,
    Clothes,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tip {
    pub kind: TipKind,
    pub title: String,
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Delta {
    pub label: String,
    pub value: String,
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DashboardModel {
    pub view: View,
    pub year_month: String,
    pub day: u32,
    pub lunar: String,
    pub weekday: String,
    pub countdowns: [String; 2],
    pub header: String,
    pub updated: String,
    pub stale: bool,
    pub condition: String,
    pub icon: WeatherIcon,
    /// (low, high)
    pub temps: Option<(i32, i32)>,
    pub detail: String,
    pub delta: Delta,
    pub tips: [Tip; 2],
}

/// Data older than this at render time is labelled with its fetch time.
const STALE_MINUTES: i64 = 180;

fn signed(d: i32) -> String {
    match d {
        0 => "持平".into(),
        d if d > 0 => format!("↑{d}°"),
        d => format!("↓{}°", -d),
    }
}

fn warning_color_cn(c: &str) -> &'static str {
    match c.to_ascii_lowercase().as_str() {
        "blue" => "蓝色",
        "yellow" => "黄色",
        "orange" => "橙色",
        "red" => "红色",
        "white" => "白色",
        _ => "",
    }
}

/// The day a view describes. The evening view stays up overnight, so after
/// midnight it still means the day after the evening it was made for.
pub fn target_date(view: View, now: NaiveDateTime) -> NaiveDate {
    match view {
        View::Morning => now.date(),
        View::Evening => {
            let evening = (now - chrono::Duration::hours(12)).date();
            evening.succ_opt().unwrap_or(evening)
        }
    }
}

pub fn build_model(input: &DashboardInput) -> DashboardModel {
    let today = input.now.date();
    let date = target_date(input.view, input.now);
    let (term, term_days) = calendar::next_solar_term(date);
    let (holiday, holiday_days) = input.holidays.next_holiday(date);
    let first_countdown = match calendar::solar_term_today(date) {
        Some(t) => format!("今日{t}"),
        None => format!("距{term} {term_days} 天"),
    };
    let countdowns = [first_countdown, format!("距{holiday} {holiday_days} 天")];

    let day_word = match input.view {
        View::Morning => "今日",
        View::Evening => "明日",
    };
    let stale = input
        .fetched_at
        .is_none_or(|f| (input.now - f).num_minutes() > STALE_MINUTES);
    let updated = match input.fetched_at {
        None => "暂无数据".into(),
        Some(f) if stale && f.date() != today => format!("更新于 {}", f.format("%m/%d %H:%M")),
        Some(f) if stale => format!("更新于 {}", f.format("%H:%M")),
        Some(_) => format!("{} 更新", input.now.format("%H:%M")),
    };

    let t = input.target;
    let condition = match t {
        Some(f) if f.text_day == f.text_night || f.text_night.is_empty() => f.text_day.clone(),
        Some(f) => format!("{}转{}", f.text_day, f.text_night),
        None => "暂无天气".into(),
    };
    let detail = t
        .map(|f| {
            let mut parts = vec![
                format!("湿度 {}%", f.humidity),
                format!("{} {} 级", f.wind_dir, f.wind_scale),
            ];
            if let Some(a) = input.air {
                parts.push(format!("空气{a}"));
            }
            parts.join("　")
        })
        .unwrap_or_default();

    let ref_word = match input.view {
        View::Morning => "比昨天",
        View::Evening => "比今天",
    };
    let (delta, max_change, min_change) = match (t, input.reference) {
        (Some(a), Some(b)) => {
            let (dmax, dmin) = (a.temp_max - b.temp_max, a.temp_min - b.temp_min);
            let (v, cap) = if dmax.abs() >= dmin.abs() {
                (dmax, "最高气温")
            } else {
                (dmin, "最低气温")
            };
            (
                Delta {
                    label: ref_word.into(),
                    value: signed(v),
                    caption: cap.into(),
                },
                dmax,
                dmin,
            )
        }
        _ => (
            Delta {
                label: ref_word.into(),
                value: "—".into(),
                caption: "暂无对比".into(),
            },
            0,
            0,
        ),
    };

    let th = input.thresholds;
    let mut tips: Vec<Tip> = Vec::new();
    if let Some(w) = input.warnings.first() {
        tips.push(Tip {
            kind: TipKind::Alert,
            title: format!("{}{}预警", w.type_name, warning_color_cn(&w.color)),
            caption: format!("发布 {}", w.pub_time),
        });
    }
    let pop = t.and_then(|f| f.precip_prob);
    match pop {
        Some(p) if p >= th.rain_probability_pct => tips.push(Tip {
            kind: TipKind::Umbrella,
            title: "带伞出门".into(),
            caption: format!("降水概率 {p}%"),
        }),
        Some(p) => tips.push(Tip {
            kind: TipKind::NoUmbrella,
            title: "不用带伞".into(),
            caption: format!("降水概率 {p}%"),
        }),
        None if t.is_some_and(|f| f.precip_mm > 0.0) => tips.push(Tip {
            kind: TipKind::Umbrella,
            title: "带伞出门".into(),
            caption: "预计有降水".into(),
        }),
        None => {}
    }
    let thr = th.temp_change_c;
    if let Some(f) = t {
        let tip = if max_change <= -thr || min_change <= -thr {
            Tip {
                kind: TipKind::Clothes,
                title: "加件外套".into(),
                caption: format!("早晚只有 {}°", f.temp_min),
            }
        } else if max_change >= thr || min_change >= thr {
            Tip {
                kind: TipKind::Clothes,
                title: "减件衣服".into(),
                caption: format!("最高 {}°", f.temp_max),
            }
        } else {
            Tip {
                kind: TipKind::Clothes,
                title: "穿衣照旧".into(),
                caption: format!("{}–{}°", f.temp_min, f.temp_max),
            }
        };
        tips.push(tip);
    }
    while tips.len() < 2 {
        tips.push(Tip {
            kind: TipKind::NoUmbrella,
            title: "暂无提示".into(),
            caption: "等待天气数据".into(),
        });
    }
    let mut it = tips.into_iter();
    let tips = [it.next(), it.next()].map(|t| {
        t.unwrap_or(Tip {
            kind: TipKind::Clothes,
            title: String::new(),
            caption: String::new(),
        })
    });

    DashboardModel {
        view: input.view,
        year_month: format!("{} 年 {} 月", date.year(), date.month()),
        day: date.day(),
        lunar: format!("农历{}", calendar::lunar_text(date)),
        weekday: calendar::weekday_cn(date).into(),
        countdowns,
        header: format!("{} {}", input.city, day_word),
        updated,
        stale,
        condition,
        icon: t
            .map(|f| WeatherIcon::from_qweather(&f.icon_day))
            .unwrap_or(WeatherIcon::Cloudy),
        temps: t.map(|f| (f.temp_min, f.temp_max)),
        detail,
        delta,
        tips,
    }
}

const PAPER: Ink = Ink::White;

pub fn render(model: &DashboardModel, fonts: &Fonts) -> Frame {
    let mut c = Canvas::new(fonts, PAPER);
    calendar_page(&mut c, model);
    weather_panel(&mut c, model);
    delta_card(&mut c, model);
    tip_tile(&mut c, &model.tips[0], 288, true);
    tip_tile(&mut c, &model.tips[1], 544, false);
    c.into_frame()
}

/// Red page, x 8..280, y 8..472, padding 24 22 20 26.
fn calendar_page(c: &mut Canvas, m: &DashboardModel) {
    c.fill(8, 8, 272, 464, Ink::Red);
    let (left, right, bottom) = (34.0, 258.0, 452.0);
    let mut y = 32.0;
    let ym = Style::new(Face::SansBold, 20.0);
    c.text(left, y, &ym, PAPER, &m.year_month);
    y += c.line_height(&ym) + 2.0;

    let num = Style::new(Face::SerifBlack, 156.0)
        .spacing(-6.0)
        .line_height(0.95);
    let day = m.day.to_string();
    let num_w = c.measure(&num, &day);
    c.text(left, y, &num, PAPER, &day);
    let lunar = Style::new(Face::SerifBold, 20.0).spacing(5.0);
    let col_w = c.line_height(&lunar);
    let col_x = (left + num_w + 10.0 + col_w / 2.0).min(right - col_w / 2.0);
    let col_h = c.text_vertical(col_x, y + 14.0, &lunar, PAPER, &m.lunar);
    y += c.line_height(&num).max(14.0 + col_h) + 6.0;

    let wk = Style::new(Face::SerifBlack, 32.0);
    c.text(left, y, &wk, PAPER, &m.weekday);

    let line = Style::new(Face::SansBold, 18.0);
    let lh = c.line_height(&line);
    let box_h = (3.0 + 8.0 + lh * 2.0 + 2.0 + 8.0 + 3.0).round();
    let top = bottom - box_h;
    c.border(
        left as i32,
        top as i32,
        (right - left) as i32,
        box_h as i32,
        3,
        PAPER,
    );
    let inner_w = right - left - 30.0;
    for (i, text) in m.countdowns.iter().enumerate() {
        let s = c.fit(line, text, inner_w, 12.0);
        c.text(
            left + 15.0,
            top + 11.0 + i as f32 * (lh + 2.0),
            &s,
            PAPER,
            text,
        );
    }
}

/// Blue panel, x 288..600, y 8..276, padding 20 22.
fn weather_panel(c: &mut Canvas, m: &DashboardModel) {
    c.fill(288, 8, 312, 268, Ink::Blue);
    let (left, right, top, bottom) = (310.0, 578.0, 28.0, 256.0);
    let fg = PAPER;

    let head = Style::new(Face::SansBold, 17.0);
    c.text(left, top, &head, fg, &m.header);
    c.text_right(right, top, &head, fg, &m.updated);

    let row = top + c.line_height(&head) + 14.0;
    c.icon(m.icon.svg(), left as i32, row as i32, 64, fg);
    let cond = c.fit(
        Style::new(Face::SerifBlack, 28.0),
        &m.condition,
        right - left - 76.0,
        18.0,
    );
    let ch = c.line_height(&cond);
    c.text(left + 76.0, row + 32.0 - ch / 2.0, &cond, fg, &m.condition);

    let det = c.fit(
        Style::new(Face::SansBold, 16.0),
        &m.detail,
        right - left,
        12.0,
    );
    let det_top = bottom - c.line_height(&det);
    c.text(left, det_top, &det, fg, &m.detail);

    if let Some((lo, hi)) = m.temps {
        let (a, b) = (format!("{lo}°"), format!("{hi}°"));
        let mut big = Style::new(Face::SerifBlack, 60.0)
            .spacing(-1.0)
            .line_height(1.0);
        let mut slash = Style::new(Face::SerifBlack, 36.0).spacing(-1.0);
        let width = |c: &Canvas, big: &Style, slash: &Style| {
            c.measure(big, &a) + c.measure(big, &b) + c.measure(slash, "/") + 12.0
        };
        while width(c, &big, &slash) > right - left && big.size > 36.0 {
            big.size -= 2.0;
            slash.size = big.size * 0.6;
        }
        let t_top = det_top - 8.0 - c.line_height(&big);
        let baseline = c.baseline(t_top, &big);
        let mut x = left + c.text(left, t_top, &big, fg, &a) + 6.0;
        x += c.text_on_baseline(x, baseline, &slash, fg, "/") + 6.0;
        c.text(x, t_top, &big, fg, &b);
    }
}

/// Yellow card, x 608..792, y 8..276, padding 20.
fn delta_card(c: &mut Canvas, m: &DashboardModel) {
    c.fill(608, 8, 184, 268, Ink::Yellow);
    let (left, right, top, bottom) = (628.0, 772.0, 28.0, 256.0);
    let fg = Ink::Black;
    c.text(
        left,
        top,
        &Style::new(Face::SansBold, 20.0),
        fg,
        &m.delta.label,
    );
    let cap = Style::new(Face::SansBold, 16.0);
    let cap_top = bottom - c.line_height(&cap);
    c.text(left, cap_top, &cap, fg, &m.delta.caption);
    // The serif arrows are hairline; draw them in the black sans weight.
    let (arrow, rest) = match m.delta.value.chars().next() {
        Some(a @ ('↑' | '↓')) => (Some(a.to_string()), &m.delta.value[a.len_utf8()..]),
        _ => (None, m.delta.value.as_str()),
    };
    let mut val = Style::new(Face::SerifBlack, 64.0).line_height(1.0);
    let mut arw = Style::new(Face::SansBlack, 56.0).spacing(-4.0);
    let width = |c: &Canvas, v: &Style, a: &Style| {
        c.measure(v, rest) + arrow.as_ref().map_or(0.0, |s| c.measure(a, s))
    };
    while width(c, &val, &arw) > right - left && val.size > 32.0 {
        val.size -= 2.0;
        arw.size = val.size * 0.875;
    }
    let top = cap_top - 8.0 - c.line_height(&val);
    let baseline = c.baseline(top, &val);
    let mut x = left;
    if let Some(a) = &arrow {
        x += c.text_on_baseline(x, baseline, &arw, fg, a);
    }
    c.text(x, top, &val, fg, rest);
}

/// Advice tile, y 284..472, 248 wide. The first is solid, the second outlined;
/// a weather warning is always solid red.
fn tip_tile(c: &mut Canvas, tip: &Tip, x: i32, solid: bool) {
    let (y, w, h) = (284, 248, 188);
    let (fg, icon_ink) = match (tip.kind, solid) {
        (TipKind::Alert, _) => {
            c.fill(x, y, w, h, Ink::Red);
            (PAPER, Ink::Yellow)
        }
        (_, true) => {
            c.fill(x, y, w, h, Ink::Black);
            (PAPER, Ink::Yellow)
        }
        (_, false) => {
            c.border(x, y, w, h, 4, Ink::Black);
            (Ink::Black, Ink::Black)
        }
    };
    let svg = match tip.kind {
        TipKind::Alert => icons::ALERT,
        TipKind::Umbrella => icons::UMBRELLA,
        TipKind::NoUmbrella => icons::SUN,
        TipKind::Clothes => icons::SHIRT,
    };
    let cy = (y + h / 2) as f32;
    c.icon(svg, x + 20, (cy - 26.0) as i32, 52, icon_ink);

    let tx = x as f32 + 86.0;
    let max_w = w as f32 - 86.0 - 18.0;
    let title = c.fit(
        Style::new(Face::SerifBlack, 34.0).line_height(1.15),
        &tip.title,
        max_w,
        20.0,
    );
    let cap = c.fit(Style::new(Face::SansBold, 17.0), &tip.caption, max_w, 12.0);
    let block = c.line_height(&title) + 4.0 + c.line_height(&cap);
    let top = cy - block / 2.0;
    c.text(tx, top, &title, fg, &tip.title);
    c.text(
        tx,
        top + c.line_height(&title) + 4.0,
        &cap,
        fg,
        &tip.caption,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day(date: NaiveDate, lo: i32, hi: i32, text: &str, icon: &str, pop: u32) -> DailyForecast {
        DailyForecast {
            date,
            temp_max: hi,
            temp_min: lo,
            text_day: text.into(),
            text_night: text.into(),
            icon_day: icon.into(),
            precip_prob: Some(pop),
            precip_mm: 0.0,
            humidity: 60,
            wind_dir: "东北风".into(),
            wind_scale: "3".into(),
        }
    }

    fn input<'a>(
        target: &'a DailyForecast,
        reference: &'a DailyForecast,
        cal: &'a HolidayCalendar,
        th: &'a Thresholds,
        warnings: &'a [Warning],
    ) -> DashboardInput<'a> {
        let now = target.date.and_hms_opt(7, 0, 0).unwrap_or_default();
        DashboardInput {
            view: View::Morning,
            now,
            city: "深圳",
            target: Some(target),
            reference: Some(reference),
            fetched_at: Some(now - chrono::Duration::minutes(5)),
            air: Some("优"),
            warnings,
            holidays: cal,
            thresholds: th,
        }
    }

    #[test]
    fn cooler_rainy_day_asks_for_umbrella_and_coat() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap_or_default();
        let today = day(d, 21, 26, "中雨", "306", 70);
        let yesterday = day(d - chrono::Duration::days(1), 25, 31, "晴", "100", 10);
        let (cal, th) = (HolidayCalendar::new(), Thresholds::default());
        let m = build_model(&input(&today, &yesterday, &cal, &th, &[]));
        assert_eq!(m.delta.value, "↓5°");
        assert_eq!(m.tips[0].title, "带伞出门");
        assert_eq!(m.tips[1].title, "加件外套");
        assert_eq!(
            m.countdowns,
            ["距霜降 8 天".to_string(), "距元旦 78 天".to_string()]
        );
        assert_eq!(m.updated, "07:00 更新");
        assert_eq!(m.icon, WeatherIcon::Rain);
    }

    #[test]
    fn warning_takes_first_tile_and_small_changes_are_quiet() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap_or_default();
        let today = day(d, 27, 33, "雷阵雨", "302", 20);
        let yesterday = day(d - chrono::Duration::days(1), 26, 32, "多云", "101", 10);
        let (cal, th) = (HolidayCalendar::new(), Thresholds::default());
        let w = [Warning {
            id: "1".into(),
            title: "".into(),
            type_name: "暴雨".into(),
            color: "Yellow".into(),
            pub_time: "06:40".into(),
        }];
        let m = build_model(&input(&today, &yesterday, &cal, &th, &w));
        assert_eq!(m.tips[0].kind, TipKind::Alert);
        assert_eq!(m.tips[0].title, "暴雨黄色预警");
        assert_eq!(m.tips[1].title, "不用带伞");
        assert_eq!(m.delta.value, "↑1°");
    }

    #[test]
    fn evening_view_after_midnight_still_means_the_same_tomorrow() {
        let at = |d: u32, h: u32| {
            NaiveDate::from_ymd_opt(2026, 10, d)
                .and_then(|x| x.and_hms_opt(h, 30, 0))
                .unwrap_or_default()
        };
        let oct16 = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap_or_default();
        assert_eq!(target_date(View::Evening, at(15, 22)), oct16);
        assert_eq!(target_date(View::Evening, at(16, 1)), oct16);
        assert_eq!(target_date(View::Evening, at(16, 7)), oct16);
        assert_eq!(target_date(View::Morning, at(16, 7)), oct16);
    }

    #[test]
    fn stale_data_is_labelled() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap_or_default();
        let today = day(d, 21, 26, "阴", "104", 0);
        let (cal, th) = (HolidayCalendar::new(), Thresholds::default());
        let mut i = input(&today, &today, &cal, &th, &[]);
        i.fetched_at = d.pred_opt().and_then(|p| p.and_hms_opt(21, 0, 0));
        let m = build_model(&i);
        assert!(m.stale);
        assert_eq!(m.updated, "更新于 10/14 21:00");
    }
}
