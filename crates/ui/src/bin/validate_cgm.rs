//! Local-only offline validator for personal Abbott Lingo and commercial CGM/BGM exports.
//!
//! Reads a local `.csv`, Apple Health `export.xml`, or Health Connect / `dabney.health_bridge.v1`
//! `.json` file completely offline, validates timestamp chronology, sampling cadence, sensor
//! bounds (`<55` / `>200 mg/dL`), `"Out of range"` status rows, and ATTD wear completeness,
//! and prints ADA 5-band TIR, GMI, GRI, CV%, MAGE, hourly AGP coverage, circadian blocks,
//! sustained ATTD episodes, 15-min rate of change, wear sessions/gaps, daily tables,
//! 14-day period comparisons, and optional local-only Markdown clinician briefs.
//!
//! Never uploads data or writes personal data into the repository.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use ui::mental_health_cgm::{
    export_health_bridge_json, monitor_hint_from_filename, nominal_window_hours_for_trace,
    parse_commercial_glucose_input, slice_readings_window,
    summarize_commercial_glucose_with_window, ClinicalGlucoseSummary, GlucoseMonitor,
    GlucoseWindow, ValidationLevel, SAMPLE_APPLE_HEALTH_XML, SAMPLE_HEALTH_BRIDGE_JSON,
};
use ui::mental_health_cgm_alerts::{
    evaluate_window_range_alerts, format_glucose_reading_honestly, AlertDispatchState,
    GlucoseAlertConfig, RangeBreachEpisode, WindowRangeAlertSummary,
};
use ui::mental_health_cgm_analytics::{
    analyze_glucose_trace, AdvancedGlucoseAnalytics, EpisodeKind, GlycemicEpisode,
};
use ui::mental_health_episodes::summarize_episode_cadence;
use ui::mental_health_lingo::SAMPLE_LINGO_CSV;
use ui::mental_health_support::render_clinician_brief_markdown_with_analytics;

fn print_usage() {
    eprintln!(
        "\
Dabney.moe Offline CGM & Abbott Lingo Validator (100% local, zero network)

Usage:
  cargo run -p ui --features ssr --bin validate-cgm -- <path-to-export.(csv|xml|json)> [options]
  cargo run -p ui --features ssr --bin validate-cgm -- --sample-lingo
  cargo run -p ui --features ssr --bin validate-cgm -- --sample-health-connect
  cargo run -p ui --features ssr --bin validate-cgm -- --sample-apple-health

Options:
  --monitor <slug>                       Override monitor detection (e.g. abbott-lingo, dexcom-clarity)
  --window <24h|3d|7d|14d|30d|90d|all>   Compute clinical summary over trailing window (default: all)
  --daily                                Print local calendar-day summary table
  --episodes                             Print sustained ATTD episodes (>=15m) & 15-min velocity (RoC)
  --sessions                             Print continuous sensor wear sessions & gaps (>=2h)
  --compare                              Print trailing 14-day vs prior 14-day comparison table
  --alerts                               Print target-range alert evaluation, custom TIR/TBR/TAR & breaches
  --target-low <mg/dL>                   Override target low alert threshold (default: 70)
  --target-high <mg/dL>                  Override target high alert threshold (default: 140)
  --urgent-low <mg/dL>                   Override urgent low alert threshold (default: 55)
  --urgent-high <mg/dL>                  Override urgent high alert threshold (default: 180)
  --brief <path>                         Write local-only clinician Markdown brief (keep outside Git)
  --export-bridge-json <path>            Write normalized dabney.health_bridge.v1 JSON locally (keep outside Git)
  --json                                 Emit machine-readable JSON diagnostics, summary & analytics to stdout
  -h, --help                             Show this help message"
    );
}

fn parse_window_arg(raw: &str) -> Option<GlucoseWindow> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "24h" | "1d" => Some(GlucoseWindow::Hours24),
        "3d" => Some(GlucoseWindow::Days3),
        "7d" => Some(GlucoseWindow::Days7),
        "14d" => Some(GlucoseWindow::Days14),
        "30d" => Some(GlucoseWindow::Days30),
        "90d" => Some(GlucoseWindow::Days90),
        "all" => Some(GlucoseWindow::All),
        _ => None,
    }
}

fn format_duration_minutes(minutes: f64) -> String {
    if minutes >= 120.0 {
        format!("{:.1}h ({:.0}m)", minutes / 60.0, minutes)
    } else {
        format!("{:.0}m", minutes)
    }
}

