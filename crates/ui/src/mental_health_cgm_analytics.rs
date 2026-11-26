//! Pure-Rust advanced CGM analytics for multi-month Abbott Lingo and commercial CGM/BGM traces.
//!
//! Provides gap-aware chart segmentation/downsampling, per-day glycemic summaries, ATTD
//! sustained hypo/hyperglycemia episode detection, continuous wear-session & gap segmentation,
//! circadian period breakdowns, 15-minute glucose velocity (rate-of-change), and trailing
//! period-over-period comparisons.
//!
//! 100% `wasm32`-safe with zero network calls or external runtime dependencies.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::mental_health_cgm::{
    downsample_readings_for_chart, effective_reading_bound, extract_clock_hour, parse_timestamp,
    percentile_sorted, GlucoseMonitor, MeasurementMethod,
};
use crate::mental_health_lingo::{GlucoseReadingBound, LingoReading};

/// Default gap threshold (hours) for breaking 2D SVG and 3D WebGL2 chart polylines/ribbons.
pub const DEFAULT_CHART_GAP_HOURS: f64 = 2.0;

/// Default gap threshold (hours) separating continuous sensor wear sessions.
pub const DEFAULT_SESSION_GAP_HOURS: f64 = 2.0;

/// Default trailing comparison window length (days) for period-over-period deltas.
pub const DEFAULT_COMPARISON_WINDOW_DAYS: f64 = 14.0;

/// Clinical rapid glucose velocity threshold (`±2.0 mg/dL/min`).
pub const RAPID_ROC_THRESHOLD_MG_DL_PER_MIN: f64 = 2.0;

/// Split a chronologically ordered glucose trace into contiguous segments wherever consecutive
/// readings are separated by more than `max_gap_hours` (typically `2.0` hours for CGM).
pub fn split_readings_into_segments(
    readings: &[LingoReading],
    max_gap_hours: f64,
) -> Vec<Vec<LingoReading>> {
    if readings.is_empty() {
        return Vec::new();
    }
    if !max_gap_hours.is_finite() || max_gap_hours <= 0.0 {
        return vec![readings.to_vec()];
    }

    let mut segments: Vec<Vec<LingoReading>> = Vec::new();
    let mut current: Vec<LingoReading> = vec![readings[0].clone()];

    for reading in &readings[1..] {
        let prev = current.last().expect("current segment is non-empty");
        let gap_hours = (reading.hour_offset - prev.hour_offset).abs();
        if gap_hours > max_gap_hours + 1e-6 {
            segments.push(current);
            current = vec![reading.clone()];
        } else {
            current.push(reading.clone());
        }
    }
    segments.push(current);
    segments
}

/// Split `readings` across gaps exceeding `max_gap_hours` and downsample each segment
/// proportionally so total rendered points stay within `max_points` while NEVER bridging
/// or merging readings across a sensor-off gap.
pub fn downsample_segments_for_chart(
    readings: &[LingoReading],
    max_points: usize,
    max_gap_hours: f64,
) -> Vec<Vec<LingoReading>> {
    let segments = split_readings_into_segments(readings, max_gap_hours);
    if segments.is_empty() || readings.len() <= max_points || max_points < 4 {
        return segments;
    }

    let min_per_seg: Vec<usize> = segments.iter().map(|s| s.len().min(2)).collect();
    let min_total: usize = min_per_seg.iter().sum();

    if max_points <= min_total {
        return segments
            .into_iter()
            .map(|seg| {
                if seg.len() <= 2 {
                    seg
                } else {
                    vec![seg[0].clone(), seg[seg.len() - 1].clone()]
                }
            })
            .collect();
    }

    let preferred_min_per_seg: Vec<usize> = segments.iter().map(|s| s.len().min(4)).collect();
    let preferred_min_total: usize = preferred_min_per_seg.iter().sum();
    let base_alloc = if preferred_min_total <= max_points {
        preferred_min_per_seg
    } else {
        min_per_seg
    };
    let base_sum: usize = base_alloc.iter().sum();
    let extra_budget = max_points.saturating_sub(base_sum);

    let excess_counts: Vec<usize> = segments
        .iter()
        .zip(&base_alloc)
        .map(|(seg, &base)| seg.len().saturating_sub(base))
        .collect();
    let total_excess: usize = excess_counts.iter().sum();

    let mut allocations = base_alloc;
    if extra_budget > 0 && total_excess > 0 {
        let mut remaining = extra_budget;
        for (alloc, &excess) in allocations.iter_mut().zip(&excess_counts) {
            if excess == 0 {
                continue;
            }
            let share =
                ((excess as f64 / total_excess as f64) * extra_budget as f64).floor() as usize;
            let granted = share.min(excess).min(remaining);
            *alloc += granted;
            remaining = remaining.saturating_sub(granted);
        }
        // Distribute any rounding remainder to the largest still-compressible segments.
        if remaining > 0 {
            let mut order: Vec<usize> = (0..segments.len())
                .filter(|&i| allocations[i] < segments[i].len())
                .collect();
            order.sort_by(|&a, &b| {
                (segments[b].len() - allocations[b]).cmp(&(segments[a].len() - allocations[a]))
            });
            for idx in order {
                if remaining == 0 {
                    break;
                }
                let room = segments[idx].len() - allocations[idx];
                let add = room.min(remaining);
                allocations[idx] += add;
                remaining -= add;
            }
        }
    }

    segments
        .into_iter()
        .zip(allocations)
        .map(|(seg, budget)| downsample_single_segment(&seg, budget))
        .collect()
}

fn downsample_single_segment(seg: &[LingoReading], budget: usize) -> Vec<LingoReading> {
    if seg.len() <= budget || budget == 0 {
        return seg.to_vec();
    }
    if budget == 1 {
        return vec![seg[0].clone()];
    }
    if budget == 2 {
        return vec![seg[0].clone(), seg[seg.len() - 1].clone()];
    }
    if budget == 3 {
        let interior = &seg[1..seg.len() - 1];
        let mid_mean = (seg[0].glucose_mg_dl + seg[seg.len() - 1].glucose_mg_dl) * 0.5;
        let extreme = interior
            .iter()
            .max_by(|a, b| {
                (a.glucose_mg_dl - mid_mean)
                    .abs()
                    .total_cmp(&(b.glucose_mg_dl - mid_mean).abs())
            })
            .cloned()
            .unwrap_or_else(|| seg[seg.len() / 2].clone());
        return vec![seg[0].clone(), extreme, seg[seg.len() - 1].clone()];
    }
    downsample_readings_for_chart(seg, budget)
}

