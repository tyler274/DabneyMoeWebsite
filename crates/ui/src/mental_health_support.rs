//! Local-first Personal Crisis & Support Card (Stanley-Brown 6-part structure) +
//! 1-Page Printable/Exportable Clinician Brief for Dabney.moe `/mental-health`.
//!
//! All personal contacts, early-warning signs, and coping steps stay strictly in the
//! user's browser (`localStorage`), never leave the device unless explicitly printed or
//! downloaded by the user, and can be wiped in one click.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::mental_health_cgm::ClinicalGlucoseSummary;
use crate::mental_health_cgm_analytics::AdvancedGlucoseAnalytics;
use crate::mental_health_episodes::{classify_asrm, EpisodeCadenceSummary};
use crate::mental_health_lingo::{classify_gad7, classify_phq9, MentalHealthCheckIn};

#[cfg(any(feature = "hydrate", feature = "csr"))]
const STORAGE_KEY_SUPPORT_CARD: &str = "dabney.mental_health.v1.support_card";

/// Patient-owned local Stanley-Brown safety & support card persisted exclusively in `localStorage`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonalSupportCard {
    pub clinician_name: String,
    pub clinician_contact: String,
    pub urgent_clinic_note: String,
    pub trusted_person_name: String,
    pub trusted_person_contact: String,
    pub trusted_person_note: String,
    pub calming_places_note: String,
    pub safe_environment_note: String,
    pub warning_signs: Vec<String>,
    pub coping_steps: Vec<String>,
}

impl Default for PersonalSupportCard {
    fn default() -> Self {
        Self {
            clinician_name: String::new(),
            clinician_contact: String::new(),
            urgent_clinic_note: String::new(),
            trusted_person_name: String::new(),
            trusted_person_contact: String::new(),
            trusted_person_note:
                "Remind me to pause, drink water, and wait before sending or publishing big decisions."
                    .to_string(),
            calming_places_note:
                "Quiet room with dim lighting, neighborhood walk, or a calm coffee shop / library."
                    .to_string(),
            safe_environment_note:
                "Put phone/laptop on Do Not Disturb, keep water and regular meals nearby, and delay irreversible messages or purchases by 24 hours."
                    .to_string(),
            warning_signs: vec![
                "Racing thoughts, feeling unusually wired, or urgent pressure to launch/negotiate."
                    .to_string(),
                "Rapid burst messaging or urge to contact people repeatedly late at night."
                    .to_string(),
                "Skipped meals, caffeine spikes, or sharp CGM variability (> 25% CV).".to_string(),
            ],
            coping_steps: vec![
                "Step away from screens and drink a full glass of water.".to_string(),
                "Switch phone and chat apps to Do Not Disturb until morning.".to_string(),
                "Take a 10-minute walk or 5 minutes of slow box breathing (4-4-4-4).".to_string(),
                "Message or call my trusted support person or care clinician.".to_string(),
            ],
        }
    }
}

/// Verified free, confidential, non-police 24/7 crisis & mental health support resources.
pub struct CrisisResource {
    pub name: &'static str,
    pub contact: &'static str,
    pub detail: &'static str,
    pub call_href: Option<&'static str>,
    pub call_label: Option<&'static str>,
    pub sms_href: Option<&'static str>,
    pub sms_label: Option<&'static str>,
}

pub const CRISIS_RESOURCES: [CrisisResource; 3] = [
    CrisisResource {
        name: "988 Suicide & Crisis Lifeline (US / Canada)",
        contact: "Call or text 988",
        detail: "Free, confidential, 24/7 behavioral health crisis counselors; you can also ask about local non-police mobile crisis response.",
        call_href: Some("tel:988"),
        call_label: Some("Call 988"),
        sms_href: Some("sms:988"),
        sms_label: Some("Text 988"),
    },
    CrisisResource {
        name: "Crisis Text Line",
        contact: "Text HOME to 741741",
        detail: "Free, 24/7 text-based support with a trained crisis counselor.",
        call_href: None,
        call_label: None,
        sms_href: Some("sms:741741?&body=HOME"),
        sms_label: Some("Text HOME to 741741"),
    },
    CrisisResource {
        name: "SAMHSA National Helpline",
        contact: "1-800-662-4357",
        detail: "Free, confidential, 24/7 treatment referral and mental health support information.",
        call_href: Some("tel:18006624357"),
        call_label: Some("Call 1-800-662-4357"),
        sms_href: None,
        sms_label: None,
    },
];