fn saturation_caveat_text(summary: &ClinicalGlucoseSummary) -> Option<String> {
    if !summary.has_sensor_saturation() {
        return None;
    }
    let mut parts = Vec::new();
    if summary.floor_capped_readings > 0 {
        parts.push(format!(
            "floor <=54 (<55 cap): {} ({:.1}%)",
            summary.floor_capped_readings, summary.floor_capped_pct
        ));
    }
    if summary.ceiling_capped_readings > 0 {
        parts.push(format!(
            "ceiling >=201 (>200 cap): {} ({:.1}%)",
            summary.ceiling_capped_readings, summary.ceiling_capped_pct
        ));
    }
    Some(format!(
        "{} — Mean, GMI, TAR >250, and CV% (flat-line suppression) are lower bounds / clipped in this window.",
        parts.join(" | ")
    ))
}

fn print_circadian_summary(analytics: &AdvancedGlucoseAnalytics) {
    println!("\nCircadian Summary (local clock time):");
    println!(
        "  {:<10} {:<11} {:>7} {:>7} {:>7} {:>6} {:>7} {:>7} {:>7}",
        "Period", "Hours", "Rdgs", "Mean", "Med", "p95", "TIR%", "TBR%", "Ceil%"
    );
    for c in &analytics.circadian {
        if c.reading_count == 0 {
            println!(
                "  {:<10} {:<11} {:>7} {:>7} {:>7} {:>6} {:>7} {:>7} {:>7}",
                c.period.label(),
                c.period.hours_label(),
                0,
                "—",
                "—",
                "—",
                "—",
                "—",
                "—"
            );
        } else {
            println!(
                "  {:<10} {:<11} {:>7} {:>7.1} {:>7.1} {:>6.0} {:>6.1}% {:>6.1}% {:>6.1}%",
                c.period.label(),
                c.period.hours_label(),
                c.reading_count,
                c.mean_mg_dl,
                c.median_mg_dl,
                c.p95_mg_dl,
                c.tir_70_180_pct,
                c.tbr_lt_70_pct,
                c.ceiling_capped_pct,
            );
        }
    }
}

fn print_episode_counts(analytics: &AdvancedGlucoseAnalytics) {
    let ep = &analytics.episode_counts;
    println!("\nSustained Glycemic Episodes (>=15 min, ATTD):");
    println!(
        "  Level 2 / Floor Hypo (<54 / <=54): {:>4} episode(s) · total {}",
        ep.level2_hypo_count,
        format_duration_minutes(ep.level2_hypo_minutes)
    );
    println!(
        "  Level 1 Hypo (<70 mg/dL):          {:>4} episode(s) · total {}",
        ep.level1_hypo_count,
        format_duration_minutes(ep.level1_hypo_minutes)
    );
    println!(
        "  Hyperglycemia (>180 mg/dL):        {:>4} episode(s) · total {}",
        ep.hyper_gt_180_count,
        format_duration_minutes(ep.hyper_gt_180_minutes)
    );
    println!(
        "  Lingo Excursions (>140 mg/dL):     {:>4} episode(s) · total {}",
        ep.lingo_excursion_gt_140_count,
        format_duration_minutes(ep.lingo_excursion_gt_140_minutes)
    );
}

fn print_episodes_detail(analytics: &AdvancedGlucoseAnalytics) {
    let roc = &analytics.rate_of_change;
    println!("\n15-Minute Glucose Velocity (Rate of Change):");
    if roc.sample_pairs == 0 {
        println!("  Insufficient consecutive 10–25 min pairs in this window.");
    } else {
        println!(
            "  Evaluated pairs: {} · Mean |RoC|: {:.3} mg/dL/min",
            roc.sample_pairs, roc.mean_abs_roc
        );
        println!(
            "  Steepest rise / fall: {:+.2} / {:+.2} mg/dL/min",
            roc.max_rise_mg_dl_per_min, roc.max_fall_mg_dl_per_min
        );
        println!(
            "  Rapid bursts (>= +2.0 / <= -2.0 mg/dL/min): {} rise(s) · {} fall(s)",
            roc.rapid_rise_events, roc.rapid_fall_events
        );
    }

    let notable_hypos: Vec<&GlycemicEpisode> = analytics
        .episodes
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EpisodeKind::Level2HypoLt54 | EpisodeKind::Level1HypoLt70
            )
        })
        .rev()
        .take(8)
        .collect();
    if !notable_hypos.is_empty() {
        println!("\nNotable Recent Hypo Episodes (up to 8 most recent):");
        println!(
            "  {:<26} {:<26} {:<26} {:>9} {:>7} {:>6}",
            "Kind", "Start", "End", "Duration", "Nadir", "Cap"
        );
        for e in notable_hypos {
            println!(
                "  {:<26} {:<26} {:<26} {:>9} {:>7.0} {:>6}",
                e.kind.label(),
                e.start_timestamp,
                e.end_timestamp,
                format_duration_minutes(e.duration_minutes),
                e.extreme_mg_dl,
                if e.sensor_capped { "floor" } else { "—" }
            );
        }
    }

    let notable_hypers: Vec<&GlycemicEpisode> = analytics
        .episodes
        .iter()
        .filter(|e| e.kind == EpisodeKind::HyperGt180)
        .rev()
        .take(8)
        .collect();
    if !notable_hypers.is_empty() {
        println!("\nNotable Recent Hyper (>180) Episodes (up to 8 most recent):");
        println!(
            "  {:<26} {:<26} {:<26} {:>9} {:>7} {:>6}",
            "Kind", "Start", "End", "Duration", "Peak", "Cap"
        );
        for e in notable_hypers {
            println!(
                "  {:<26} {:<26} {:<26} {:>9} {:>7.0} {:>6}",
                e.kind.label(),
                e.start_timestamp,
                e.end_timestamp,
                format_duration_minutes(e.duration_minutes),
                e.extreme_mg_dl,
                if e.sensor_capped { "ceil" } else { "—" }
            );
        }
    }
}

