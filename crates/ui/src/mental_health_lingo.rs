//! Pure-Rust Mental Health Tracking & Abbott Lingo Continuous Glucose Monitoring (CGM) model.
//!
//! Parses Abbott Lingo glucose CSV exports in-browser (no data leaves the device),
//! computes metabolic summary metrics (mean, SD, CV%, 70–140 mg/dL Lingo target
//! Time-in-Range, spike excursions, and Lingo Count), scores PHQ-9 and GAD-7
//! check-ins, and correlates glucose excursions with logged mood/anxiety shifts.

/// A single interstitial glucose reading from an Abbott Lingo biosensor.
#[derive(Clone, Debug, PartialEq)]
pub struct LingoReading {
    /// Timestamp label (ISO-8601 or `YYYY-MM-DD HH:MM`).
    pub timestamp: String,
    /// Relative offset in hours from the start of the series for plotting.
    pub hour_offset: f64,
    /// Interstitial glucose in `mg/dL`.
    pub glucose_mg_dl: f64,
    /// Optional Abbott Lingo Count contribution for an excursion.
    pub lingo_count: Option<f64>,
    /// Optional meal, exercise, or stressor tag.
    pub tag: Option<String>,
}

/// Severity classification for PHQ-9 (0–27) and GAD-7 (0–21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClinicalBand {
    Minimal,
    Mild,
    Moderate,
    ModeratelySevere,
    Severe,
}

impl ClinicalBand {
    pub fn label(self) -> &'static str {
        match self {
            Self::Minimal => "Minimal",
            Self::Mild => "Mild",
            Self::Moderate => "Moderate",
            Self::ModeratelySevere => "Moderately severe",
            Self::Severe => "Severe",
        }
    }

    pub fn is_elevated(self) -> bool {
        !matches!(self, Self::Minimal | Self::Mild)
    }
}

/// A structured mental health & daily state check-in.
#[derive(Clone, Debug, PartialEq)]
pub struct MentalHealthCheckIn {
    /// Timestamp label matching the CGM window (`YYYY-MM-DD HH:MM`).
    pub recorded_at: String,
    /// Relative offset in hours from the start of the CGM window.
    pub hour_offset: f64,
    /// Self-rated mood (`1` very low .. `10` thriving).
    pub mood: u8,
    /// Self-rated anxiety (`1` calm .. `10` intense).
    pub anxiety: u8,
    /// Self-rated energy (`1` depleted .. `10` energized).
    pub energy: u8,
    /// Prior night's sleep duration in hours.
    pub sleep_hours: f32,
    /// Optional PHQ-9 total score (`0..=27`).
    pub phq9: Option<u8>,
    /// Optional GAD-7 total score (`0..=21`).
    pub gad7: Option<u8>,
    /// Context note (trigger, meal, walk, therapy note, medication timing).
    pub note: String,
}

/// Summary statistics over a Lingo CGM trace.
#[derive(Clone, Debug, PartialEq)]
pub struct LingoSummary {
    pub reading_count: usize,
    pub mean_mg_dl: f64,
    pub min_mg_dl: f64,
    pub max_mg_dl: f64,
    pub std_dev_mg_dl: f64,
    pub cv_percent: f64,
    /// Percentage of readings within Abbott Lingo's `70–140 mg/dL` metabolic zone.
    pub tir_metabolic_pct: f64,
    /// Percentage of readings within standard `70–180 mg/dL` clinical range.
    pub tir_clinical_pct: f64,
    /// Number of readings above `140 mg/dL`.
    pub spike_readings: usize,
    /// Total Lingo Count (sum of explicit counts or estimated from `>140 mg/dL` excursions).
    pub total_lingo_count: f64,
}

/// Paired glucose-and-mood observation around a mental health check-in.
#[derive(Clone, Debug, PartialEq)]
pub struct GlucoseMoodCorrelation {
    pub recorded_at: String,
    pub nearest_glucose_mg_dl: f64,
    pub peak_prior_2h_mg_dl: f64,
    pub mood: u8,
    pub anxiety: u8,
    pub energy: u8,
    pub sleep_hours: f32,
    pub phq9: Option<u8>,
    pub gad7: Option<u8>,
    pub spike_adjacent: bool,
    pub note: String,
}

/// A longitudinal PHQ-9 / GAD-7 assessment point for multi-week trend charting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssessmentPoint {
    pub date: String,
    pub phq9: Option<u8>,
    pub gad7: Option<u8>,
    pub label: String,
}

