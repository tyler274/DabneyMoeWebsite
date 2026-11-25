//! Local-only offline validator for personal Abbott Lingo and commercial CGM/BGM exports.
//!
//! Reads a local `.csv`, Apple Health `export.xml`, or Health Connect / `dabney.health_bridge.v1`
//! `.json` file completely offline, validates timestamp chronology, sampling cadence, sensor
//! bounds (`<55` / `>200 mg/dL`), and ATTD wear completeness, and prints ADA 5-band TIR, GMI,
//! GRI, CV%, MAGE, and hourly AGP coverage.
//!
//! Never uploads data or writes personal data into the repository.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use ui::mental_health_cgm::{
    export_health_bridge_json, monitor_hint_from_filename, parse_commercial_glucose_input,
    slice_readings_window, summarize_commercial_glucose, GlucoseMonitor, GlucoseWindow,
    ValidationLevel, SAMPLE_APPLE_HEALTH_XML, SAMPLE_HEALTH_BRIDGE_JSON,
};
use ui::mental_health_lingo::SAMPLE_LINGO_CSV;

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
  --monitor <slug>              Override monitor detection (e.g. abbott-lingo, dexcom-clarity)
  --window <24h|3d|7d|14d|all>  Compute clinical summary over trailing window (default: all)
  --export-bridge-json <path>   Write normalized dabney.health_bridge.v1 JSON locally (keep outside Git)
  --json                        Emit machine-readable JSON diagnostics & summary to stdout
  -h, --help                    Show this help message"
    );
}

fn parse_window_arg(raw: &str) -> Option<GlucoseWindow> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "24h" | "1d" => Some(GlucoseWindow::Hours24),
        "3d" => Some(GlucoseWindow::Days3),
        "7d" => Some(GlucoseWindow::Days7),
        "14d" => Some(GlucoseWindow::Days14),
        "all" => Some(GlucoseWindow::All),
        _ => None,
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
                        eprintln!("Invalid --window `{val}` (expected 24h, 3d, 7d, 14d, or all).");
                        return ExitCode::from(2);
                    }
                }
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
    let Some(summary) =
        summarize_commercial_glucose(&windowed, import.monitor, import.source_unit, import.method)
    else {
        eprintln!(
            "No readings remained after applying window `{}`.",
            window.label()
        );
        return ExitCode::from(1);
    };

    if json_output {
        let report = serde_json::json!({
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
                "estimated_active_wear_pct": summary.estimated_active_wear_pct,
                "agp_populated_hours": summary.agp_hourly.iter().filter(|b| b.count > 0).count(),
            }
        });
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
        "Counts:         {} parsed / {} rows seen ({} non-glucose, {} invalid skipped, {} duplicates dropped, {} sentinels)",
        diag.parsed_readings,
        diag.total_rows_seen,
        diag.skipped_non_glucose_rows,
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

    ExitCode::SUCCESS
}