/// Per-calendar-day glycemic and sensor saturation summary (grouped in each reading's local date).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyGlucoseSummary {
    /// Local calendar date (`YYYY-MM-DD`, or `day-NNNN` fallback when timestamps are unparseable).
    pub date: String,
    pub reading_count: usize,
    pub active_wear_pct: f64,
    pub mean_mg_dl: f64,
    pub median_mg_dl: f64,
    pub min_mg_dl: f64,
    pub max_mg_dl: f64,
    pub tir_70_180_pct: f64,
    pub titr_70_140_pct: f64,
    pub tbr_lt_70_pct: f64,
    pub tar_gt_180_pct: f64,
    pub ceiling_capped_pct: f64,
    pub floor_capped_pct: f64,
    /// Mean glucose during overnight clock hours (`00:00–06:00`), if readings exist.
    pub overnight_mean_mg_dl: Option<f64>,
    /// Mean glucose during daytime/evening clock hours (`06:00–24:00`), if readings exist.
    pub daytime_mean_mg_dl: Option<f64>,
}

/// Classification of sustained ATTD glycemic episodes (`>= 15 min`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EpisodeKind {
    /// `< 54 mg/dL` or floor-capped `<= 55 mg/dL` (including Abbott Lingo `<55` -> `54`).
    Level2HypoLt54,
    /// `< 70 mg/dL` or any `BelowSensorFloor` sentinel.
    Level1HypoLt70,
    /// `> 180 mg/dL` (Level 1+2 hyperglycemia).
    HyperGt180,
    /// `> 140 mg/dL` (Abbott Lingo metabolic excursion).
    LingoExcursionGt140,
}

impl EpisodeKind {
    pub const ALL: [Self; 4] = [
        Self::Level2HypoLt54,
        Self::Level1HypoLt70,
        Self::HyperGt180,
        Self::LingoExcursionGt140,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Level2HypoLt54 => "Level 2 Hypo (<54 / floor)",
            Self::Level1HypoLt70 => "Level 1 Hypo (<70 mg/dL)",
            Self::HyperGt180 => "Hyperglycemia (>180 mg/dL)",
            Self::LingoExcursionGt140 => "Lingo Excursion (>140 mg/dL)",
        }
    }

    fn is_hypo(self) -> bool {
        matches!(self, Self::Level2HypoLt54 | Self::Level1HypoLt70)
    }

    fn matches(self, reading: &LingoReading, bound: GlucoseReadingBound) -> bool {
        let g = reading.glucose_mg_dl;
        match self {
            Self::Level2HypoLt54 => {
                g < 54.0 || (bound == GlucoseReadingBound::BelowSensorFloor && g <= 55.0)
            }
            Self::Level1HypoLt70 => g < 70.0 || bound == GlucoseReadingBound::BelowSensorFloor,
            Self::HyperGt180 => {
                g > 180.0 || (bound == GlucoseReadingBound::AboveSensorCeiling && g >= 180.0)
            }
            Self::LingoExcursionGt140 => {
                g > 140.0 || bound == GlucoseReadingBound::AboveSensorCeiling
            }
        }
    }

    fn is_relevant_cap(self, bound: GlucoseReadingBound) -> bool {
        if self.is_hypo() {
            bound == GlucoseReadingBound::BelowSensorFloor
        } else {
            bound == GlucoseReadingBound::AboveSensorCeiling
        }
    }
}

/// A single sustained glycemic episode (`>= 15 min`, allowing a brief interruption `<= 10 min`,
/// broken across sensor gaps `> 30 min`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlycemicEpisode {
    pub kind: EpisodeKind,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub duration_minutes: f64,
    pub reading_count: usize,
    /// Nadir (`min`) for hypo episodes or peak (`max`) for hyper/excursion episodes.
    pub extreme_mg_dl: f64,
    /// Whether any reading inside this episode hit a relevant sensor floor/ceiling cap.
    pub sensor_capped: bool,
}

/// Aggregate sustained-episode counts and total durations across all four episode kinds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EpisodeCounts {
    pub level2_hypo_count: usize,
    pub level2_hypo_minutes: f64,
    pub level1_hypo_count: usize,
    pub level1_hypo_minutes: f64,
    pub hyper_gt_180_count: usize,
    pub hyper_gt_180_minutes: f64,
    pub lingo_excursion_gt_140_count: usize,
    pub lingo_excursion_gt_140_minutes: f64,
}

/// A sensor-off / Bluetooth dropout gap separating two continuous wear sessions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SensorGap {
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub start_hour_offset: f64,
    pub end_hour_offset: f64,
    pub duration_hours: f64,
}

/// A continuous sensor wear session separated from neighboring sessions by gaps `>= 2.0` hours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SensorSession {
    /// 1-based wear session index in chronological order.
    pub session_index: usize,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub duration_hours: f64,
    pub reading_count: usize,
    pub active_wear_pct: f64,
    pub mean_mg_dl: f64,
    pub tir_70_180_pct: f64,
    pub ceiling_capped_pct: f64,
    pub preceding_gap_hours: Option<f64>,
}

/// Four-quadrant local clock-time circadian period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CircadianPeriod {
    /// `00:00–06:00` local time.
    Overnight,
    /// `06:00–12:00` local time.
    Morning,
    /// `12:00–18:00` local time.
    Afternoon,
    /// `18:00–24:00` local time.
    Evening,
}

impl CircadianPeriod {
    pub const ALL: [Self; 4] = [
        Self::Overnight,
        Self::Morning,
        Self::Afternoon,
        Self::Evening,
    ];

    pub fn from_clock_hour(hour: u8) -> Self {
        match hour % 24 {
            0..=5 => Self::Overnight,
            6..=11 => Self::Morning,
            12..=17 => Self::Afternoon,
            _ => Self::Evening,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overnight => "Overnight",
            Self::Morning => "Morning",
            Self::Afternoon => "Afternoon",
            Self::Evening => "Evening",
        }
    }

    pub fn hours_label(self) -> &'static str {
        match self {
            Self::Overnight => "00:00–06:00",
            Self::Morning => "06:00–12:00",
            Self::Afternoon => "12:00–18:00",
            Self::Evening => "18:00–24:00",
        }
    }
}

/// Glycemic summary metrics aggregated within one [`CircadianPeriod`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CircadianPeriodSummary {
    pub period: CircadianPeriod,
    pub reading_count: usize,
    pub mean_mg_dl: f64,
    pub median_mg_dl: f64,
    pub p95_mg_dl: f64,
    pub tir_70_180_pct: f64,
    pub titr_70_140_pct: f64,
    pub tbr_lt_70_pct: f64,
    pub ceiling_capped_pct: f64,
}

/// 15-minute glucose velocity (rate-of-change in `mg/dL/min`) summary.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RateOfChangeSummary {
    /// Number of valid unbroken 10–25 minute velocity pairs evaluated.
    pub sample_pairs: usize,
    /// Steepest 15-minute rise velocity (`>= 0.0 mg/dL/min`).
    pub max_rise_mg_dl_per_min: f64,
    /// Steepest 15-minute fall velocity (`<= 0.0 mg/dL/min`).
    pub max_fall_mg_dl_per_min: f64,
    /// Distinct contiguous rapid-rise bursts (`>= +2.0 mg/dL/min`).
    pub rapid_rise_events: usize,
    /// Distinct contiguous rapid-fall bursts (`<= -2.0 mg/dL/min`).
    pub rapid_fall_events: usize,
    /// Mean absolute 15-minute velocity (`|mg/dL/min|`).
    pub mean_abs_roc: f64,
}