/// Preloaded 4-week synthetic PHQ-9 & GAD-7 baseline trajectory.
pub fn sample_assessment_history() -> Vec<AssessmentPoint> {
    vec![
        AssessmentPoint {
            date: "2026-10-31".to_string(),
            phq9: Some(11),
            gad7: Some(12),
            label: "Baseline before Lingo CGM".to_string(),
        },
        AssessmentPoint {
            date: "2026-11-07".to_string(),
            phq9: Some(9),
            gad7: Some(10),
            label: "Week 1 Lingo tracking".to_string(),
        },
        AssessmentPoint {
            date: "2026-11-14".to_string(),
            phq9: Some(7),
            gad7: Some(8),
            label: "Reduced afternoon spikes".to_string(),
        },
        AssessmentPoint {
            date: "2026-11-21".to_string(),
            phq9: Some(6),
            gad7: Some(7),
            label: "Current check-in".to_string(),
        },
    ]
}

/// Combine longitudinal baseline assessments with any newly logged check-ins that carry PHQ-9/GAD-7.
pub fn build_assessment_series(
    baseline: &[AssessmentPoint],
    checkins: &[MentalHealthCheckIn],
) -> Vec<AssessmentPoint> {
    let mut series = baseline.to_vec();
    for c in checkins {
        if (c.phq9.is_some() || c.gad7.is_some()) && c.recorded_at != "2026-11-21 07:15" {
            series.push(AssessmentPoint {
                date: c.recorded_at.clone(),
                phq9: c.phq9,
                gad7: c.gad7,
                label: c.note.clone(),
            });
        }
    }
    series
}

/// Quote-aware RFC-4180 CSV row splitter supporting `"..."` and `""` escapes.
fn split_csv_row(line: &str) -> Result<Vec<String>, String> {
    let mut cols = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    let mut just_closed_quote = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                    just_closed_quote = true;
                }
            } else {
                current.push(ch);
            }
        } else if just_closed_quote {
            if ch == ',' {
                cols.push(current.trim().to_string());
                current.clear();
                just_closed_quote = false;
            } else if ch.is_whitespace() {
                continue;
            } else {
                return Err("Unexpected characters after closing quote.".to_string());
            }
        } else if ch == ',' {
            cols.push(current.trim().to_string());
            current.clear();
        } else if ch == '"' && current.trim().is_empty() {
            current.clear();
            in_quotes = true;
        } else {
            current.push(ch);
        }
    }
    if in_quotes {
        return Err("Unterminated quoted field.".to_string());
    }
    cols.push(current.trim().to_string());
    Ok(cols)
}

/// Format an RFC-4180 CSV export of all correlated mental health check-ins.
pub fn export_correlations_csv(rows: &[GlucoseMoodCorrelation]) -> String {
    let mut out = String::from(
        "recorded_at,nearest_glucose_mg_dl,peak_prior_2h_mg_dl,mood,anxiety,energy,sleep_hours,phq9,gad7,pattern,note\n",
    );
    for row in rows {
        let phq9 = row.phq9.map(|v| v.to_string()).unwrap_or_default();
        let gad7 = row.gad7.map(|v| v.to_string()).unwrap_or_default();
        let pattern = if row.spike_adjacent {
            "post_spike_gt_140"
        } else {
            "in_lingo_zone"
        };
        let escaped_note = row.note.replace('"', "\"\"");
        out.push_str(&format!(
            "\"{}\",{:.1},{:.1},{},{},{},{:.1},{},{},{},\"{}\"\n",
            row.recorded_at.replace('"', "\"\""),
            row.nearest_glucose_mg_dl,
            row.peak_prior_2h_mg_dl,
            row.mood,
            row.anxiety,
            row.energy,
            row.sleep_hours,
            phq9,
            gad7,
            pattern,
            escaped_note,
        ));
    }
    out
}