/// Format the patient's local Stanley-Brown Personal Crisis & Support Card as clean Markdown.
pub fn render_support_card_markdown(card: &PersonalSupportCard) -> String {
    let mut md = String::from(
        "# Personal Crisis & Support Card (Stanley-Brown Safety Plan)\n\n\
         ## 1. My Early Warning Signs\n",
    );
    for (i, item) in card.warning_signs.iter().enumerate() {
        md.push_str(&format!("{}. {item}\n", i + 1));
    }
    md.push_str("\n## 2. Internal Grounding & Coping Steps\n");
    for (i, step) in card.coping_steps.iter().enumerate() {
        md.push_str(&format!("{}. {step}\n", i + 1));
    }
    md.push_str("\n## 3. Grounding Places & Calming Settings\n");
    md.push_str(&format!(
        "- {}\n",
        if card.calming_places_note.trim().is_empty() {
            "Not set"
        } else {
            card.calming_places_note.trim()
        }
    ));
    md.push_str("\n## 4. Trusted Support Person\n");
    md.push_str(&format!(
        "- **Name:** {}\n- **Contact:** {}\n- **What helps:** {}\n",
        if card.trusted_person_name.trim().is_empty() {
            "Not set"
        } else {
            card.trusted_person_name.trim()
        },
        if card.trusted_person_contact.trim().is_empty() {
            "Not set"
        } else {
            card.trusted_person_contact.trim()
        },
        card.trusted_person_note.trim(),
    ));
    md.push_str("\n## 5. Care Team, Urgent Clinic & Free 24/7 Non-Police Crisis Resources\n");
    md.push_str(&format!(
        "- **Clinician / Therapist:** {}\n- **Clinic Phone / Portal:** {}\n- **Urgent Clinic / Pharmacy Note:** {}\n",
        if card.clinician_name.trim().is_empty() {
            "Not set"
        } else {
            card.clinician_name.trim()
        },
        if card.clinician_contact.trim().is_empty() {
            "Not set"
        } else {
            card.clinician_contact.trim()
        },
        if card.urgent_clinic_note.trim().is_empty() {
            "Not set"
        } else {
            card.urgent_clinic_note.trim()
        },
    ));
    for res in &CRISIS_RESOURCES {
        md.push_str(&format!(
            "- **{}:** {} — {}\n",
            res.name, res.contact, res.detail
        ));
    }
    md.push_str("\n## 6. Making My Environment Calmer & Safer\n");
    md.push_str(&format!(
        "- {}\n",
        if card.safe_environment_note.trim().is_empty() {
            "Not set"
        } else {
            card.safe_environment_note.trim()
        }
    ));
    md
}

/// Render a clean, 1-page patient-prepared Markdown brief ready to bring to a clinician visit.
///
/// Contains only aggregate scores, clinical CGM/AGP metrics, and privacy-stripped cadence totals
/// (zero private chat text, zero third-party handles). Optionally appends the patient's own
/// Support Card when explicitly opted in.
pub fn render_clinician_brief_markdown(
    glucose: Option<&ClinicalGlucoseSummary>,
    checkins: &[MentalHealthCheckIn],
    cadence: &EpisodeCadenceSummary,
    asrm: Option<u8>,
    sleep_hours: f32,
    included_support_card: Option<&PersonalSupportCard>,
) -> String {
    render_clinician_brief_markdown_with_analytics(
        glucose,
        None,
        checkins,
        cadence,
        asrm,
        sleep_hours,
        included_support_card,
    )
}

fn format_brief_minutes(minutes: f64) -> String {
    if minutes >= 120.0 {
        format!("{:.1} h ({:.0} min)", minutes / 60.0, minutes)
    } else {
        format!("{:.0} min", minutes)
    }
}