/// Trailing period-over-period comparison (e.g., last 14 days vs immediately preceding 14 days).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PeriodComparison {
    pub recent_days: f64,
    pub prior_days: f64,
    pub recent_readings: usize,
    pub prior_readings: usize,
    pub recent_mean_mg_dl: f64,
    pub prior_mean_mg_dl: f64,
    pub delta_mean_mg_dl: f64,
    pub recent_gmi_pct: f64,
    pub prior_gmi_pct: f64,
    pub delta_gmi_pct: f64,
    pub recent_tir_pct: f64,
    pub prior_tir_pct: f64,
    pub delta_tir_pct: f64,
    pub recent_titr_pct: f64,
    pub prior_titr_pct: f64,
    pub delta_titr_pct: f64,
    pub recent_tbr_pct: f64,
    pub prior_tbr_pct: f64,
    pub delta_tbr_pct: f64,
    pub recent_ceiling_capped_pct: f64,
    pub prior_ceiling_capped_pct: f64,
    pub delta_ceiling_capped_pct: f64,
}

/// Unified advanced analytics bundle produced by [`analyze_glucose_trace`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdvancedGlucoseAnalytics {
    pub daily: Vec<DailyGlucoseSummary>,
    pub episodes: Vec<GlycemicEpisode>,
    pub episode_counts: EpisodeCounts,
    pub sessions: Vec<SensorSession>,
    pub gaps: Vec<SensorGap>,
    pub circadian: Vec<CircadianPeriodSummary>,
    pub rate_of_change: RateOfChangeSummary,
    pub period_comparison: Option<PeriodComparison>,
}

/// Estimate nominal sampling cadence in minutes from a glucose trace (clamped to `1..=60` min).
fn estimate_nominal_cadence_minutes(readings: &[LingoReading]) -> f64 {
    let mut intervals_min: Vec<f64> = readings
        .windows(2)
        .map(|w| (w[1].hour_offset - w[0].hour_offset).abs() * 60.0)
        .filter(|m| *m > 0.05)
        .collect();
    if intervals_min.is_empty() {
        return 5.0;
    }
    intervals_min.sort_by(f64::total_cmp);
    percentile_sorted(&intervals_min, 0.50).clamp(1.0, 60.0)
}

fn local_date_key(reading: &LingoReading) -> String {
    if let Some(parsed) = parse_timestamp(&reading.timestamp, None) {
        parsed.local_date.to_string()
    } else {
        let day_idx = (reading.hour_offset / 24.0).floor().max(0.0) as usize + 1;
        format!("day-{day_idx:04}")
    }
}

/// Compute per-local-calendar-date summaries across `readings`.
pub fn summarize_daily_glucose(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    method: MeasurementMethod,
) -> Vec<DailyGlucoseSummary> {
    if readings.is_empty() {
        return Vec::new();
    }

    let cadence_min = estimate_nominal_cadence_minutes(readings);
    let expected_per_day = (1440.0 / cadence_min).max(1.0);

    let mut by_date: BTreeMap<String, Vec<&LingoReading>> = BTreeMap::new();
    for r in readings {
        by_date.entry(local_date_key(r)).or_default().push(r);
    }

    by_date
        .into_iter()
        .map(|(date, day_readings)| {
            let count = day_readings.len();
            let n = count as f64;
            let mut sorted: Vec<f64> = day_readings.iter().map(|r| r.glucose_mg_dl).collect();
            sorted.sort_by(f64::total_cmp);

            let mean_mg_dl = sorted.iter().sum::<f64>() / n;
            let median_mg_dl = percentile_sorted(&sorted, 0.50);
            let min_mg_dl = sorted[0];
            let max_mg_dl = sorted[sorted.len() - 1];

            let mut tir = 0usize;
            let mut titr = 0usize;
            let mut tbr = 0usize;
            let mut tar = 0usize;
            let mut ceiling_capped = 0usize;
            let mut floor_capped = 0usize;
            let mut overnight_sum = 0.0_f64;
            let mut overnight_count = 0usize;
            let mut daytime_sum = 0.0_f64;
            let mut daytime_count = 0usize;

            for r in &day_readings {
                let g = r.glucose_mg_dl;
                if g < 70.0 {
                    tbr += 1;
                } else if g <= 180.0 {
                    tir += 1;
                    if g <= 140.0 {
                        titr += 1;
                    }
                } else {
                    tar += 1;
                }
                match effective_reading_bound(r, monitor) {
                    GlucoseReadingBound::AboveSensorCeiling => ceiling_capped += 1,
                    GlucoseReadingBound::BelowSensorFloor => floor_capped += 1,
                    GlucoseReadingBound::Measured => {}
                }
                let hour = extract_clock_hour(&r.timestamp, r.hour_offset);
                if hour < 6 {
                    overnight_sum += g;
                    overnight_count += 1;
                } else {
                    daytime_sum += g;
                    daytime_count += 1;
                }
            }

            let active_wear_pct = if method == MeasurementMethod::CapillaryBgm {
                100.0
            } else {
                ((n / expected_per_day) * 100.0).clamp(0.0, 100.0)
            };

            DailyGlucoseSummary {
                date,
                reading_count: count,
                active_wear_pct,
                mean_mg_dl,
                median_mg_dl,
                min_mg_dl,
                max_mg_dl,
                tir_70_180_pct: (tir as f64 / n) * 100.0,
                titr_70_140_pct: (titr as f64 / n) * 100.0,
                tbr_lt_70_pct: (tbr as f64 / n) * 100.0,
                tar_gt_180_pct: (tar as f64 / n) * 100.0,
                ceiling_capped_pct: (ceiling_capped as f64 / n) * 100.0,
                floor_capped_pct: (floor_capped as f64 / n) * 100.0,
                overnight_mean_mg_dl: (overnight_count > 0)
                    .then(|| overnight_sum / overnight_count as f64),
                daytime_mean_mg_dl: (daytime_count > 0).then(|| daytime_sum / daytime_count as f64),
            }
        })
        .collect()
}