/// Percent-encode CSV text for a `data:text/csv;charset=utf-8,...` download link.
pub fn csv_data_uri(csv: &str) -> String {
    let mut encoded = String::from("data:text/csv;charset=utf-8,");
    for byte in csv.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Classify a PHQ-9 score (`0..=27`).
pub fn classify_phq9(score: u8) -> ClinicalBand {
    match score {
        0..=4 => ClinicalBand::Minimal,
        5..=9 => ClinicalBand::Mild,
        10..=14 => ClinicalBand::Moderate,
        15..=19 => ClinicalBand::ModeratelySevere,
        _ => ClinicalBand::Severe,
    }
}

/// Classify a GAD-7 score (`0..=21`).
pub fn classify_gad7(score: u8) -> ClinicalBand {
    match score {
        0..=4 => ClinicalBand::Minimal,
        5..=9 => ClinicalBand::Mild,
        10..=14 => ClinicalBand::Moderate,
        _ => ClinicalBand::Severe,
    }
}

/// Parse an Abbott Lingo CSV export (`timestamp,glucose_mg_dl[,lingo_count[,tag]]`).
///
/// Ignores blank lines, `#` comments, and a header row containing `"glucose"` or
/// `"timestamp"`. Assigns sequential `hour_offset` values from parsed `HH:MM`
/// timestamps when possible (or evenly spaced `0.5h` steps as fallback).
pub fn parse_lingo_csv(input: &str) -> Result<Vec<LingoReading>, String> {
    let mut readings = Vec::new();
    let mut first_minutes: Option<f64> = None;

    for (line_idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if readings.is_empty() && (lower.contains("timestamp") || lower.contains("glucose")) {
            continue;
        }

        let cols = split_csv_row(line).map_err(|err| format!("Line {}: {err}", line_idx + 1))?;
        if cols.len() < 2 {
            return Err(format!(
                "Line {} needs at least `timestamp,glucose_mg_dl`.",
                line_idx + 1
            ));
        }

        let timestamp = cols[0].clone();
        if timestamp.is_empty() {
            return Err(format!("Line {} has an empty timestamp.", line_idx + 1));
        }

        let glucose: f64 = cols[1]
            .parse()
            .map_err(|_| format!("Line {} has invalid glucose `{}`.", line_idx + 1, cols[1]))?;
        if !glucose.is_finite() || !(35.0..=450.0).contains(&glucose) {
            return Err(format!(
                "Line {} glucose `{glucose}` is outside physiological sensor range (35–450 mg/dL).",
                line_idx + 1
            ));
        }

        let lingo_count = cols
            .get(2)
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);

        let tag = cols.get(3).filter(|s| !s.is_empty()).cloned();

        let hour_offset = if let Some(mins) = parse_pseudo_minutes(&timestamp) {
            let start = *first_minutes.get_or_insert(mins);
            ((mins - start) / 60.0).max(0.0)
        } else {
            readings.len() as f64 * 0.5
        };

        readings.push(LingoReading {
            timestamp,
            hour_offset,
            glucose_mg_dl: glucose,
            lingo_count,
            tag,
        });
    }

    if readings.is_empty() {
        return Err("No valid Abbott Lingo glucose rows found in CSV.".to_string());
    }

    Ok(readings)
}

/// Extract monotonic minutes from `YYYY-MM-DD HH:MM` or `YYYY-MM-DDTHH:MM`.
fn parse_pseudo_minutes(ts: &str) -> Option<f64> {
    let cleaned = ts.replace('T', " ");
    let mut parts = cleaned.split_whitespace();
    let date_part = parts.next()?;
    let time_part = parts.next()?;

    let mut d = date_part.split('-');
    let _year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;

    let mut t = time_part.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;

    let day_index = month * 31 + day;
    Some((day_index * 24 * 60 + hour * 60 + minute) as f64)
}

/// Compute Lingo CGM summary statistics.
pub fn summarize_lingo(readings: &[LingoReading]) -> Option<LingoSummary> {
    if readings.is_empty() {
        return None;
    }
    let n = readings.len() as f64;
    let sum: f64 = readings.iter().map(|r| r.glucose_mg_dl).sum();
    let mean = sum / n;
    let min = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(f64::INFINITY, f64::min);
    let max = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(f64::NEG_INFINITY, f64::max);
    let variance = readings
        .iter()
        .map(|r| (r.glucose_mg_dl - mean).powi(2))
        .sum::<f64>()
        / n;
    let std_dev = variance.sqrt();
    let cv_percent = if mean > 0.0 {
        (std_dev / mean) * 100.0
    } else {
        0.0
    };
    let in_metabolic = readings
        .iter()
        .filter(|r| (70.0..=140.0).contains(&r.glucose_mg_dl))
        .count() as f64;
    let in_clinical = readings
        .iter()
        .filter(|r| (70.0..=180.0).contains(&r.glucose_mg_dl))
        .count() as f64;
    let spike_readings = readings.iter().filter(|r| r.glucose_mg_dl > 140.0).count();

    let explicit_count: f64 = readings.iter().filter_map(|r| r.lingo_count).sum();
    let total_lingo_count = if explicit_count > 0.0 {
        explicit_count
    } else {
        readings
            .iter()
            .filter(|r| r.glucose_mg_dl > 140.0)
            .map(|r| ((r.glucose_mg_dl - 140.0) / 10.0).max(1.0))
            .sum::<f64>()
            .round()
    };

    Some(LingoSummary {
        reading_count: readings.len(),
        mean_mg_dl: mean,
        min_mg_dl: min,
        max_mg_dl: max,
        std_dev_mg_dl: std_dev,
        cv_percent,
        tir_metabolic_pct: (in_metabolic / n) * 100.0,
        tir_clinical_pct: (in_clinical / n) * 100.0,
        spike_readings,
        total_lingo_count,
    })
}