fn print_daily_table(analytics: &AdvancedGlucoseAnalytics) {
    println!(
        "\nDaily Glucose Breakdown ({} local calendar day(s)):",
        analytics.daily.len()
    );
    if analytics.daily.is_empty() {
        println!("  No daily rows available.");
        return;
    }
    println!(
        "  {:<12} {:>5} {:>6} {:>6} {:>9} {:>7} {:>7} {:>7} {:>7}",
        "Date", "Rdgs", "Wear%", "Mean", "Min-Max", "TIR%", "Tight%", "TBR%", "Ceil%"
    );
    for d in &analytics.daily {
        let range = format!("{:.0}-{:.0}", d.min_mg_dl, d.max_mg_dl);
        println!(
            "  {:<12} {:>5} {:>5.0}% {:>6.1} {:>9} {:>6.1}% {:>6.1}% {:>6.1}% {:>6.1}%",
            d.date,
            d.reading_count,
            d.active_wear_pct,
            d.mean_mg_dl,
            range,
            d.tir_70_180_pct,
            d.titr_70_140_pct,
            d.tbr_lt_70_pct,
            d.ceiling_capped_pct,
        );
    }
}

fn print_sessions_and_gaps(analytics: &AdvancedGlucoseAnalytics) {
    println!(
        "\nContinuous Sensor Wear Sessions ({} session(s), split on gaps >=2.0h):",
        analytics.sessions.len()
    );
    println!(
        "  {:>3} {:<26} {:<26} {:>8} {:>6} {:>6} {:>6} {:>7} {:>7}",
        "#", "Start", "End", "Span", "Rdgs", "Wear%", "Mean", "TIR%", "Ceil%"
    );
    for s in &analytics.sessions {
        let span = if s.duration_hours >= 48.0 {
            format!("{:.1}d", s.duration_hours / 24.0)
        } else {
            format!("{:.1}h", s.duration_hours)
        };
        println!(
            "  {:>3} {:<26} {:<26} {:>8} {:>6} {:>5.0}% {:>6.1} {:>6.1}% {:>6.1}%",
            s.session_index,
            s.start_timestamp,
            s.end_timestamp,
            span,
            s.reading_count,
            s.active_wear_pct,
            s.mean_mg_dl,
            s.tir_70_180_pct,
            s.ceiling_capped_pct,
        );
    }

    if analytics.gaps.is_empty() {
        println!("\nSensor Gaps (>=2.0h): none in this window.");
    } else {
        println!("\nSensor Gaps >=2.0h ({} gap(s)):", analytics.gaps.len());
        println!("  {:<26} {:<26} {:>12}", "Gap Start", "Gap End", "Duration");
        for g in &analytics.gaps {
            let dur = if g.duration_hours >= 48.0 {
                format!("{:.1}h ({:.1}d)", g.duration_hours, g.duration_hours / 24.0)
            } else {
                format!("{:.1}h", g.duration_hours)
            };
            println!(
                "  {:<26} {:<26} {:>12}",
                g.start_timestamp, g.end_timestamp, dur
            );
        }
    }
}

