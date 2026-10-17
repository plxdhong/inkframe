//! Renders the dashboard with fixture data and the test patterns to PNGs.
//! `cargo run -p render --example preview -- <out_dir>`

use chrono::NaiveDate;
use common::calendar::HolidayCalendar;
use common::config::{PhotoConfig, Thresholds};
use common::weather::{DailyForecast, Warning};
use render::dashboard::{build_model, render, DashboardInput, View};
use render::{patterns, preview_png, Fonts, Palette};
use std::path::PathBuf;

fn day(date: NaiveDate, lo: i32, hi: i32, d: &str, n: &str, icon: &str, pop: u32) -> DailyForecast {
    DailyForecast {
        date,
        temp_max: hi,
        temp_min: lo,
        text_day: d.into(),
        text_night: n.into(),
        icon_day: icon.into(),
        precip_prob: Some(pop),
        precip_mm: 4.0,
        humidity: 85,
        wind_dir: "东北风".into(),
        wind_scale: "3".into(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/preview".into()),
    );
    std::fs::create_dir_all(&out)?;
    let fonts = Fonts::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts"))?;
    let palette = Palette::default();
    let cal = HolidayCalendar::new();
    let th = Thresholds::default();
    let d = NaiveDate::from_ymd_opt(2026, 10, 15).ok_or("date")?;
    let yesterday = day(d.pred_opt().ok_or("date")?, 25, 31, "晴", "晴", "100", 10);
    let today = day(d, 21, 26, "中雨", "阴", "306", 70);
    let tomorrow = day(
        d.succ_opt().ok_or("date")?,
        20,
        25,
        "多云",
        "多云",
        "101",
        20,
    );
    let now = d.and_hms_opt(7, 0, 0).ok_or("time")?;
    let warn = [Warning {
        id: "w".into(),
        title: String::new(),
        type_name: "暴雨".into(),
        color: "Orange".into(),
        pub_time: "06:40".into(),
    }];

    type Case<'a> = (
        &'a str,
        View,
        &'a DailyForecast,
        &'a DailyForecast,
        &'a [Warning],
        chrono::NaiveDateTime,
    );
    let cases: [Case; 3] = [
        (
            "dashboard-morning",
            View::Morning,
            &today,
            &yesterday,
            &[],
            now,
        ),
        (
            "dashboard-evening",
            View::Evening,
            &tomorrow,
            &today,
            &[],
            d.and_hms_opt(22, 30, 0).ok_or("time")?,
        ),
        (
            "dashboard-alert",
            View::Morning,
            &today,
            &yesterday,
            &warn,
            now,
        ),
    ];
    for (name, view, target, reference, warnings, now) in cases {
        let model = build_model(&DashboardInput {
            view,
            now,
            city: "深圳",
            target: Some(target),
            reference: Some(reference),
            fetched_at: Some(now - chrono::Duration::minutes(8)),
            air: Some("优"),
            warnings,
            holidays: &cal,
            thresholds: &th,
        });
        std::fs::write(
            out.join(format!("{name}.png")),
            preview_png(&render(&model, &fonts), &palette)?,
        )?;
    }
    std::fs::write(
        out.join("pattern-colors.png"),
        preview_png(&patterns::color_bars(&fonts), &palette)?,
    )?;
    std::fs::write(
        out.join("pattern-dither.png"),
        preview_png(&patterns::dither_sample(&palette), &palette)?,
    )?;
    std::fs::write(
        out.join("pattern-dither-enhanced.png"),
        preview_png(
            &patterns::dither_sample_enhanced(&palette, &PhotoConfig::default()),
            &palette,
        )?,
    )?;
    std::fs::write(
        out.join("setup.png"),
        preview_png(
            &render::setup::render(&fonts, "inkframe-3fa2", "7K3M-9QXR-2D4F"),
            &palette,
        )?,
    )?;
    println!("wrote previews to {}", out.display());
    Ok(())
}