/// Pair each mental health check-in with nearest glucose and prior 2-hour peak.
pub fn correlate_glucose_and_mood(
    readings: &[LingoReading],
    checkins: &[MentalHealthCheckIn],
) -> Vec<GlucoseMoodCorrelation> {
    if readings.is_empty() {
        return Vec::new();
    }
    checkins
        .iter()
        .map(|c| {
            let nearest = readings
                .iter()
                .min_by(|a, b| {
                    (a.hour_offset - c.hour_offset)
                        .abs()
                        .total_cmp(&(b.hour_offset - c.hour_offset).abs())
                })
                .map(|r| r.glucose_mg_dl)
                .unwrap_or(0.0);
            let peak_prior_2h = readings
                .iter()
                .filter(|r| r.hour_offset <= c.hour_offset && r.hour_offset >= c.hour_offset - 2.0)
                .map(|r| r.glucose_mg_dl)
                .fold(nearest, f64::max);

            GlucoseMoodCorrelation {
                recorded_at: c.recorded_at.clone(),
                nearest_glucose_mg_dl: nearest,
                peak_prior_2h_mg_dl: peak_prior_2h,
                mood: c.mood,
                anxiety: c.anxiety,
                energy: c.energy,
                sleep_hours: c.sleep_hours,
                phq9: c.phq9,
                gad7: c.gad7,
                spike_adjacent: peak_prior_2h > 140.0,
                note: c.note.clone(),
            }
        })
        .collect()
}

/// Preloaded synthetic 24-hour Abbott Lingo CGM CSV sample.
pub const SAMPLE_LINGO_CSV: &str = "\
timestamp,glucose_mg_dl,lingo_count,tag
2026-11-21 06:00,84,0,Fasting wake
2026-11-21 07:00,88,0,Morning light walk
2026-11-21 08:00,118,0,Oatmeal & espresso
2026-11-21 09:00,104,0,Steady morning
2026-11-21 10:30,92,0,Deep work block
2026-11-21 12:00,95,0,Pre-lunch
2026-11-21 13:00,164,14,Sweetened iced latte + pastry
2026-11-21 13:45,152,8,Post-lunch peak
2026-11-21 14:30,78,0,Rapid reactive dip
2026-11-21 16:00,96,0,Protein snack + walk
2026-11-21 18:30,108,0,Balanced dinner
2026-11-21 20:00,116,0,Evening wind-down
2026-11-21 22:00,94,0,Pre-sleep
2026-11-22 02:00,82,0,Overnight steady
2026-11-22 06:00,86,0,Fasting wake";