fn print_period_comparison(analytics: &AdvancedGlucoseAnalytics) {
    println!("\n14-Day vs Prior 14-Day Period Comparison:");
    let Some(cmp) = &analytics.period_comparison else {
        println!("  Not enough history (requires at least ~20 days across the loaded trace).");
        return;
    };
    println!(
        "  Recent: {:.1}d ({} readings) | Prior: {:.1}d ({} readings)",
        cmp.recent_days, cmp.recent_readings, cmp.prior_days, cmp.prior_readings
    );
    if cmp.prior_days < cmp.recent_days - 1.0 {
        println!(
            "  Note:   Prior window covers only {:.1}d active span vs recent {:.1}d due to sensor gaps.",
            cmp.prior_days, cmp.recent_days
        );
    }
    println!(
        "  {:<22} {:>12} {:>12} {:>12}",
        "Metric", "Prior 14d", "Recent 14d", "Delta"
    );
    println!(
        "  {:<22} {:>10.1}   {:>10.1}   {:>+10.1} mg/dL",
        "Mean Glucose", cmp.prior_mean_mg_dl, cmp.recent_mean_mg_dl, cmp.delta_mean_mg_dl
    );
    println!(
        "  {:<22} {:>10.2}%  {:>10.2}%  {:>+10.2}%",
        "GMI (est. A1C)", cmp.prior_gmi_pct, cmp.recent_gmi_pct, cmp.delta_gmi_pct
    );
    println!(
        "  {:<22} {:>10.1}%  {:>10.1}%  {:>+10.1}pp",
        "TIR (70–180)", cmp.prior_tir_pct, cmp.recent_tir_pct, cmp.delta_tir_pct
    );
    println!(
        "  {:<22} {:>10.1}%  {:>10.1}%  {:>+10.1}pp",
        "Tight TIR (70–140)", cmp.prior_titr_pct, cmp.recent_titr_pct, cmp.delta_titr_pct
    );
    println!(
        "  {:<22} {:>10.1}%  {:>10.1}%  {:>+10.1}pp",
        "TBR (<70)", cmp.prior_tbr_pct, cmp.recent_tbr_pct, cmp.delta_tbr_pct
    );
    println!(
        "  {:<22} {:>10.1}%  {:>10.1}%  {:>+10.1}pp",
        "Ceiling-Capped (>=201)",
        cmp.prior_ceiling_capped_pct,
        cmp.recent_ceiling_capped_pct,
        cmp.delta_ceiling_capped_pct
    );
}

fn parse_threshold_arg(raw: &str) -> Option<f64> {
    let val = raw.trim().parse::<f64>().ok()?;
    val.is_finite().then_some(val)
}

fn build_cli_alert_config(
    target_low: Option<f64>,
    target_high: Option<f64>,
    urgent_low: Option<f64>,
    urgent_high: Option<f64>,
) -> GlucoseAlertConfig {
    let mut cfg = GlucoseAlertConfig::default();
    if let Some(low) = target_low {
        cfg.low_mg_dl = low;
    }
    if let Some(high) = target_high {
        cfg.high_mg_dl = high;
    }
    if let Some(ulow) = urgent_low {
        cfg.urgent_low_mg_dl = ulow;
    } else if cfg.low_mg_dl <= cfg.urgent_low_mg_dl {
        cfg.urgent_low_mg_dl = (cfg.low_mg_dl - 15.0).max(45.0);
    }
    if let Some(uhigh) = urgent_high {
        cfg.urgent_high_mg_dl = uhigh;
    } else if cfg.high_mg_dl >= cfg.urgent_high_mg_dl {
        cfg.urgent_high_mg_dl = if (cfg.high_mg_dl - 180.0).abs() < 1e-6 {
            250.0
        } else {
            (cfg.high_mg_dl + 40.0).min(350.0)
        };
    }
    cfg.sanitized()
}