/// Detect distinct sustained ATTD glycemic episodes (`>= 15 min`, allowing a single brief
/// interruption `<= 10 min`, broken across sensor gaps `> 30 min`).
pub fn detect_glycemic_episodes(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
) -> (Vec<GlycemicEpisode>, EpisodeCounts) {
    if readings.len() < 2 {
        return (Vec::new(), EpisodeCounts::default());
    }

    const MIN_EPISODE_MINUTES: f64 = 15.0;
    const MAX_INTERRUPTION_MINUTES: f64 = 10.0;
    const MAX_SENSOR_GAP_MINUTES: f64 = 30.0;

    let bounds: Vec<GlucoseReadingBound> = readings
        .iter()
        .map(|r| effective_reading_bound(r, monitor))
        .collect();

    let mut tagged_episodes: Vec<(usize, usize, GlycemicEpisode)> = Vec::new();
    let mut counts = EpisodeCounts::default();

    for (kind_order, kind) in EpisodeKind::ALL.into_iter().enumerate() {
        struct ActiveEpisode {
            start_idx: usize,
            last_in_idx: usize,
            in_count: usize,
            extreme_mg_dl: f64,
            sensor_capped: bool,
            in_interruption: bool,
            interruption_used: bool,
        }

        let mut active: Option<ActiveEpisode> = None;

        let flush = |ep: ActiveEpisode,
                     out: &mut Vec<(usize, usize, GlycemicEpisode)>,
                     counts: &mut EpisodeCounts| {
            let duration_minutes = ((readings[ep.last_in_idx].hour_offset
                - readings[ep.start_idx].hour_offset)
                * 60.0)
                .max(0.0);
            if ep.in_count >= 2 && duration_minutes >= MIN_EPISODE_MINUTES - 1e-6 {
                match kind {
                    EpisodeKind::Level2HypoLt54 => {
                        counts.level2_hypo_count += 1;
                        counts.level2_hypo_minutes += duration_minutes;
                    }
                    EpisodeKind::Level1HypoLt70 => {
                        counts.level1_hypo_count += 1;
                        counts.level1_hypo_minutes += duration_minutes;
                    }
                    EpisodeKind::HyperGt180 => {
                        counts.hyper_gt_180_count += 1;
                        counts.hyper_gt_180_minutes += duration_minutes;
                    }
                    EpisodeKind::LingoExcursionGt140 => {
                        counts.lingo_excursion_gt_140_count += 1;
                        counts.lingo_excursion_gt_140_minutes += duration_minutes;
                    }
                }
                out.push((
                    ep.start_idx,
                    kind_order,
                    GlycemicEpisode {
                        kind,
                        start_timestamp: readings[ep.start_idx].timestamp.clone(),
                        end_timestamp: readings[ep.last_in_idx].timestamp.clone(),
                        duration_minutes,
                        reading_count: ep.in_count,
                        extreme_mg_dl: ep.extreme_mg_dl,
                        sensor_capped: ep.sensor_capped,
                    },
                ));
            }
        };

        let start_episode = |idx: usize| -> ActiveEpisode {
            ActiveEpisode {
                start_idx: idx,
                last_in_idx: idx,
                in_count: 1,
                extreme_mg_dl: readings[idx].glucose_mg_dl,
                sensor_capped: kind.is_relevant_cap(bounds[idx]),
                in_interruption: false,
                interruption_used: false,
            }
        };

        for (idx, r) in readings.iter().enumerate() {
            let matches = kind.matches(r, bounds[idx]);
            if idx > 0 {
                let gap_min = (r.hour_offset - readings[idx - 1].hour_offset).abs() * 60.0;
                if gap_min > MAX_SENSOR_GAP_MINUTES + 1e-6 {
                    if let Some(ep) = active.take() {
                        flush(ep, &mut tagged_episodes, &mut counts);
                    }
                }
            }

            match active.as_mut() {
                None => {
                    if matches {
                        active = Some(start_episode(idx));
                    }
                }
                Some(ep) => {
                    if matches {
                        if ep.in_interruption {
                            let inter_min =
                                (r.hour_offset - readings[ep.last_in_idx].hour_offset).abs() * 60.0;
                            if inter_min <= MAX_INTERRUPTION_MINUTES + 1e-6 && !ep.interruption_used
                            {
                                ep.in_interruption = false;
                                ep.interruption_used = true;
                                ep.last_in_idx = idx;
                                ep.in_count += 1;
                                ep.extreme_mg_dl = if kind.is_hypo() {
                                    ep.extreme_mg_dl.min(r.glucose_mg_dl)
                                } else {
                                    ep.extreme_mg_dl.max(r.glucose_mg_dl)
                                };
                                ep.sensor_capped |= kind.is_relevant_cap(bounds[idx]);
                            } else {
                                let finished = active.take().expect("active episode present");
                                flush(finished, &mut tagged_episodes, &mut counts);
                                active = Some(start_episode(idx));
                            }
                        } else {
                            ep.last_in_idx = idx;
                            ep.in_count += 1;
                            ep.extreme_mg_dl = if kind.is_hypo() {
                                ep.extreme_mg_dl.min(r.glucose_mg_dl)
                            } else {
                                ep.extreme_mg_dl.max(r.glucose_mg_dl)
                            };
                            ep.sensor_capped |= kind.is_relevant_cap(bounds[idx]);
                        }
                    } else {
                        let inter_min =
                            (r.hour_offset - readings[ep.last_in_idx].hour_offset).abs() * 60.0;
                        if ep.interruption_used || inter_min > MAX_INTERRUPTION_MINUTES + 1e-6 {
                            let finished = active.take().expect("active episode present");
                            flush(finished, &mut tagged_episodes, &mut counts);
                        } else {
                            ep.in_interruption = true;
                        }
                    }
                }
            }
        }

        if let Some(ep) = active.take() {
            flush(ep, &mut tagged_episodes, &mut counts);
        }
    }

    tagged_episodes.sort_by_key(|(start_idx, kind_order, _)| (*start_idx, *kind_order));
    let episodes = tagged_episodes.into_iter().map(|(_, _, ep)| ep).collect();
    (episodes, counts)
}

/// Segment `readings` into continuous wear sessions separated by gaps `>= min_gap_hours`
/// (typically `2.0` hours), returning both the sessions and the intervening [`SensorGap`]s.
pub fn segment_sensor_sessions(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    method: MeasurementMethod,
    min_gap_hours: f64,
) -> (Vec<SensorSession>, Vec<SensorGap>) {
    if readings.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let threshold = if min_gap_hours.is_finite() && min_gap_hours > 0.0 {
        min_gap_hours
    } else {
        DEFAULT_SESSION_GAP_HOURS
    };
    let cadence_h = (estimate_nominal_cadence_minutes(readings) / 60.0).max(1.0 / 60.0);

    let mut ranges: Vec<(usize, usize, Option<f64>)> = Vec::new();
    let mut gaps: Vec<SensorGap> = Vec::new();
    let mut seg_start = 0usize;
    let mut pending_preceding_gap: Option<f64> = None;

    for i in 1..readings.len() {
        let gap_h = (readings[i].hour_offset - readings[i - 1].hour_offset).abs();
        if gap_h >= threshold - 1e-6 {
            ranges.push((seg_start, i - 1, pending_preceding_gap));
            gaps.push(SensorGap {
                start_timestamp: readings[i - 1].timestamp.clone(),
                end_timestamp: readings[i].timestamp.clone(),
                start_hour_offset: readings[i - 1].hour_offset,
                end_hour_offset: readings[i].hour_offset,
                duration_hours: gap_h,
            });
            seg_start = i;
            pending_preceding_gap = Some(gap_h);
        }
    }
    ranges.push((seg_start, readings.len() - 1, pending_preceding_gap));

    let sessions = ranges
        .into_iter()
        .enumerate()
        .map(|(idx, (start, end, preceding_gap_hours))| {
            let slice = &readings[start..=end];
            let count = slice.len();
            let n = count as f64;
            let first = &slice[0];
            let last = &slice[count - 1];
            let duration_hours = (last.hour_offset - first.hour_offset).max(0.0);

            let mut sum = 0.0_f64;
            let mut tir = 0usize;
            let mut ceiling_capped = 0usize;
            for r in slice {
                sum += r.glucose_mg_dl;
                if (70.0..=180.0).contains(&r.glucose_mg_dl) {
                    tir += 1;
                }
                if effective_reading_bound(r, monitor) == GlucoseReadingBound::AboveSensorCeiling {
                    ceiling_capped += 1;
                }
            }

            let active_wear_pct =
                if method == MeasurementMethod::CapillaryBgm || duration_hours <= 0.0 {
                    100.0
                } else {
                    let expected = (duration_hours / cadence_h) + 1.0;
                    ((n / expected) * 100.0).clamp(0.0, 100.0)
                };

            SensorSession {
                session_index: idx + 1,
                start_timestamp: first.timestamp.clone(),
                end_timestamp: last.timestamp.clone(),
                duration_hours,
                reading_count: count,
                active_wear_pct,
                mean_mg_dl: sum / n,
                tir_70_180_pct: (tir as f64 / n) * 100.0,
                ceiling_capped_pct: (ceiling_capped as f64 / n) * 100.0,
                preceding_gap_hours,
            }
        })
        .collect();

    (sessions, gaps)
}