/// Preloaded synthetic mental health check-ins paired with [`SAMPLE_LINGO_CSV`].
pub fn sample_checkins() -> Vec<MentalHealthCheckIn> {
    vec![
        MentalHealthCheckIn {
            recorded_at: "2026-11-21 07:15".to_string(),
            hour_offset: 1.25,
            mood: 8,
            anxiety: 3,
            energy: 8,
            sleep_hours: 7.5,
            phq9: Some(6),
            gad7: Some(7),
            note: "Clear-headed after 7.5h sleep and morning walk; glucose steady.".to_string(),
        },
        MentalHealthCheckIn {
            recorded_at: "2026-11-21 14:30".to_string(),
            hour_offset: 8.5,
            mood: 4,
            anxiety: 8,
            energy: 3,
            sleep_hours: 7.5,
            phq9: None,
            gad7: None,
            note: "Jittery, racing thoughts & fatigue ~90m after 164 mg/dL lunch spike and rapid dip to 78.".to_string(),
        },
        MentalHealthCheckIn {
            recorded_at: "2026-11-21 17:00".to_string(),
            hour_offset: 11.0,
            mood: 7,
            anxiety: 4,
            energy: 6,
            sleep_hours: 7.5,
            phq9: None,
            gad7: None,
            note: "Anxiety settled after protein snack and 15-minute walk.".to_string(),
        },
        MentalHealthCheckIn {
            recorded_at: "2026-11-21 21:30".to_string(),
            hour_offset: 15.5,
            mood: 7,
            anxiety: 3,
            energy: 6,
            sleep_hours: 7.5,
            phq9: None,
            gad7: None,
            note: "Calm evening after balanced dinner; no late excursion.".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample_lingo_csv_and_computes_metrics() {
        let readings = parse_lingo_csv(SAMPLE_LINGO_CSV).expect("sample CSV parses");
        assert_eq!(readings.len(), 15);
        assert!((readings[0].hour_offset - 0.0).abs() < 1e-6);
        assert!((readings[6].glucose_mg_dl - 164.0).abs() < 1e-6);

        let summary = summarize_lingo(&readings).expect("summary present");
        assert_eq!(summary.reading_count, 15);
        assert_eq!(summary.spike_readings, 2);
        assert!((summary.total_lingo_count - 22.0).abs() < 1e-6);
        assert!((summary.min_mg_dl - 78.0).abs() < 1e-6);
        assert!((summary.max_mg_dl - 164.0).abs() < 1e-6);
        assert!(summary.tir_metabolic_pct > 80.0 && summary.tir_metabolic_pct < 90.0);
        assert!((summary.tir_clinical_pct - 100.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_empty_and_out_of_range_lingo_csv() {
        assert!(parse_lingo_csv("timestamp,glucose_mg_dl\n").is_err());
        assert!(parse_lingo_csv("2026-11-21 08:00,999\n").is_err());
        assert!(parse_lingo_csv("bad-row-without-comma").is_err());
    }

    #[test]
    fn correlates_spike_adjacent_anxiety_dip() {
        let readings = parse_lingo_csv(SAMPLE_LINGO_CSV).unwrap();
        let checkins = sample_checkins();
        let corr = correlate_glucose_and_mood(&readings, &checkins);
        assert_eq!(corr.len(), 4);
        assert!(!corr[0].spike_adjacent);
        assert!(corr[1].spike_adjacent);
        assert!((corr[1].peak_prior_2h_mg_dl - 164.0).abs() < 1e-6);
        assert_eq!(corr[1].anxiety, 8);
    }

    #[test]
    fn classifies_phq9_and_gad7_bands() {
        assert_eq!(classify_phq9(4), ClinicalBand::Minimal);
        assert_eq!(classify_phq9(6), ClinicalBand::Mild);
        assert_eq!(classify_phq9(12), ClinicalBand::Moderate);
        assert_eq!(classify_phq9(17), ClinicalBand::ModeratelySevere);
        assert_eq!(classify_phq9(22), ClinicalBand::Severe);

        assert_eq!(classify_gad7(3), ClinicalBand::Minimal);
        assert_eq!(classify_gad7(7), ClinicalBand::Mild);
        assert_eq!(classify_gad7(12), ClinicalBand::Moderate);
        assert_eq!(classify_gad7(18), ClinicalBand::Severe);
    }

    #[test]
    fn parses_quoted_commas_and_exports_rfc4180_csv() {
        let csv = "timestamp,glucose_mg_dl,lingo_count,tag\n\
                   \"2026-11-21 13:00\",164,14,\"Oat latte, pastry, \"\"rush\"\"\"\n";
        let readings = parse_lingo_csv(csv).expect("quoted comma CSV parses");
        assert_eq!(readings.len(), 1);
        assert_eq!(
            readings[0].tag.as_deref(),
            Some("Oat latte, pastry, \"rush\"")
        );

        let checkins = sample_checkins();
        let corr = correlate_glucose_and_mood(&readings, &checkins);
        let exported = export_correlations_csv(&corr);
        assert!(exported.starts_with("recorded_at,nearest_glucose_mg_dl,"));
        assert!(exported.contains("post_spike_gt_140"));
        assert!(csv_data_uri(&exported).starts_with("data:text/csv;charset=utf-8,"));

        let series = build_assessment_series(&sample_assessment_history(), &checkins);
        assert_eq!(series.len(), 4);
    }
}