fn print_range_alerts(summary: &WindowRangeAlertSummary) {
    println!(
        "\nTarget-Range Alerts ({:.0}–{:.0} mg/dL · urgent <{:.0} / >{:.0} mg/dL):",
        summary.target_low_mg_dl,
        summary.target_high_mg_dl,
        summary.urgent_low_mg_dl,
        summary.urgent_high_mg_dl,
    );
    println!(
        "  Custom TIR / TBR / TAR: {:.1}% in-target · {:.1}% below (<{:.0}) · {:.1}% above (>{:.0})",
        summary.time_in_target_pct,
        summary.time_below_target_pct,
        summary.target_low_mg_dl,
        summary.time_above_target_pct,
        summary.target_high_mg_dl,
    );
    println!(
        "  Urgent readings:        {} urgent low (<{:.0}) · {} urgent high (>{:.0})",
        summary.urgent_low_readings,
        summary.urgent_low_mg_dl,
        summary.urgent_high_readings,
        summary.urgent_high_mg_dl,
    );

    let latest = &summary.latest;
    if let Some(val) = latest.latest_reading_mg_dl {
        let formatted = format_glucose_reading_honestly(val, latest.latest_bound);
        let status = if latest.in_target_range {
            "In target range"
        } else {
            "OUT OF TARGET RANGE"
        };
        println!(
            "  Latest reading:         {} at {} — {}",
            formatted,
            latest.latest_timestamp.as_deref().unwrap_or("—"),
            status
        );
    }

    if let Some(alert) = &latest.active_alert {
        println!(
            "  Active alert:           [{}] {}",
            alert.severity.slug().to_ascii_uppercase(),
            alert.in_app_title
        );
        println!("  Detail:                 {}", alert.in_app_detail);
        println!(
            "  Lock-screen copy:       {} — {}",
            alert.os_title, alert.os_body
        );
        println!(
            "  OS dispatch:            {}",
            if latest.should_dispatch_os {
                "would notify".to_string()
            } else {
                format!(
                    "suppressed ({})",
                    latest
                        .suppressed_reason
                        .as_deref()
                        .unwrap_or("cooldown/dedup")
                )
            }
        );
    } else if let Some(reason) = &latest.suppressed_reason {
        println!("  Active alert:           None ({reason})");
    } else {
        println!("  Active alert:           None (latest reading within target range)");
    }

    if summary.breaches.is_empty() {
        println!("  Breach episodes:        0 in this window.");
    } else {
        let recent: Vec<&RangeBreachEpisode> = summary.breaches.iter().rev().take(8).collect();
        println!(
            "  Breach episodes:        {} total in window (up to 8 most recent):",
            summary.breaches.len()
        );
        println!(
            "    {:<20} {:<26} {:<26} {:>9} {:>5} {:>7} {:>6}",
            "Severity", "Start", "End", "Duration", "Rdgs", "Extreme", "Cap"
        );
        for b in recent {
            let cap = if b.sensor_capped {
                if b.severity.is_low() {
                    "floor"
                } else {
                    "ceil"
                }
            } else {
                "—"
            };
            println!(
                "    {:<20} {:<26} {:<26} {:>9} {:>5} {:>7.0} {:>6}",
                b.severity.label(),
                b.start_timestamp,
                b.end_timestamp,
                format_duration_minutes(b.duration_minutes),
                b.reading_count,
                b.extreme_mg_dl,
                cap
            );
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return if args.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    let mut input_path: Option<String> = None;
    let mut built_in_sample: Option<(&str, &str)> = None;
    let mut monitor_override: Option<GlucoseMonitor> = None;
    let mut window = GlucoseWindow::All;
    let mut export_bridge_path: Option<String> = None;
    let mut brief_path: Option<String> = None;
    let mut show_daily = false;
    let mut show_episodes = false;
    let mut show_sessions = false;
    let mut show_compare = false;
    let mut show_alerts = false;
    let mut target_low_override: Option<f64> = None;
    let mut target_high_override: Option<f64> = None;
    let mut urgent_low_override: Option<f64> = None;
    let mut urgent_high_override: Option<f64> = None;
    let mut json_output = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--sample-lingo" => {
                built_in_sample = Some(("synthetic-lingo.csv", SAMPLE_LINGO_CSV));
                i += 1;
            }
            "--sample-health-connect" => {
                built_in_sample =
                    Some(("synthetic-health-connect.json", SAMPLE_HEALTH_BRIDGE_JSON));
                i += 1;
            }
            "--sample-apple-health" => {
                built_in_sample = Some(("synthetic-apple-health.xml", SAMPLE_APPLE_HEALTH_XML));
                i += 1;
            }
            "--monitor" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --monitor");
                    return ExitCode::from(2);
                };
                match GlucoseMonitor::from_slug(val) {
                    Some(m) => monitor_override = Some(m),
                    None => {
                        eprintln!("Unknown monitor slug `{val}`.");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--window" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --window");
                    return ExitCode::from(2);
                };
                match parse_window_arg(val) {
                    Some(w) => window = w,
                    None => {
                        eprintln!(
                            "Invalid --window `{val}` (expected 24h, 3d, 7d, 14d, 30d, 90d, or all)."
                        );
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--daily" => {
                show_daily = true;
                i += 1;
            }
            "--episodes" => {
                show_episodes = true;
                i += 1;
            }
            "--sessions" => {
                show_sessions = true;
                i += 1;
            }
            "--compare" => {
                show_compare = true;
                i += 1;
            }
            "--alerts" => {
                show_alerts = true;
                i += 1;
            }
            "--target-low" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --target-low");
                    return ExitCode::from(2);
                };
                match parse_threshold_arg(val) {
                    Some(v) => target_low_override = Some(v),
                    None => {
                        eprintln!("Invalid --target-low `{val}` (expected numeric mg/dL).");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--target-high" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --target-high");
                    return ExitCode::from(2);
                };
                match parse_threshold_arg(val) {
                    Some(v) => target_high_override = Some(v),
                    None => {
                        eprintln!("Invalid --target-high `{val}` (expected numeric mg/dL).");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--urgent-low" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --urgent-low");
                    return ExitCode::from(2);
                };
                match parse_threshold_arg(val) {
                    Some(v) => urgent_low_override = Some(v),
                    None => {
                        eprintln!("Invalid --urgent-low `{val}` (expected numeric mg/dL).");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--urgent-high" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --urgent-high");
                    return ExitCode::from(2);
                };
                match parse_threshold_arg(val) {
                    Some(v) => urgent_high_override = Some(v),
                    None => {
                        eprintln!("Invalid --urgent-high `{val}` (expected numeric mg/dL).");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--brief" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --brief");
                    return ExitCode::from(2);
                };
                brief_path = Some(val.clone());
                i += 2;
            }
            "--export-bridge-json" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Missing value for --export-bridge-json");
                    return ExitCode::from(2);
                };
                export_bridge_path = Some(val.clone());
                i += 2;
            }
            "--json" => {
                json_output = true;
                i += 1;
            }
            flag if flag.starts_with('-') => {
                eprintln!("Unknown option `{flag}`");
                print_usage();
                return ExitCode::from(2);
            }
            path => {
                input_path = Some(path.to_string());
                i += 1;
            }
        }
    }

    let (source_name, payload) = if let Some((name, sample)) = built_in_sample {
        (name.to_string(), sample.to_string())
    } else if let Some(path) = input_path {
        match fs::read_to_string(&path) {
            Ok(text) => (
                Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&path)
                    .to_string(),
                text,
            ),
            Err(err) => {
                eprintln!("Failed to read `{path}`: {err}");
                return ExitCode::from(1);
            }
        }
    } else {
        eprintln!("Provide a local glucose export file path or a --sample-* flag.");
        print_usage();
        return ExitCode::from(2);
    };

    let effective_hint = monitor_override.or_else(|| monitor_hint_from_filename(&source_name));
    let import = match parse_commercial_glucose_input(&payload, effective_hint) {
        Ok(imp) => imp,
        Err(err) => {
            eprintln!("Validation FAILED for `{source_name}`: {err}");
            return ExitCode::from(1);
        }
    };

    if let Some(out_path) = export_bridge_path {
        let bridge_json = export_health_bridge_json(&import, "local_offline_export");
        if let Err(err) = fs::write(&out_path, bridge_json) {
            eprintln!("Failed to write normalized bridge JSON `{out_path}`: {err}");
            return ExitCode::from(1);
        }
        if !json_output {
            eprintln!(
                "Wrote normalized dabney.health_bridge.v1 JSON to `{out_path}` (keep personal exports outside Git)."
            );
        }
    }

    let windowed = slice_readings_window(&import.readings, window);
    let nominal_window_hours = nominal_window_hours_for_trace(&import.readings, window);
    let Some(summary) = summarize_commercial_glucose_with_window(
        &windowed,
        import.monitor,
        import.source_unit,
        import.method,
        nominal_window_hours,
    ) else {
        eprintln!(
            "No readings remained after applying window `{}`.",
            window.label()
        );
        return ExitCode::from(1);
    };

    let analytics = analyze_glucose_trace(
        &windowed,
        import.monitor,
        import.method,
        Some(&import.readings),
    );
    let sat_caveat = saturation_caveat_text(&summary);
    let include_range_alerts = show_alerts
        || target_low_override.is_some()
        || target_high_override.is_some()
        || urgent_low_override.is_some()
        || urgent_high_override.is_some();
    let range_alerts = include_range_alerts.then(|| {
        let alert_cfg = build_cli_alert_config(
            target_low_override,
            target_high_override,
            urgent_low_override,
            urgent_high_override,
        );
        evaluate_window_range_alerts(&windowed, &alert_cfg, &AlertDispatchState::default())
    });

    if let Some(out_path) = brief_path {
        // Pass neutral baseline sleep (7.5h) to cadence summary so absent self-reported sleep does
        // not falsely trigger Early Watch, while passing 0.0 to the brief renderer so Section 1
        // reports "Not recorded".
        let empty_cadence = summarize_episode_cadence(&[], None, 7.5);
        let brief_md = render_clinician_brief_markdown_with_analytics(
            Some(&summary),
            Some(&analytics),
            &[],
            &empty_cadence,
            None,
            0.0,
            None,
        );
        if let Err(err) = fs::write(&out_path, brief_md) {
            eprintln!("Failed to write clinician brief Markdown `{out_path}`: {err}");
            return ExitCode::from(1);
        }
        if !json_output {
            eprintln!(
                "Wrote local-only clinician Markdown brief to `{out_path}` (keep personal exports outside Git)."
            );
        }
    }

    if json_output {
        let mut report = serde_json::json!({
            "source": source_name,
            "monitor": import.monitor.display_name(),
            "monitor_slug": import.monitor.slug(),
            "source_unit": import.source_unit.label(),
            "method": import.method.label(),
            "window": window.label(),
            "diagnostics": import.diagnostics,
            "summary": {
                "reading_count": summary.reading_count,
                "mean_mg_dl": summary.mean_mg_dl,
                "median_mg_dl": summary.median_mg_dl,
                "min_mg_dl": summary.min_mg_dl,
                "max_mg_dl": summary.max_mg_dl,
                "std_dev_mg_dl": summary.std_dev_mg_dl,
                "cv_percent": summary.cv_percent,
                "gmi_percent": summary.gmi_percent,
                "gri_score": summary.gri_score,
                "gri_zone": summary.gri_zone.label(),
                "mage_mg_dl": summary.mage_mg_dl,
                "tbr_very_low_lt_54_pct": summary.tbr_very_low_lt_54_pct,
                "tbr_low_54_69_pct": summary.tbr_low_54_69_pct,
                "titr_70_140_pct": summary.titr_70_140_pct,
                "tir_70_180_pct": summary.tir_70_180_pct,
                "tar_high_181_250_pct": summary.tar_high_181_250_pct,
                "tar_very_high_gt_250_pct": summary.tar_very_high_gt_250_pct,
                "floor_capped_readings": summary.floor_capped_readings,
                "ceiling_capped_readings": summary.ceiling_capped_readings,
                "floor_capped_pct": summary.floor_capped_pct,
                "ceiling_capped_pct": summary.ceiling_capped_pct,
                "has_sensor_saturation": summary.has_sensor_saturation(),
                "saturation_caveat": sat_caveat,
                "estimated_active_wear_pct": summary.estimated_active_wear_pct,
                "agp_populated_hours": summary.agp_hourly.iter().filter(|b| b.count > 0).count(),
            },
            "analytics": analytics,
        });
        if let (Some(alerts), Some(obj)) = (&range_alerts, report.as_object_mut()) {
            obj.insert(
                "range_alerts".to_string(),
                serde_json::to_value(alerts).unwrap_or(serde_json::Value::Null),
            );
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
        );
        return ExitCode::SUCCESS;
    }

    let diag = &import.diagnostics;
    println!("=== Dabney.moe Offline CGM Validation Report ===");
    println!("Local-only validation (zero network · never commit personal exports to Git)");
    println!("Source:         {source_name}");
    println!("Format:         {}", diag.format.label());
    println!(
        "Monitor:        {} ({} · {})",
        import.monitor.display_name(),
        import.method.label(),
        import.source_unit.label()
    );
    if !diag.detected_sources.is_empty() {
        println!("Data origins:   {}", diag.detected_sources.join(", "));
    }
    println!(
        "Date range:     {} -> {}",
        diag.first_timestamp.as_deref().unwrap_or("—"),
        diag.last_timestamp.as_deref().unwrap_or("—")
    );
    println!(
        "Counts:         {} parsed / {} rows seen ({} non-glucose, {} out-of-range, {} invalid skipped, {} duplicates dropped, {} sentinels)",
        diag.parsed_readings,
        diag.total_rows_seen,
        diag.skipped_non_glucose_rows,
        diag.out_of_range_rows,
        diag.skipped_invalid_rows,
        diag.duplicate_timestamps_dropped,
        diag.bounded_sentinels
    );
    println!(
        "Span & Cadence: {:.1}h ({:.1}d) · median {} · max gap {}",
        diag.span_hours,
        diag.span_hours / 24.0,
        diag.median_interval_minutes
            .map(|m| format!("{m:.1} min"))
            .unwrap_or_else(|| "—".to_string()),
        diag.longest_gap_minutes
            .map(|g| format!("{:.1} h", g / 60.0))
            .unwrap_or_else(|| "—".to_string()),
    );
    println!("\nValidation checks:");
    for chk in &diag.checks {
        let tag = match chk.level {
            ValidationLevel::Pass => "PASS",
            ValidationLevel::Warn => "WARN",
            ValidationLevel::Info => "INFO",
        };
        println!("  [{tag}] {}: {}", chk.name, chk.detail);
    }
    if !diag.sample_warnings.is_empty() {
        println!("\nSample row notices:");
        for w in &diag.sample_warnings {
            println!("  - {w}");
        }
    }

    println!(
        "\nADA / ATTD Summary (window: {}, {} readings):",
        window.label(),
        summary.reading_count
    );
    println!(
        "  Mean / Median: {:.1} / {:.1} mg/dL (range {:.0}–{:.0} mg/dL)",
        summary.mean_mg_dl, summary.median_mg_dl, summary.min_mg_dl, summary.max_mg_dl
    );
    println!(
        "  GMI / CV% / SD / MAGE: {:.2}% · {:.1}% (target <36%) · {:.1} mg/dL · {:.1} mg/dL",
        summary.gmi_percent, summary.cv_percent, summary.std_dev_mg_dl, summary.mage_mg_dl
    );
    println!(
        "  GRI:           {:.1} / 100 ({})",
        summary.gri_score,
        summary.gri_zone.label()
    );
    println!(
        "  5-Band TIR:    <54: {:.1}% | 54–69: {:.1}% | 70–140 Tight: {:.1}% | 70–180: {:.1}% | 181–250: {:.1}% | >250: {:.1}%",
        summary.tbr_very_low_lt_54_pct,
        summary.tbr_low_54_69_pct,
        summary.titr_70_140_pct,
        summary.tir_70_180_pct,
        summary.tar_high_181_250_pct,
        summary.tar_very_high_gt_250_pct
    );
    println!(
        "  Wear / AGP:    {:.1}% active wear · {}/24 clock hours populated · {} spikes >140 mg/dL",
        summary.estimated_active_wear_pct,
        summary.agp_hourly.iter().filter(|b| b.count > 0).count(),
        summary.spike_readings_gt_140
    );
    if let Some(caveat) = &sat_caveat {
        println!("  Saturation:    {caveat}");
    }

    print_circadian_summary(&analytics);
    print_episode_counts(&analytics);

    if let Some(alerts) = &range_alerts {
        print_range_alerts(alerts);
    }
    if show_episodes {
        print_episodes_detail(&analytics);
    }
    if show_compare {
        print_period_comparison(&analytics);
    }
    if show_sessions {
        print_sessions_and_gaps(&analytics);
    }
    if show_daily {
        print_daily_table(&analytics);
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_windows_including_30d_and_90d() {
        assert_eq!(parse_window_arg("24h"), Some(GlucoseWindow::Hours24));
        assert_eq!(parse_window_arg("1d"), Some(GlucoseWindow::Hours24));
        assert_eq!(parse_window_arg("3d"), Some(GlucoseWindow::Days3));
        assert_eq!(parse_window_arg("7d"), Some(GlucoseWindow::Days7));
        assert_eq!(parse_window_arg("14d"), Some(GlucoseWindow::Days14));
        assert_eq!(parse_window_arg("30d"), Some(GlucoseWindow::Days30));
        assert_eq!(parse_window_arg("90d"), Some(GlucoseWindow::Days90));
        assert_eq!(parse_window_arg("all"), Some(GlucoseWindow::All));
        assert_eq!(parse_window_arg("365d"), None);
    }

    #[test]
    fn formats_windowed_saturation_caveat_only_when_capped() {
        let import = parse_commercial_glucose_input(SAMPLE_LINGO_CSV, None).unwrap();
        let summary = summarize_commercial_glucose_with_window(
            &import.readings,
            import.monitor,
            import.source_unit,
            import.method,
            None,
        )
        .unwrap();
        assert_eq!(
            saturation_caveat_text(&summary).is_some(),
            summary.has_sensor_saturation()
        );
    }

    #[test]
    fn cli_alert_config_builds_sanitized_thresholds_and_promotes_ada_urgent_high() {
        let default_cfg = build_cli_alert_config(None, None, None, None);
        assert_eq!(default_cfg.low_mg_dl, 70.0);
        assert_eq!(default_cfg.high_mg_dl, 140.0);
        assert_eq!(default_cfg.urgent_low_mg_dl, 55.0);
        assert_eq!(default_cfg.urgent_high_mg_dl, 180.0);

        let ada_high_cfg = build_cli_alert_config(Some(70.0), Some(180.0), None, None);
        assert_eq!(ada_high_cfg.low_mg_dl, 70.0);
        assert_eq!(ada_high_cfg.high_mg_dl, 180.0);
        assert_eq!(ada_high_cfg.urgent_low_mg_dl, 55.0);
        assert_eq!(ada_high_cfg.urgent_high_mg_dl, 250.0);

        let custom_cfg = build_cli_alert_config(Some(65.0), Some(150.0), Some(52.0), Some(210.0));
        assert_eq!(custom_cfg.urgent_low_mg_dl, 52.0);
        assert_eq!(custom_cfg.low_mg_dl, 65.0);
        assert_eq!(custom_cfg.high_mg_dl, 150.0);
        assert_eq!(custom_cfg.urgent_high_mg_dl, 210.0);

        assert_eq!(parse_threshold_arg("70"), Some(70.0));
        assert_eq!(parse_threshold_arg("NaN"), None);
        assert_eq!(parse_threshold_arg("abc"), None);
    }
}