/// Render a clean, 1-page patient-prepared Markdown brief ready to bring to a clinician visit,
/// enriched with windowed sensor saturation caveats and optional [`AdvancedGlucoseAnalytics`]
/// (sustained ATTD episodes, circadian breakdown, 14-day period comparison, and wear sessions/gaps).
pub fn render_clinician_brief_markdown_with_analytics(
    glucose: Option<&ClinicalGlucoseSummary>,
    analytics: Option<&AdvancedGlucoseAnalytics>,
    checkins: &[MentalHealthCheckIn],
    cadence: &EpisodeCadenceSummary,
    asrm: Option<u8>,
    sleep_hours: f32,
    included_support_card: Option<&PersonalSupportCard>,
) -> String {
    let latest_phq9 = checkins.iter().rev().find_map(|c| c.phq9);
    let latest_gad7 = checkins.iter().rev().find_map(|c| c.gad7);
    let avg_mood = if checkins.is_empty() {
        None
    } else {
        Some(checkins.iter().map(|c| f64::from(c.mood)).sum::<f64>() / checkins.len() as f64)
    };
    let avg_anx = if checkins.is_empty() {
        None
    } else {
        Some(checkins.iter().map(|c| f64::from(c.anxiety)).sum::<f64>() / checkins.len() as f64)
    };

    let mut md = String::from(
        "# Curie Mobile — Patient-Prepared Clinical Brief\n\n\
         > **Note:** Self-monitoring summary prepared locally by the patient for clinician review. Not an automated medical diagnosis.\n\n\
         ## 1. Mental Health Scales & Daily Check-Ins\n",
    );
    md.push_str(&format!(
        "- **PHQ-9 (Depression, 0–27):** {}\n",
        latest_phq9
            .map(|v| format!("{v}/27 ({})", classify_phq9(v).label()))
            .unwrap_or_else(|| "Not recorded".to_string())
    ));
    md.push_str(&format!(
        "- **GAD-7 (Anxiety, 0–21):** {}\n",
        latest_gad7
            .map(|v| format!("{v}/21 ({})", classify_gad7(v).label()))
            .unwrap_or_else(|| "Not recorded".to_string())
    ));
    md.push_str(&format!(
        "- **ASRM (Activation, 0–20):** {}\n",
        asrm.map(|v| format!("{v}/20 ({})", classify_asrm(v).label()))
            .unwrap_or_else(|| "Not recorded".to_string())
    ));
    md.push_str(&format!(
        "- **Recent Sleep Input:** {}\n",
        if sleep_hours > 0.0 {
            format!("{sleep_hours:.1} h")
        } else {
            "Not recorded".to_string()
        }
    ));
    if let (Some(m), Some(a)) = (avg_mood, avg_anx) {
        md.push_str(&format!(
            "- **Check-In Averages (n = {}):** Mood {m:.1}/10 · Anxiety {a:.1}/10\n",
            checkins.len()
        ));
    }

    md.push_str("\n## 2. Commercial CGM / Glucose Analytics (ADA / ATTD Consensus)\n");
    if let Some(g) = glucose {
        md.push_str(&format!(
            "- **Monitor:** {} ({} · source {})\n\
             - **Readings / Estimated Wear:** {} readings ({:.0}% estimated active wear)\n\
             - **Mean / Median / Range:** {:.1} mg/dL (median {:.1}, range {:.0}–{:.0} mg/dL)\n\
             - **GMI (est. A1C):** {:.1}%\n\
             - **Variability:** CV {:.1}% (SD {:.1} mg/dL) · MAGE {:.1} mg/dL\n\
             - **GRI (Glycemia Risk Index):** {:.0}/100 ({})\n\
             - **5-Band Time in Range:** `<54` {:.1}% · `54–69` {:.1}% · **`70–180` {:.1}%** (`70–140` tight {:.1}%) · `181–250` {:.1}% · `>250` {:.1}%\n",
            g.monitor.display_name(),
            g.method.label(),
            g.source_unit.label(),
            g.reading_count,
            g.estimated_active_wear_pct,
            g.mean_mg_dl,
            g.median_mg_dl,
            g.min_mg_dl,
            g.max_mg_dl,
            g.gmi_percent,
            g.cv_percent,
            g.std_dev_mg_dl,
            g.mage_mg_dl,
            g.gri_score,
            g.gri_zone.label(),
            g.tbr_very_low_lt_54_pct,
            g.tbr_low_54_69_pct,
            g.tir_70_180_pct,
            g.titr_70_140_pct,
            g.tar_high_181_250_pct,
            g.tar_very_high_gt_250_pct,
        ));

        if g.has_sensor_saturation() {
            let mut cap_parts = Vec::new();
            if g.floor_capped_readings > 0 {
                cap_parts.push(format!(
                    "`<55` floor (`<=54`): {} ({:.1}%)",
                    g.floor_capped_readings, g.floor_capped_pct
                ));
            }
            if g.ceiling_capped_readings > 0 {
                cap_parts.push(format!(
                    "`>200` ceiling (`>=201`): {} ({:.1}%)",
                    g.ceiling_capped_readings, g.ceiling_capped_pct
                ));
            }
            md.push_str(&format!(
                "- **Sensor Saturation Caveat (Windowed):** {} — Mean, GMI, TAR `>250`, and CV% (flat-line suppression) are lower bounds / clipped by sensor range limits.\n",
                cap_parts.join(" · ")
            ));
        }

        if let Some(a) = analytics {
            let ep = &a.episode_counts;
            md.push_str(&format!(
                "- **Sustained Episodes (≥15 min, ATTD):** `<54`/`<55` floor: {} ({}) · `<70` hypo: {} ({}) · `>180` hyper: {} ({}) · `>140` excursions: {} ({})\n",
                ep.level2_hypo_count,
                format_brief_minutes(ep.level2_hypo_minutes),
                ep.level1_hypo_count,
                format_brief_minutes(ep.level1_hypo_minutes),
                ep.hyper_gt_180_count,
                format_brief_minutes(ep.hyper_gt_180_minutes),
                ep.lingo_excursion_gt_140_count,
                format_brief_minutes(ep.lingo_excursion_gt_140_minutes),
            ));

            let circadian_parts: Vec<String> = a
                .circadian
                .iter()
                .filter(|c| c.reading_count > 0)
                .map(|c| {
                    format!(
                        "{} ({}): {:.0} mg/dL · TIR {:.0}% · TBR {:.1}%",
                        c.period.label(),
                        c.period.hours_label(),
                        c.mean_mg_dl,
                        c.tir_70_180_pct,
                        c.tbr_lt_70_pct
                    )
                })
                .collect();
            if !circadian_parts.is_empty() {
                md.push_str(&format!(
                    "- **Circadian Summary (Local Time):** {}\n",
                    circadian_parts.join(" | ")
                ));
            }

            if let Some(cmp) = &a.period_comparison {
                let gap_note = if cmp.prior_days < cmp.recent_days - 1.0 {
                    format!(
                        " *(prior span truncated to {:.1}d by sensor gaps)*",
                        cmp.prior_days
                    )
                } else {
                    String::new()
                };
                md.push_str(&format!(
                    "- **14-Day vs Prior 14-Day Trend:** Recent {:.1}d ({} rdgs) vs Prior {:.1}d ({} rdgs) — Mean {:.1} vs {:.1} mg/dL ({:+.1}) · GMI {:.2}% vs {:.2}% ({:+.2}%) · TIR {:.1}% vs {:.1}% ({:+.1}pp) · Ceiling-Cap {:.1}% vs {:.1}% ({:+.1}pp){gap_note}\n",
                    cmp.recent_days,
                    cmp.recent_readings,
                    cmp.prior_days,
                    cmp.prior_readings,
                    cmp.recent_mean_mg_dl,
                    cmp.prior_mean_mg_dl,
                    cmp.delta_mean_mg_dl,
                    cmp.recent_gmi_pct,
                    cmp.prior_gmi_pct,
                    cmp.delta_gmi_pct,
                    cmp.recent_tir_pct,
                    cmp.prior_tir_pct,
                    cmp.delta_tir_pct,
                    cmp.recent_ceiling_capped_pct,
                    cmp.prior_ceiling_capped_pct,
                    cmp.delta_ceiling_capped_pct,
                ));
            }

            if a.rate_of_change.sample_pairs > 0 {
                md.push_str(&format!(
                    "- **15-Min Velocity (RoC):** {} rapid rise burst(s) (`>= +2.0`), {} rapid fall burst(s) (`<= -2.0 mg/dL/min`) · max {:+.2} / {:+.2} mg/dL/min\n",
                    a.rate_of_change.rapid_rise_events,
                    a.rate_of_change.rapid_fall_events,
                    a.rate_of_change.max_rise_mg_dl_per_min,
                    a.rate_of_change.max_fall_mg_dl_per_min,
                ));
            }

            if !a.sessions.is_empty() {
                let longest_gap = a
                    .gaps
                    .iter()
                    .map(|gap| gap.duration_hours)
                    .fold(0.0_f64, f64::max);
                let gap_detail = if a.gaps.is_empty() {
                    "0 gaps ≥2h".to_string()
                } else {
                    format!("{} gap(s) ≥2h (longest {:.1} h)", a.gaps.len(), longest_gap)
                };
                md.push_str(&format!(
                    "- **Sensor Wear Sessions & Gaps:** {} continuous session(s) across {} calendar day(s) · {gap_detail}\n",
                    a.sessions.len(),
                    a.daily.len(),
                ));
            }
        }
    } else {
        md.push_str("- No glucose readings imported.\n");
    }

    md.push_str("\n## 3. Aggregate Circadian Activity Cadence (Timestamps Only)\n");
    if !cadence.by_platform.is_empty() {
        let platforms_line = cadence
            .by_platform
            .iter()
            .map(|b| {
                format!(
                    "{} ({} · {:.0}%)",
                    b.platform.display_name(),
                    b.total_events,
                    b.share_pct
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        md.push_str(&format!(
            "- **Opted-In Platforms Synthesized:** {platforms_line}\n"
        ));
    }
    let ratio_note = cadence
        .baseline
        .late_night_ratio
        .map(|r| format!(" ({r:.1}× personal baseline)"))
        .unwrap_or_default();
    md.push_str(&format!(
        "- **Self-Monitoring Status:** {}\n\
         - **Recent 48h Late-Night Activity (00:00–05:00):** {} events ({:.0}% of 48h activity{ratio_note}), {} late-night calls\n\
         - **Rapid-Burst Clusters (≥4 events / 15m):** {}\n\
         - **Longest Quiet Window (48h):** {:.1} h\n",
        cadence.activation.label(),
        cadence.late_night_events,
        cadence.baseline.recent_48h_late_night_pct,
        cadence.late_night_calls,
        cadence.rapid_burst_clusters,
        cadence.longest_quiet_gap_hours,
    ));

    if let Some(card) = included_support_card {
        md.push_str("\n---\n\n");
        md.push_str(&render_support_card_markdown(card));
    }

    md
}

/// Percent-encode Markdown text for a local `data:text/markdown;charset=utf-8,...` download link.
pub fn markdown_data_uri(md: &str) -> String {
    let mut encoded = String::from("data:text/markdown;charset=utf-8,");
    for byte in md.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn read_support_card() -> Option<PersonalSupportCard> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(STORAGE_KEY_SUPPORT_CARD).ok()??;
    serde_json::from_str(&raw).ok()
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn write_support_card(card: &PersonalSupportCard) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        if let Ok(json) = serde_json::to_string(card) {
            let _ = storage.set_item(STORAGE_KEY_SUPPORT_CARD, &json);
        }
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn clear_support_card() {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.remove_item(STORAGE_KEY_SUPPORT_CARD);
    }
}

/// On-Device Stanley-Brown Personal Crisis & Support Card + 1-Page Printable Clinician Visit Brief.
#[component]
pub fn SupportAndClinicianBriefSection(
    glucose_summary: Signal<Option<ClinicalGlucoseSummary>>,
    #[prop(optional)] analytics: Option<Signal<Option<AdvancedGlucoseAnalytics>>>,
    checkins: Signal<Vec<MentalHealthCheckIn>>,
    cadence_summary: Signal<EpisodeCadenceSummary>,
    asrm_score: Signal<Option<u8>>,
    sleep_hours: Signal<f32>,
    #[prop(optional)] external_card: Option<RwSignal<PersonalSupportCard>>,
) -> impl IntoView {
    let card = external_card.unwrap_or_else(|| RwSignal::new(PersonalSupportCard::default()));
    let new_warning = RwSignal::new(String::new());
    let new_step = RwSignal::new(String::new());
    let include_card_in_brief = RwSignal::new(false);
    let card_status = RwSignal::new(Option::<String>::None);
    let confirm_wipe_card = RwSignal::new(false);
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    let hydrated = RwSignal::new(false);

    // Memoize the rendered clinician brief so Support Card keystrokes do NOT trigger redundant
    // CGM summary/analytics reads or double Markdown rendering when `include_card_in_brief` is off.
    let brief_markdown = Memo::new(move |_| {
        let g = glucose_summary.get();
        let a = analytics.and_then(|sig| sig.get());
        let c = checkins.get();
        let cad = cadence_summary.get();
        let current_card = include_card_in_brief.get().then(|| card.get());
        render_clinician_brief_markdown_with_analytics(
            g.as_ref(),
            a.as_ref(),
            &c,
            &cad,
            asrm_score.get(),
            sleep_hours.get(),
            current_card.as_ref(),
        )
    });

    #[cfg(any(feature = "hydrate", feature = "csr"))]
    Effect::new(move |_| {
        if hydrated.get_untracked() {
            return;
        }
        if let Some(saved) = read_support_card() {
            card.set(saved);
        }
        hydrated.set(true);
    });

    #[cfg(any(feature = "hydrate", feature = "csr"))]
    Effect::new(move |_| {
        let current = card.get();
        if hydrated.get() {
            write_support_card(&current);
        }
    });

    let add_warning_sign = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let trimmed = new_warning.get().trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        card.update(|c| c.warning_signs.push(trimmed));
        new_warning.set(String::new());
    };

    let add_coping_step = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let trimmed = new_step.get().trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        card.update(|c| c.coping_steps.push(trimmed));
        new_step.set(String::new());
    };

    let reset_support_card = move |_| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        clear_support_card();
        card.set(PersonalSupportCard::default());
        confirm_wipe_card.set(false);
        card_status.set(Some(
            "Local support card wiped and reset to defaults.".into(),
        ));
    };

    let trigger_print = move |_| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        if let Some(win) = web_sys::window() {
            let _ = win.print();
        }
    };

    view! {
        <div class="print-brief-container mt-8 grid gap-6 lg:grid-cols-2 print:mt-0 print:block">
            // 1. On-Device Stanley-Brown Personal Crisis & Support Card
            <section class="print-hide print:hidden rounded-2xl border border-emerald-400/20 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-start justify-between gap-2">
                    <div>
                        <p class="mb-1 text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "Patient-Owned · Stanley-Brown Safety Plan · Local Only"
                        </p>
                        <h2 class="text-lg font-semibold text-white">
                            "Personal Crisis & Support Card"
                        </h2>
                        <p class="mt-1 text-xs text-slate-400">
                            "Keep your early warning signs, coping steps, care team, trusted person, and verified 24/7 non-police support lines privately on this device."
                        </p>
                    </div>
                    <div class="flex flex-wrap items-center gap-1.5">
                        {move || {
                            let href = markdown_data_uri(&render_support_card_markdown(&card.get()));
                            view! {
                                <a
                                    href=href
                                    download="curie-personal-support-card.md"
                                    class="inline-flex min-h-[36px] items-center rounded-full border border-emerald-400/30 px-3 py-1 text-xs font-medium text-emerald-300 transition hover:bg-emerald-500/10"
                                >
                                    "Save card .md"
                                </a>
                            }
                        }}
                        {move || if confirm_wipe_card.get() {
                            view! {
                                <div class="flex flex-wrap items-center gap-1.5 rounded-xl border border-rose-400/40 bg-rose-950/60 px-2.5 py-1">
                                    <span class="text-[11px] font-medium text-rose-100">
                                        "Wipe safety card?"
                                    </span>
                                    <button
                                        type="button"
                                        on:click=reset_support_card
                                        class="min-h-[36px] rounded-full bg-rose-500 px-3 py-1 text-xs font-semibold text-white transition hover:bg-rose-400"
                                    >
                                        "Confirm wipe"
                                    </button>
                                    <button
                                        type="button"
                                        on:click=move |_| confirm_wipe_card.set(false)
                                        class="min-h-[36px] rounded-full border border-white/20 px-3 py-1 text-xs text-slate-200 transition hover:bg-white/10"
                                    >
                                        "Cancel"
                                    </button>
                                </div>
                            }
                            .into_any()
                        } else {
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| confirm_wipe_card.set(true)
                                    class="min-h-[36px] rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:border-rose-400/40 hover:bg-rose-500/10 hover:text-rose-200"
                                >
                                    "Wipe card"
                                </button>
                            }
                            .into_any()
                        }}
                    </div>
                </div>

                // Verified 24/7 non-police crisis resources with tappable tel:/sms: links (>=44px tap targets)
                <div class="mb-5 space-y-2.5">
                    {CRISIS_RESOURCES.iter().map(|res| view! {
                        <div class="rounded-xl border border-emerald-400/25 bg-emerald-500/5 px-3.5 py-3">
                            <div class="flex flex-wrap items-center justify-between gap-2">
                                <span class="text-xs font-semibold text-white">{res.name}</span>
                                <div class="flex flex-wrap items-center gap-2">
                                    {res.call_href.zip(res.call_label).map(|(href, label)| view! {
                                        <a
                                            href=href
                                            class="inline-flex min-h-[44px] items-center rounded-full border border-emerald-400/45 bg-emerald-500/15 px-3.5 py-2 font-mono text-xs font-bold text-emerald-200 transition hover:bg-emerald-500/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400"
                                        >
                                            {label}
                                        </a>
                                    })}
                                    {res.sms_href.zip(res.sms_label).map(|(href, label)| view! {
                                        <a
                                            href=href
                                            class="inline-flex min-h-[44px] items-center rounded-full border border-cyan-400/45 bg-cyan-500/15 px-3.5 py-2 font-mono text-xs font-bold text-cyan-200 transition hover:bg-cyan-500/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400"
                                        >
                                            {label}
                                        </a>
                                    })}
                                </div>
                            </div>
                            <p class="mt-1.5 text-[11px] text-slate-300">{res.detail}</p>
                        </div>
                    }).collect_view()}
                </div>

                <div class="space-y-5">
                    // Step 1: Personal Early Warning Signs
                    <div class="space-y-2">
                        <p class="text-xs font-semibold text-amber-300">"1. My early warning signs"</p>
                        <ul class="space-y-1.5 text-xs text-slate-300">
                            {move || card.get().warning_signs.into_iter().enumerate().map(|(i, sign)| view! {
                                <li class="flex items-center justify-between gap-2 rounded-lg border border-white/5 bg-slate-950/60 py-1 pr-1 pl-2.5">
                                    <span class="flex items-start gap-2 py-1">
                                        <span class="font-mono text-[11px] text-amber-300">{format!("{}.", i + 1)}</span>
                                        <span>{sign}</span>
                                    </span>
                                    <button
                                        type="button"
                                        aria-label="Remove warning sign"
                                        on:click=move |_| {
                                            card.update(|c| {
                                                if i < c.warning_signs.len() {
                                                    c.warning_signs.remove(i);
                                                }
                                            });
                                        }
                                        class="inline-flex min-h-[40px] min-w-[40px] shrink-0 items-center justify-center rounded-lg text-base text-slate-400 transition hover:bg-rose-500/15 hover:text-rose-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-400"
                                    >
                                        "×"
                                    </button>
                                </li>
                            }).collect_view()}
                        </ul>
                        <form on:submit=add_warning_sign class="flex gap-2">
                            <input
                                type="text"
                                aria-label="Add an early warning sign"
                                placeholder="Add an early warning sign..."
                                prop:value=move || new_warning.get()
                                on:input=move |ev| new_warning.set(event_target_value(&ev))
                                class="min-h-[40px] flex-1 rounded-lg border border-white/10 bg-slate-900 px-3 py-1.5 text-xs text-white placeholder:text-slate-500"
                            />
                            <button
                                type="submit"
                                class="min-h-[40px] rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                            >
                                "Add sign"
                            </button>
                        </form>
                    </div>

                    // Step 2: Internal Grounding & Coping Steps
                    <div class="space-y-2">
                        <p class="text-xs font-semibold text-emerald-300">"2. Personal grounding & coping steps"</p>
                        <ul class="space-y-1.5 text-xs text-slate-300">
                            {move || card.get().coping_steps.into_iter().enumerate().map(|(i, step)| view! {
                                <li class="flex items-center justify-between gap-2 rounded-lg border border-white/5 bg-slate-950/60 py-1 pr-1 pl-2.5">
                                    <span class="flex items-start gap-2 py-1">
                                        <span class="font-mono text-[11px] text-emerald-300">{format!("{}.", i + 1)}</span>
                                        <span>{step}</span>
                                    </span>
                                    <button
                                        type="button"
                                        aria-label="Remove coping step"
                                        on:click=move |_| {
                                            card.update(|c| {
                                                if i < c.coping_steps.len() {
                                                    c.coping_steps.remove(i);
                                                }
                                            });
                                        }
                                        class="inline-flex min-h-[40px] min-w-[40px] shrink-0 items-center justify-center rounded-lg text-base text-slate-400 transition hover:bg-rose-500/15 hover:text-rose-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-400"
                                    >
                                        "×"
                                    </button>
                                </li>
                            }).collect_view()}
                        </ul>
                        <form on:submit=add_coping_step class="flex gap-2 pt-1">
                            <input
                                type="text"
                                aria-label="Add a personal grounding step"
                                placeholder="Add a personal grounding step..."
                                prop:value=move || new_step.get()
                                on:input=move |ev| new_step.set(event_target_value(&ev))
                                class="min-h-[40px] flex-1 rounded-lg border border-white/10 bg-slate-900 px-3 py-1.5 text-xs text-white placeholder:text-slate-500"
                            />
                            <button
                                type="submit"
                                class="min-h-[40px] rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                            >
                                "Add step"
                            </button>
                        </form>
                    </div>

                    // Step 3: Grounding Places & Calming Settings
                    <div class="space-y-1.5">
                        <label class="block text-xs font-semibold text-cyan-300">
                            "3. Grounding places & calming settings"
                            <input
                                type="text"
                                placeholder="e.g. Quiet room, neighborhood walk, library"
                                prop:value=move || card.get().calming_places_note
                                on:input=move |ev| {
                                    let v = event_target_value(&ev);
                                    card.update(|c| c.calming_places_note = v);
                                }
                                class="mt-1.5 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-2 text-xs font-normal text-white placeholder:text-slate-500"
                            />
                        </label>
                    </div>

                    // Step 4: Trusted Support Person
                    <div class="space-y-2">
                        <p class="text-xs font-semibold text-cyan-300">"4. Trusted support person"</p>
                        <div class="grid gap-3 sm:grid-cols-2">
                            <label class="block text-xs text-slate-300">
                                "Trusted friend / family member"
                                <input
                                    type="text"
                                    placeholder="Name"
                                    prop:value=move || card.get().trusted_person_name
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        card.update(|c| c.trusted_person_name = v);
                                    }
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "Trusted contact phone"
                                <input
                                    type="text"
                                    placeholder="Phone or preferred method"
                                    prop:value=move || card.get().trusted_person_contact
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        card.update(|c| c.trusted_person_contact = v);
                                    }
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                                />
                            </label>
                            <label class="block text-xs text-slate-300 sm:col-span-2">
                                "What helps my trusted person say or do"
                                <input
                                    type="text"
                                    placeholder="e.g. Remind me to pause and wait before sending big messages"
                                    prop:value=move || card.get().trusted_person_note
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        card.update(|c| c.trusted_person_note = v);
                                    }
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                                />
                            </label>
                        </div>
                    </div>

                    // Step 5: Care Team & Urgent Clinic
                    <div class="space-y-2">
                        <p class="text-xs font-semibold text-cyan-300">"5. Care team & urgent clinic"</p>
                        <div class="grid gap-3 sm:grid-cols-2">
                            <label class="block text-xs text-slate-300">
                                "Care clinician / therapist"
                                <input
                                    type="text"
                                    placeholder="e.g. Dr. Rivera (Psychiatry)"
                                    prop:value=move || card.get().clinician_name
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        card.update(|c| c.clinician_name = v);
                                    }
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "Clinic phone / portal"
                                <input
                                    type="text"
                                    placeholder="e.g. (555) 010-2000"
                                    prop:value=move || card.get().clinician_contact
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        card.update(|c| c.clinician_contact = v);
                                    }
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                                />
                            </label>
                            <label class="block text-xs text-slate-300 sm:col-span-2">
                                "Urgent behavioral health clinic / pharmacy note"
                                <input
                                    type="text"
                                    placeholder="e.g. Preferred walk-in crisis clinic, pharmacy hours, or medication reminder"
                                    prop:value=move || card.get().urgent_clinic_note
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        card.update(|c| c.urgent_clinic_note = v);
                                    }
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                                />
                            </label>
                        </div>
                    </div>

                    // Step 6: Making My Environment Calmer & Safer
                    <div class="space-y-1.5">
                        <label class="block text-xs font-semibold text-cyan-300">
                            "6. Making my environment calmer & safer"
                            <input
                                type="text"
                                placeholder="e.g. Do Not Disturb on devices, water nearby, 24h wait on big decisions"
                                prop:value=move || card.get().safe_environment_note
                                on:input=move |ev| {
                                    let v = event_target_value(&ev);
                                    card.update(|c| c.safe_environment_note = v);
                                }
                                class="mt-1.5 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-2 text-xs font-normal text-white placeholder:text-slate-500"
                            />
                        </label>
                    </div>

                    {move || card_status.get().map(|msg| view! {
                        <p role="status" aria-live="polite" class="text-xs text-emerald-300">{msg}</p>
                    })}
                </div>
            </section>

            // 2. 1-Page Printable / Exportable Clinician Visit Brief
            <section class="print-brief-card rounded-2xl border border-cyan-400/20 bg-white/[0.02] p-6">
                <div class="print-hide print:hidden mb-4 flex flex-wrap items-start justify-between gap-2">
                    <div>
                        <p class="mb-1 text-xs font-semibold uppercase tracking-widest text-cyan-300">
                            "Patient-Consented Clinical Handoff"
                        </p>
                        <h2 class="text-lg font-semibold text-white">
                            "1-Page Clinician Visit Brief"
                        </h2>
                        <p class="mt-1 text-xs text-slate-400">
                            "Bring a structured summary of PHQ-9, GAD-7, ASRM, sleep, ADA CGM/AGP metrics, and aggregate cadence to your doctor or therapist."
                        </p>
                    </div>
                    {move || {
                        let href = markdown_data_uri(&brief_markdown.get());
                        view! {
                            <div class="flex flex-wrap gap-2">
                                <button
                                    type="button"
                                    on:click=trigger_print
                                    class="min-h-[36px] rounded-full bg-cyan-500 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-cyan-400"
                                >
                                    "Print / Save PDF"
                                </button>
                                <a
                                    href=href
                                    download="curie-clinician-visit-brief.md"
                                    class="inline-flex min-h-[36px] items-center rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                                >
                                    "Download .md"
                                </a>
                            </div>
                        }
                    }}
                </div>
                <label class="print-hide print:hidden mb-3 inline-flex cursor-pointer items-center gap-2 text-xs text-slate-300">
                    <input
                        type="checkbox"
                        prop:checked=move || include_card_in_brief.get()
                        on:change=move |_| include_card_in_brief.update(|v| *v = !*v)
                        class="accent-cyan-400"
                    />
                    "Include my Personal Support Card in the printed/exported brief (off by default for privacy)"
                </label>
                {move || {
                    let md = brief_markdown.get();
                    view! {
                        <pre class="print-brief-pre max-h-[480px] overflow-auto whitespace-pre-wrap rounded-xl border border-white/10 bg-slate-950 p-4 font-mono text-xs leading-relaxed text-slate-200">
                            {md}
                        </pre>
                    }
                }}
            </section>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_health_cgm::{
        parse_commercial_glucose_csv, summarize_commercial_glucose, SAMPLE_DEXCOM_CLARITY_CSV,
    };
    use crate::mental_health_episodes::{
        parse_self_activity_log, summarize_episode_cadence, SAMPLE_SELF_ACTIVITY_CSV,
    };
    use crate::mental_health_lingo::sample_checkins;
    use leptos::prelude::RenderHtml;

    #[test]
    fn generates_complete_non_diagnostic_clinician_brief_and_support_card_markdown() {
        let dexcom = parse_commercial_glucose_csv(SAMPLE_DEXCOM_CLARITY_CSV, None).unwrap();
        let glucose = summarize_commercial_glucose(
            &dexcom.readings,
            dexcom.monitor,
            dexcom.source_unit,
            dexcom.method,
        );
        let checkins = sample_checkins();
        let events = parse_self_activity_log(SAMPLE_SELF_ACTIVITY_CSV).unwrap();
        let cadence = summarize_episode_cadence(&events, Some(6), 6.0);
        let card = PersonalSupportCard::default();

        let md = render_clinician_brief_markdown(
            glucose.as_ref(),
            &checkins,
            &cadence,
            Some(6),
            6.0,
            None,
        );
        assert!(md.contains("Not an automated medical diagnosis"));
        assert!(md.contains("PHQ-9 (Depression, 0–27):"));
        assert!(md.contains("GAD-7 (Anxiety, 0–21):"));
        assert!(md.contains("ASRM (Activation, 0–20):"));
        assert!(md.contains("Dexcom G6 / G7 / ONE+ (Clarity)"));
        assert!(!md.contains("Stanley-Brown Safety Plan"));

        let md_with_card = render_clinician_brief_markdown(
            glucose.as_ref(),
            &checkins,
            &cadence,
            Some(6),
            6.0,
            Some(&card),
        );
        assert!(md_with_card.contains("Stanley-Brown Safety Plan"));
        assert!(md_with_card.contains("## 3. Grounding Places & Calming Settings"));
        assert!(md_with_card.contains("## 6. Making My Environment Calmer & Safer"));
        assert!(md_with_card.contains("Call or text 988"));
        assert!(markdown_data_uri(&md_with_card).starts_with("data:text/markdown;charset=utf-8,"));
    }

    #[test]
    fn renders_support_card_with_988_warning_signs_and_clinician_brief_on_ssr() {
        let dexcom = parse_commercial_glucose_csv(SAMPLE_DEXCOM_CLARITY_CSV, None).unwrap();
        let glucose = RwSignal::new(summarize_commercial_glucose(
            &dexcom.readings,
            dexcom.monitor,
            dexcom.source_unit,
            dexcom.method,
        ));
        let checkins = RwSignal::new(sample_checkins());
        let events = parse_self_activity_log(SAMPLE_SELF_ACTIVITY_CSV).unwrap();
        let cadence = RwSignal::new(summarize_episode_cadence(&events, Some(4), 7.5));
        let asrm = RwSignal::new(Some(4u8));
        let sleep = RwSignal::new(7.5f32);

        let html = view! {
            <SupportAndClinicianBriefSection
                glucose_summary=glucose.into()
                checkins=checkins.into()
                cadence_summary=cadence.into()
                asrm_score=asrm.into()
                sleep_hours=sleep.into()
            />
        }
        .to_html();
        assert!(html.contains("Personal Crisis &amp; Support Card"));
        assert!(html.contains("988 Suicide &amp; Crisis Lifeline"));
        assert!(html.contains("href=\"tel:988\""));
        assert!(html.contains("href=\"sms:988\""));
        assert!(html.contains("href=\"sms:741741?&amp;body=HOME\""));
        assert!(html.contains("href=\"tel:18006624357\""));
        assert!(html.contains("My early warning signs"));
        assert!(html.contains("Save card .md"));
        assert!(html.contains("1-Page Clinician Visit Brief"));
        assert!(html.contains("Print / Save PDF"));
        assert!(html.contains("print-brief-pre"));
        // Verify Stanley-Brown 1 -> 2 -> 3 -> 4 -> 5 -> 6 sequential ordering in rendered HTML.
        let p1 = html.find("1. My early warning signs").unwrap();
        let p2 = html.find("2. Personal grounding").unwrap();
        let p3 = html.find("3. Grounding places").unwrap();
        let p4 = html.find("4. Trusted support person").unwrap();
        let p5 = html.find("5. Care team").unwrap();
        let p6 = html.find("6. Making my environment").unwrap();
        assert!(p1 < p2 && p2 < p3 && p3 < p4 && p4 < p5 && p5 < p6);
    }

    #[test]
    fn clinician_brief_with_analytics_includes_saturation_episodes_circadian_trend_and_sessions() {
        use crate::mental_health_cgm::{
            parse_commercial_glucose_input, summarize_commercial_glucose_with_window,
            GlucoseMonitor,
        };
        use crate::mental_health_cgm_analytics::analyze_glucose_trace;

        let mut csv = String::from("Timestamp,Glucose (mg/dL)\n");
        for day in 1..=26 {
            if (5..=9).contains(&day) {
                continue;
            }
            for step in 0..24 {
                let val = if day <= 12 {
                    108
                } else if step < 2 && day == 25 {
                    54
                } else if (12..=16).contains(&step) {
                    201
                } else {
                    132
                };
                csv.push_str(&format!("2025-06-{day:02}T{step:02}:00-07:00,{val}\n"));
            }
        }
        // Add a sustained 20-minute floor hypo on day 26
        for min in [0, 5, 10, 15, 20] {
            csv.push_str(&format!("2025-06-26T02:{min:02}-07:00,54\n"));
        }

        let import =
            parse_commercial_glucose_input(&csv, Some(GlucoseMonitor::AbbottLingo)).unwrap();
        let summary = summarize_commercial_glucose_with_window(
            &import.readings,
            import.monitor,
            import.source_unit,
            import.method,
            None,
        )
        .unwrap();
        let analytics = analyze_glucose_trace(
            &import.readings,
            import.monitor,
            import.method,
            Some(&import.readings),
        );
        let cadence = summarize_episode_cadence(&[], None, 0.0);

        let md = render_clinician_brief_markdown_with_analytics(
            Some(&summary),
            Some(&analytics),
            &[],
            &cadence,
            None,
            0.0,
            None,
        );
        assert!(md.contains("Recent Sleep Input:** Not recorded"));
        assert!(md.contains("Sensor Saturation Caveat (Windowed):"));
        assert!(md.contains("`>200` ceiling (`>=201`):"));
        assert!(md.contains("`<55` floor (`<=54`):"));
        assert!(md.contains("Sustained Episodes (≥15 min, ATTD):"));
        assert!(md.contains("Circadian Summary (Local Time):"));
        assert!(md.contains("Overnight (00:00–06:00):"));
        assert!(md.contains("14-Day vs Prior 14-Day Trend:"));
        assert!(md.contains("Sensor Wear Sessions & Gaps:"));

        // Verify optional `analytics` prop renders on SSR
        let g_sig = RwSignal::new(Some(summary));
        let a_sig = RwSignal::new(Some(analytics));
        let c_sig = RwSignal::new(Vec::<MentalHealthCheckIn>::new());
        let cad_sig = RwSignal::new(cadence);
        let asrm_sig = RwSignal::new(None::<u8>);
        let sleep_sig = RwSignal::new(7.0_f32);

        let html = view! {
            <SupportAndClinicianBriefSection
                glucose_summary=g_sig.into()
                analytics=a_sig.into()
                checkins=c_sig.into()
                cadence_summary=cad_sig.into()
                asrm_score=asrm_sig.into()
                sleep_hours=sleep_sig.into()
            />
        }
        .to_html();
        assert!(html.contains("Sensor Saturation Caveat (Windowed):"));
        assert!(html.contains("14-Day vs Prior 14-Day Trend:"));
    }
}