/// Summarize glycemic metrics across the four circadian periods (`Overnight`, `Morning`,
/// `Afternoon`, `Evening`) in each reading's local clock time.
pub fn summarize_circadian_periods(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
) -> Vec<CircadianPeriodSummary> {
    let mut buckets: [Vec<&LingoReading>; 4] = std::array::from_fn(|_| Vec::new());
    for r in readings {
        let hour = extract_clock_hour(&r.timestamp, r.hour_offset);
        let idx = match CircadianPeriod::from_clock_hour(hour) {
            CircadianPeriod::Overnight => 0,
            CircadianPeriod::Morning => 1,
            CircadianPeriod::Afternoon => 2,
            CircadianPeriod::Evening => 3,
        };
        buckets[idx].push(r);
    }

    CircadianPeriod::ALL
        .into_iter()
        .zip(buckets)
        .map(|(period, bucket)| {
            if bucket.is_empty() {
                return CircadianPeriodSummary {
                    period,
                    reading_count: 0,
                    mean_mg_dl: 0.0,
                    median_mg_dl: 0.0,
                    p95_mg_dl: 0.0,
                    tir_70_180_pct: 0.0,
                    titr_70_140_pct: 0.0,
                    tbr_lt_70_pct: 0.0,
                    ceiling_capped_pct: 0.0,
                };
            }
            let count = bucket.len();
            let n = count as f64;
            let mut sorted: Vec<f64> = bucket.iter().map(|r| r.glucose_mg_dl).collect();
            sorted.sort_by(f64::total_cmp);

            let mean_mg_dl = sorted.iter().sum::<f64>() / n;
            let median_mg_dl = percentile_sorted(&sorted, 0.50);
            let p95_mg_dl = percentile_sorted(&sorted, 0.95);

            let mut tir = 0usize;
            let mut titr = 0usize;
            let mut tbr = 0usize;
            let mut ceiling_capped = 0usize;
            for r in &bucket {
                let g = r.glucose_mg_dl;
                if g < 70.0 {
                    tbr += 1;
                } else if g <= 180.0 {
                    tir += 1;
                    if g <= 140.0 {
                        titr += 1;
                    }
                }
                if effective_reading_bound(r, monitor) == GlucoseReadingBound::AboveSensorCeiling {
                    ceiling_capped += 1;
                }
            }

            CircadianPeriodSummary {
                period,
                reading_count: count,
                mean_mg_dl,
                median_mg_dl,
                p95_mg_dl,
                tir_70_180_pct: (tir as f64 / n) * 100.0,
                titr_70_140_pct: (titr as f64 / n) * 100.0,
                tbr_lt_70_pct: (tbr as f64 / n) * 100.0,
                ceiling_capped_pct: (ceiling_capped as f64 / n) * 100.0,
            }
        })
        .collect()
}

/// Compute 15-minute (10–25 min window, unbroken by gaps) glucose velocity statistics (`mg/dL/min`).
pub fn compute_rate_of_change(readings: &[LingoReading]) -> RateOfChangeSummary {
    if readings.len() < 2 {
        return RateOfChangeSummary::default();
    }

    const MIN_DT_MIN: f64 = 10.0;
    const TARGET_DT_MIN: f64 = 15.0;
    const MAX_DT_MIN: f64 = 25.0;
    const EVENT_GAP_BREAK_MIN: f64 = 30.0;

    let n = readings.len();
    let mut lo = 1usize;
    let mut sample_pairs = 0usize;
    let mut max_rise = 0.0_f64;
    let mut max_fall = 0.0_f64;
    let mut rapid_rise_events = 0usize;
    let mut rapid_fall_events = 0usize;
    let mut sum_abs_roc = 0.0_f64;

    let mut in_rapid_rise = false;
    let mut in_rapid_fall = false;
    let mut last_pair_start_h: Option<f64> = None;

    for i in 0..n {
        if lo <= i {
            lo = i + 1;
        }
        while lo < n
            && (readings[lo].hour_offset - readings[i].hour_offset) * 60.0 < MIN_DT_MIN - 1e-6
        {
            lo += 1;
        }
        if lo >= n {
            break;
        }

        let mut best: Option<(usize, f64)> = None;
        let mut k = lo;
        while k < n {
            let dt_min = (readings[k].hour_offset - readings[i].hour_offset) * 60.0;
            if dt_min > MAX_DT_MIN + 1e-6 {
                break;
            }
            if dt_min >= MIN_DT_MIN - 1e-6 {
                match best {
                    None => best = Some((k, dt_min)),
                    Some((_, prev_dt))
                        if (dt_min - TARGET_DT_MIN).abs() < (prev_dt - TARGET_DT_MIN).abs() =>
                    {
                        best = Some((k, dt_min));
                    }
                    _ => {}
                }
            }
            k += 1;
        }

        let Some((best_k, dt_min)) = best else {
            continue;
        };

        // Guard against any internal sensor gap > 25 min (defensive against unsorted timestamps).
        let unbroken = readings[i..best_k]
            .iter()
            .zip(&readings[i + 1..=best_k])
            .all(|(a, b)| {
                let step = (b.hour_offset - a.hour_offset) * 60.0;
                (0.0..=MAX_DT_MIN + 1e-6).contains(&step)
            });
        if !unbroken || dt_min <= 0.0 {
            continue;
        }

        let roc = (readings[best_k].glucose_mg_dl - readings[i].glucose_mg_dl) / dt_min;
        sample_pairs += 1;
        if roc > max_rise {
            max_rise = roc;
        }
        if roc < max_fall {
            max_fall = roc;
        }
        sum_abs_roc += roc.abs();

        if let Some(prev_h) = last_pair_start_h {
            if (readings[i].hour_offset - prev_h) * 60.0 > EVENT_GAP_BREAK_MIN + 1e-6 {
                in_rapid_rise = false;
                in_rapid_fall = false;
            }
        }
        if roc >= RAPID_ROC_THRESHOLD_MG_DL_PER_MIN - 1e-9 {
            if !in_rapid_rise {
                rapid_rise_events += 1;
                in_rapid_rise = true;
            }
            in_rapid_fall = false;
        } else if roc <= -RAPID_ROC_THRESHOLD_MG_DL_PER_MIN + 1e-9 {
            if !in_rapid_fall {
                rapid_fall_events += 1;
                in_rapid_fall = true;
            }
            in_rapid_rise = false;
        } else {
            in_rapid_rise = false;
            in_rapid_fall = false;
        }
        last_pair_start_h = Some(readings[i].hour_offset);
    }

    RateOfChangeSummary {
        sample_pairs,
        max_rise_mg_dl_per_min: max_rise,
        max_fall_mg_dl_per_min: max_fall,
        rapid_rise_events,
        rapid_fall_events,
        mean_abs_roc: if sample_pairs > 0 {
            sum_abs_roc / sample_pairs as f64
        } else {
            0.0
        },
    }
}

struct PeriodSliceStats {
    days: f64,
    readings: usize,
    mean_mg_dl: f64,
    gmi_pct: f64,
    tir_pct: f64,
    titr_pct: f64,
    tbr_pct: f64,
    ceiling_capped_pct: f64,
}

fn summarize_period_slice(
    slice: &[&LingoReading],
    monitor: GlucoseMonitor,
) -> Option<PeriodSliceStats> {
    if slice.is_empty() {
        return None;
    }
    let count = slice.len();
    let n = count as f64;
    let mut sum = 0.0_f64;
    let mut tir = 0usize;
    let mut titr = 0usize;
    let mut tbr = 0usize;
    let mut ceiling_capped = 0usize;

    for r in slice {
        let g = r.glucose_mg_dl;
        sum += g;
        if g < 70.0 {
            tbr += 1;
        } else if g <= 180.0 {
            tir += 1;
            if g <= 140.0 {
                titr += 1;
            }
        }
        if effective_reading_bound(r, monitor) == GlucoseReadingBound::AboveSensorCeiling {
            ceiling_capped += 1;
        }
    }

    let mean_mg_dl = sum / n;
    let gmi_pct = 3.31 + 0.02392 * mean_mg_dl;
    let days = ((slice[count - 1].hour_offset - slice[0].hour_offset) / 24.0).max(0.0);

    Some(PeriodSliceStats {
        days,
        readings: count,
        mean_mg_dl,
        gmi_pct,
        tir_pct: (tir as f64 / n) * 100.0,
        titr_pct: (titr as f64 / n) * 100.0,
        tbr_pct: (tbr as f64 / n) * 100.0,
        ceiling_capped_pct: (ceiling_capped as f64 / n) * 100.0,
    })
}

/// Compare the trailing `window_days` (default `14.0` days) against the immediately preceding
/// `window_days` when the trace spans at least `20` days (for a 14-day window, or proportionally
/// `window_days + min(6d, 0.5 * window_days)`).
pub fn compute_period_comparison(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    window_days: f64,
) -> Option<PeriodComparison> {
    if readings.len() < 2 || !window_days.is_finite() || window_days <= 0.0 {
        return None;
    }

    let first_h = readings.first()?.hour_offset;
    let end_h = readings.last()?.hour_offset;
    let span_days = ((end_h - first_h) / 24.0).max(0.0);
    let min_span_days = window_days + (6.0_f64).min(window_days * 0.5);
    if span_days < min_span_days - 1e-6 {
        return None;
    }

    let window_h = window_days * 24.0;
    let recent_start_h = end_h - window_h;
    let prior_start_h = end_h - 2.0 * window_h;

    let recent_slice: Vec<&LingoReading> = readings
        .iter()
        .filter(|r| r.hour_offset >= recent_start_h - 1e-6)
        .collect();
    let prior_slice: Vec<&LingoReading> = readings
        .iter()
        .filter(|r| r.hour_offset >= prior_start_h - 1e-6 && r.hour_offset < recent_start_h - 1e-6)
        .collect();

    let recent = summarize_period_slice(&recent_slice, monitor)?;
    let prior = summarize_period_slice(&prior_slice, monitor)?;

    Some(PeriodComparison {
        recent_days: recent.days,
        prior_days: prior.days,
        recent_readings: recent.readings,
        prior_readings: prior.readings,
        recent_mean_mg_dl: recent.mean_mg_dl,
        prior_mean_mg_dl: prior.mean_mg_dl,
        delta_mean_mg_dl: recent.mean_mg_dl - prior.mean_mg_dl,
        recent_gmi_pct: recent.gmi_pct,
        prior_gmi_pct: prior.gmi_pct,
        delta_gmi_pct: recent.gmi_pct - prior.gmi_pct,
        recent_tir_pct: recent.tir_pct,
        prior_tir_pct: prior.tir_pct,
        delta_tir_pct: recent.tir_pct - prior.tir_pct,
        recent_titr_pct: recent.titr_pct,
        prior_titr_pct: prior.titr_pct,
        delta_titr_pct: recent.titr_pct - prior.titr_pct,
        recent_tbr_pct: recent.tbr_pct,
        prior_tbr_pct: prior.tbr_pct,
        delta_tbr_pct: recent.tbr_pct - prior.tbr_pct,
        recent_ceiling_capped_pct: recent.ceiling_capped_pct,
        prior_ceiling_capped_pct: prior.ceiling_capped_pct,
        delta_ceiling_capped_pct: recent.ceiling_capped_pct - prior.ceiling_capped_pct,
    })
}

/// Unified entrypoint computing all advanced CGM analytics over `readings`, with optional
/// `full_trace_for_comparison` (used for the trailing 14d-vs-prior-14d delta even when the
/// user is currently viewing a shorter chart window like `7d` or `14d`).
pub fn analyze_glucose_trace(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    method: MeasurementMethod,
    full_trace_for_comparison: Option<&[LingoReading]>,
) -> AdvancedGlucoseAnalytics {
    let daily = summarize_daily_glucose(readings, monitor, method);
    let (episodes, episode_counts) = detect_glycemic_episodes(readings, monitor);
    let (sessions, gaps) =
        segment_sensor_sessions(readings, monitor, method, DEFAULT_SESSION_GAP_HOURS);
    let circadian = summarize_circadian_periods(readings, monitor);
    let rate_of_change = compute_rate_of_change(readings);
    let comparison_source = full_trace_for_comparison.unwrap_or(readings);
    let period_comparison =
        compute_period_comparison(comparison_source, monitor, DEFAULT_COMPARISON_WINDOW_DAYS);

    AdvancedGlucoseAnalytics {
        daily,
        episodes,
        episode_counts,
        sessions,
        gaps,
        circadian,
        rate_of_change,
        period_comparison,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn synthetic_reading(
        day_offset: i64,
        hour: u32,
        minute: u32,
        hour_offset: f64,
        glucose_mg_dl: f64,
        bound: GlucoseReadingBound,
    ) -> LingoReading {
        let base = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let date = base + chrono::TimeDelta::days(day_offset);
        LingoReading {
            timestamp: format!("{date}T{hour:02}:{minute:02}-07:00"),
            hour_offset,
            glucose_mg_dl,
            lingo_count: None,
            tag: None,
            bound,
        }
    }

    #[test]
    fn splits_and_downsamples_segments_without_bridging_sensor_gaps() {
        // Two dense segments (300 pts each at 5-min cadence = 25h each) separated by a 6h gap.
        let mut readings = Vec::new();
        for i in 0..300 {
            let g = if i == 150 {
                198.0
            } else {
                100.0 + (i % 10) as f64
            };
            let h = i as f64 * (5.0 / 60.0);
            readings.push(synthetic_reading(
                (i / 288) as i64,
                ((i % 288) / 12) as u32,
                ((i % 12) * 5) as u32,
                h,
                g,
                GlucoseReadingBound::Measured,
            ));
        }
        let gap_start_h = readings.last().unwrap().hour_offset;
        let seg2_start_h = gap_start_h + 6.0;
        for i in 0..300 {
            let g = if i == 120 {
                61.0
            } else {
                105.0 + (i % 8) as f64
            };
            let h = seg2_start_h + i as f64 * (5.0 / 60.0);
            readings.push(synthetic_reading(
                2 + (i / 288) as i64,
                ((i % 288) / 12) as u32,
                ((i % 12) * 5) as u32,
                h,
                g,
                GlucoseReadingBound::Measured,
            ));
        }

        let segs = split_readings_into_segments(&readings, DEFAULT_CHART_GAP_HOURS);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].len(), 300);
        assert_eq!(segs[1].len(), 300);

        let ds = downsample_segments_for_chart(&readings, 120, DEFAULT_CHART_GAP_HOURS);
        assert_eq!(ds.len(), 2);
        let total_pts: usize = ds.iter().map(Vec::len).sum();
        assert!(total_pts <= 120);
        // Extremes inside each segment are preserved and never bridged across the 6h gap.
        assert!(ds[0].iter().any(|r| (r.glucose_mg_dl - 198.0).abs() < 1e-6));
        assert!(ds[1].iter().any(|r| (r.glucose_mg_dl - 61.0).abs() < 1e-6));
        assert!(ds[1][0].hour_offset - ds[0].last().unwrap().hour_offset >= 5.99);
    }

    #[test]
    fn detects_sustained_attd_episodes_brief_interruptions_and_gap_breaks() {
        // 5-min synthetic trace:
        // 00:00..00:25: Level 2 hypo (54, BelowSensorFloor) with one 5-min blip to 60 at 00:10
        //               -> bridged (gap 00:05->00:15 = 10 min <= 10 min) -> 1 Level2HypoLt54 (25 min, capped)
        //               AND 1 Level1HypoLt70 (25 min, capped)
        // 01:00..01:05: 2-point blip at 190 (5 min) -> ignored (< 15 min)
        // 02:00..02:20: Hyper >180 (201 ceiling-capped) -> 1 HyperGt180 (20 min, capped)
        // 03:05..03:20: After 45-min sensor gap (>30 min), another Hyper >180 -> distinct 2nd episode!
        let pts = vec![
            (0, 0, 0.0, 54.0, GlucoseReadingBound::BelowSensorFloor),
            (
                0,
                5,
                5.0 / 60.0,
                54.0,
                GlucoseReadingBound::BelowSensorFloor,
            ),
            (0, 10, 10.0 / 60.0, 60.0, GlucoseReadingBound::Measured), // brief single interruption
            (
                0,
                15,
                15.0 / 60.0,
                54.0,
                GlucoseReadingBound::BelowSensorFloor,
            ),
            (
                0,
                20,
                20.0 / 60.0,
                54.0,
                GlucoseReadingBound::BelowSensorFloor,
            ),
            (
                0,
                25,
                25.0 / 60.0,
                54.0,
                GlucoseReadingBound::BelowSensorFloor,
            ),
            (0, 30, 30.0 / 60.0, 95.0, GlucoseReadingBound::Measured),
            (0, 35, 35.0 / 60.0, 98.0, GlucoseReadingBound::Measured),
            (1, 0, 1.0, 190.0, GlucoseReadingBound::Measured),
            (1, 5, 1.0 + 5.0 / 60.0, 192.0, GlucoseReadingBound::Measured),
            (
                1,
                10,
                1.0 + 10.0 / 60.0,
                125.0,
                GlucoseReadingBound::Measured,
            ),
            (2, 0, 2.0, 185.0, GlucoseReadingBound::Measured),
            (
                2,
                5,
                2.0 + 5.0 / 60.0,
                201.0,
                GlucoseReadingBound::AboveSensorCeiling,
            ),
            (
                2,
                10,
                2.0 + 10.0 / 60.0,
                201.0,
                GlucoseReadingBound::AboveSensorCeiling,
            ),
            (
                2,
                15,
                2.0 + 15.0 / 60.0,
                201.0,
                GlucoseReadingBound::AboveSensorCeiling,
            ),
            (
                2,
                20,
                2.0 + 20.0 / 60.0,
                195.0,
                GlucoseReadingBound::Measured,
            ),
            // 45-minute sensor gap (> 30 min) breaks hyper episode even though still >180
            (3, 5, 3.0 + 5.0 / 60.0, 186.0, GlucoseReadingBound::Measured),
            (
                3,
                10,
                3.0 + 10.0 / 60.0,
                190.0,
                GlucoseReadingBound::Measured,
            ),
            (
                3,
                15,
                3.0 + 15.0 / 60.0,
                194.0,
                GlucoseReadingBound::Measured,
            ),
            (
                3,
                20,
                3.0 + 20.0 / 60.0,
                188.0,
                GlucoseReadingBound::Measured,
            ),
            (
                3,
                25,
                3.0 + 25.0 / 60.0,
                120.0,
                GlucoseReadingBound::Measured,
            ),
        ];

        let readings: Vec<LingoReading> = pts
            .into_iter()
            .map(|(h, m, off, g, b)| synthetic_reading(0, h, m, off, g, b))
            .collect();

        let (episodes, counts) = detect_glycemic_episodes(&readings, GlucoseMonitor::AbbottLingo);

        assert_eq!(counts.level2_hypo_count, 1);
        assert!((counts.level2_hypo_minutes - 25.0).abs() < 1e-6);
        assert_eq!(counts.level1_hypo_count, 1);
        assert!((counts.level1_hypo_minutes - 25.0).abs() < 1e-6);
        assert_eq!(counts.hyper_gt_180_count, 2);
        assert!((counts.hyper_gt_180_minutes - 35.0).abs() < 1e-6);
        assert_eq!(counts.lingo_excursion_gt_140_count, 2);

        let l2 = episodes
            .iter()
            .find(|e| e.kind == EpisodeKind::Level2HypoLt54)
            .unwrap();
        assert!(l2.sensor_capped);
        assert!((l2.extreme_mg_dl - 54.0).abs() < 1e-6);

        let hypers: Vec<&GlycemicEpisode> = episodes
            .iter()
            .filter(|e| e.kind == EpisodeKind::HyperGt180)
            .collect();
        assert_eq!(hypers.len(), 2);
        assert!(hypers[0].sensor_capped);
        assert!(!hypers[1].sensor_capped);
    }

    #[test]
    fn computes_daily_circadian_sessions_roc_and_14d_period_comparison() {
        // Synthesize 30 days of 15-min readings (96/day = 2880 readings) with:
        // - Prior 14d (days 0..16): higher afternoon/evening glucose (with ceiling caps at 14:00)
        // - Recent 14d (days 16..30): improved tighter control (lower mean, higher TIR, 0 ceiling caps)
        // - A 4-hour sensor gap on day 10 (12:00 -> 16:00) to produce 2 wear sessions and 1 gap
        // - Rapid rise & fall bursts on day 20 morning
        let mut readings = Vec::new();
        for day in 0..30_i64 {
            for slot in 0..96_u32 {
                let hour = slot / 4;
                let minute = (slot % 4) * 15;
                // Introduce a 4-hour sensor gap on day 10 (12:15..16:00 omitted)
                if day == 10 && (49..64).contains(&slot) {
                    continue;
                }
                let hour_offset = day as f64 * 24.0 + hour as f64 + minute as f64 / 60.0;
                let is_recent = day >= 16;
                let (g, bound) = if day == 20 && hour == 8 && minute == 0 {
                    (90.0, GlucoseReadingBound::Measured)
                } else if day == 20 && hour == 8 && minute == 15 {
                    (135.0, GlucoseReadingBound::Measured) // +3.0 mg/dL/min rapid rise
                } else if day == 20 && hour == 8 && minute == 30 {
                    (96.0, GlucoseReadingBound::Measured) // -2.6 mg/dL/min rapid fall
                } else if !is_recent && hour == 14 && minute == 0 {
                    (201.0, GlucoseReadingBound::AboveSensorCeiling)
                } else if !is_recent && (12..18).contains(&hour) {
                    (165.0, GlucoseReadingBound::Measured)
                } else if hour < 6 {
                    (86.0, GlucoseReadingBound::Measured)
                } else if is_recent {
                    (104.0, GlucoseReadingBound::Measured)
                } else {
                    (124.0, GlucoseReadingBound::Measured)
                };
                readings.push(synthetic_reading(day, hour, minute, hour_offset, g, bound));
            }
        }

        let analytics = analyze_glucose_trace(
            &readings,
            GlucoseMonitor::AbbottLingo,
            MeasurementMethod::InterstitialCgm,
            None,
        );

        assert_eq!(analytics.daily.len(), 30);
        assert_eq!(analytics.daily[0].date, "2026-07-01");
        assert!(analytics.daily[0].overnight_mean_mg_dl.unwrap() < 90.0);
        assert!(analytics.daily[0].daytime_mean_mg_dl.unwrap() > 120.0);
        assert!(analytics.daily[10].active_wear_pct < 90.0);
        assert!(analytics.daily[0].ceiling_capped_pct > 0.0);
        assert!((analytics.daily[25].ceiling_capped_pct - 0.0).abs() < 1e-6);

        // Wear sessions & gap
        assert_eq!(analytics.sessions.len(), 2);
        assert_eq!(analytics.gaps.len(), 1);
        assert_eq!(analytics.sessions[0].session_index, 1);
        assert!(analytics.sessions[0].preceding_gap_hours.is_none());
        assert_eq!(analytics.sessions[1].session_index, 2);
        assert!((analytics.sessions[1].preceding_gap_hours.unwrap() - 4.0).abs() < 1e-6);
        assert!((analytics.gaps[0].duration_hours - 4.0).abs() < 1e-6);

        // Circadian breakdown
        assert_eq!(analytics.circadian.len(), 4);
        assert_eq!(analytics.circadian[0].period, CircadianPeriod::Overnight);
        assert!((analytics.circadian[0].mean_mg_dl - 86.0).abs() < 1e-6);
        assert!(analytics.circadian[2].mean_mg_dl > analytics.circadian[0].mean_mg_dl);

        // Rate of change
        assert!(analytics.rate_of_change.sample_pairs > 2000);
        assert!((analytics.rate_of_change.max_rise_mg_dl_per_min - 3.0).abs() < 1e-6);
        assert!(analytics.rate_of_change.max_fall_mg_dl_per_min <= -2.5);
        assert!(analytics.rate_of_change.rapid_rise_events >= 1);
        assert!(analytics.rate_of_change.rapid_fall_events >= 1);
        assert!(analytics.rate_of_change.mean_abs_roc > 0.0);

        // 14d-vs-prior-14d period comparison
        let cmp = analytics
            .period_comparison
            .as_ref()
            .expect("30-day trace produces 14d comparison");
        assert!(cmp.recent_readings > 1000 && cmp.prior_readings > 1000);
        assert!(cmp.delta_mean_mg_dl < -10.0);
        assert!(cmp.delta_gmi_pct < -0.2);
        assert!(cmp.delta_titr_pct > 15.0);
        assert!(cmp.delta_ceiling_capped_pct < 0.0);

        // Serde round-trip check
        let serialized = serde_json::to_string(&analytics).expect("analytics serializes");
        let roundtripped: AdvancedGlucoseAnalytics =
            serde_json::from_str(&serialized).expect("analytics deserializes");
        assert_eq!(roundtripped.daily.len(), 30);
        assert_eq!(roundtripped.sessions.len(), 2);
    }

    #[test]
    fn handles_empty_and_short_traces_without_panics_or_nans() {
        let empty = analyze_glucose_trace(
            &[],
            GlucoseMonitor::AbbottLingo,
            MeasurementMethod::InterstitialCgm,
            None,
        );
        assert!(empty.daily.is_empty());
        assert!(empty.episodes.is_empty());
        assert!(empty.sessions.is_empty());
        assert!(empty.gaps.is_empty());
        assert_eq!(empty.circadian.len(), 4);
        assert!(empty.circadian.iter().all(|c| c.reading_count == 0));
        assert_eq!(empty.rate_of_change.sample_pairs, 0);
        assert!(empty.period_comparison.is_none());

        // Short 10-day trace should NOT emit a 14d period comparison (< 20d span).
        let short: Vec<LingoReading> = (0..240)
            .map(|h| {
                synthetic_reading(
                    (h / 24) as i64,
                    (h % 24) as u32,
                    0,
                    h as f64,
                    102.0,
                    GlucoseReadingBound::Measured,
                )
            })
            .collect();
        let short_res = analyze_glucose_trace(
            &short,
            GlucoseMonitor::AbbottLingo,
            MeasurementMethod::InterstitialCgm,
            None,
        );
        assert!(short_res.period_comparison.is_none());
        assert_eq!(short_res.daily.len(), 10);
        assert_eq!(short_res.sessions.len(), 1);
    }
}
