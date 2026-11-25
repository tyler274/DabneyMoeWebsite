//! Clean-room Dabney.moe clinical workspace: typed patient charts, grounded
//! citation search, and an optional Axum HTTP API (`ssr` feature).
//!
//! All patient records in this crate are fictional synthetic examples authored
//! from scratch for demonstration purposes and contain zero real patient data.

use serde::{Deserialize, Serialize};

/// Category of an individual record inside a synthetic patient chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    LabResult,
    ProgressNote,
    CgmSummary,
    PatientMessage,
    Imaging,
}

impl RecordKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LabResult => "Lab result",
            Self::ProgressNote => "Progress note",
            Self::CgmSummary => "CGM summary",
            Self::PatientMessage => "Patient message",
            Self::Imaging => "Imaging",
        }
    }
}

/// Priority classification for a physician queue item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    High,
    Moderate,
    Routine,
}

impl Urgency {
    pub const fn label(self) -> &'static str {
        match self {
            Self::High => "Priority",
            Self::Moderate => "Follow-up",
            Self::Routine => "Routine",
        }
    }
}

/// Current standing of a clinical follow-up commitment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpStanding {
    DueToday,
    Monitoring,
    Completed,
}

impl FollowUpStanding {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DueToday => "Due today",
            Self::Monitoring => "Monitoring",
            Self::Completed => "Completed",
        }
    }
}

/// Active medication entry on a synthetic chart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Medication {
    pub name: String,
    pub dose: String,
    pub schedule: String,
    pub indication: String,
}

/// Active clinical problem entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub code: String,
    pub label: String,
    pub status: String,
}

/// Individual longitudinal source record with stable ID for verbatim citation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartRecord {
    pub id: String,
    pub kind: RecordKind,
    pub title: String,
    pub recorded_at: String,
    pub source_label: String,
    pub body: String,
    pub key_values: Vec<(String, String)>,
}

/// Scheduled appointment slot for the synthetic clinic day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisitSlot {
    pub starts_at: String,
    pub reason: String,
    pub modality: String,
}

/// Clinical follow-up item tracked on the chart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUp {
    pub id: String,
    pub summary: String,
    pub standing: FollowUpStanding,
    pub next_check: String,
}

/// Optional physician-reviewable reply draft for an inbound patient message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyDraft {
    pub message_record_id: String,
    pub patient_question: String,
    pub proposed_reply: String,
    pub basis: String,
}

/// Complete synthetic patient chart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientChart {
    pub id: String,
    pub display_name: String,
    pub mrn: String,
    pub birth_date: String,
    pub primary_focus: String,
    pub summary: String,
    pub urgency: Urgency,
    pub visit: Option<VisitSlot>,
    pub problems: Vec<Problem>,
    pub medications: Vec<Medication>,
    pub records: Vec<ChartRecord>,
    pub follow_ups: Vec<FollowUp>,
    pub reply_draft: Option<ReplyDraft>,
}

/// Compact patient directory row returned by `/patients` and `/today`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientSummary {
    pub id: String,
    pub display_name: String,
    pub mrn: String,
    pub birth_date: String,
    pub primary_focus: String,
    pub urgency: Urgency,
    pub visit_time: Option<String>,
    pub record_count: usize,
    pub open_follow_ups: usize,
}

/// Clinic-day overview returned by `GET /today`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodayOverview {
    pub practice_name: String,
    pub clinic_date: String,
    pub synthetic_notice: String,
    pub patients: Vec<PatientSummary>,
}

/// Exact citation tying an answer back to a single record on the same patient's chart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCitation {
    pub record_id: String,
    pub title: String,
    pub recorded_at: String,
    pub kind: RecordKind,
    pub excerpt: String,
}

/// Request payload for `POST /patients/{id}/chat`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub question: String,
}

/// Structured, chart-grounded response returned for a physician question.
///
/// `grounded` is `true` exactly when `citations` is non-empty: an answer is
/// only grounded when at least one of the patient's own records matched the
/// question. Construct answers through [`answer_chart_question`], which keeps
/// the two in step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAnswer {
    pub patient_id: String,
    pub question: String,
    pub grounded: bool,
    pub answer: String,
    pub citations: Vec<ChatCitation>,
    pub basis: String,
}

/// Health payload returned by `GET /health`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub synthetic_data: bool,
    pub patient_count: usize,
}

/// Demo session payload returned by `POST /auth/demo-session`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoSessionResponse {
    pub session_id: String,
    pub clinician_label: String,
    pub mode: String,
    pub synthetic_data: bool,
}

/// Builds the built-in panel of four original fictional synthetic patients.
pub fn synthetic_panel() -> Vec<PatientChart> {
    vec![
        PatientChart {
            id: "pt-nadia-kulkarni".into(),
            display_name: "Nadia Kulkarni".into(),
            mrn: "DM-1042".into(),
            birth_date: "1988-04-14".into(),
            primary_focus: "Type 2 diabetes & 14-day CGM review".into(),
            summary: "HbA1c improved from 7.4% to 6.7% on metformin XR 1000 mg daily; \
                      14-day CGM shows 78% time-in-range with mild morning fasting rise."
                .into(),
            urgency: Urgency::High,
            visit: Some(VisitSlot {
                starts_at: "09:30".into(),
                reason: "Quarterly metabolic & CGM review".into(),
                modality: "Video visit".into(),
            }),
            problems: vec![
                Problem {
                    code: "E11.9".into(),
                    label: "Type 2 diabetes mellitus without complications".into(),
                    status: "Active · improving".into(),
                },
                Problem {
                    code: "E78.5".into(),
                    label: "Hyperlipidemia".into(),
                    status: "Active · stable on diet".into(),
                },
            ],
            medications: vec![Medication {
                name: "Metformin ER".into(),
                dose: "1000 mg".into(),
                schedule: "Once daily with dinner".into(),
                indication: "Glycemic control".into(),
            }],
            records: vec![
                ChartRecord {
                    id: "nk-cgm-01".into(),
                    kind: RecordKind::CgmSummary,
                    title: "14-Day Continuous Glucose Summary".into(),
                    recorded_at: "2026-11-20".into(),
                    source_label: "Home CGM Export".into(),
                    body: "14-day sensor wear 96%. Mean glucose 136 mg/dL, GMI 6.6%, CV 23.4%. \
                           Time in range (70-180 mg/dL) 78%, above 180 mg/dL 21%, below 70 mg/dL 1%. \
                           Fasting glucose climbs from ~114 mg/dL at 04:00 to ~146 mg/dL by 07:30."
                        .into(),
                    key_values: vec![
                        ("Time in range".into(), "78%".into()),
                        ("Mean glucose".into(), "136 mg/dL".into()),
                        ("GMI".into(), "6.6%".into()),
                        ("CV".into(), "23.4%".into()),
                    ],
                },
                ChartRecord {
                    id: "nk-lab-02".into(),
                    kind: RecordKind::LabResult,
                    title: "HbA1c & Comprehensive Metabolic Panel".into(),
                    recorded_at: "2026-11-18".into(),
                    source_label: "Regional Reference Lab".into(),
                    body: "HbA1c 6.7% (prior 7.4% six months ago). Fasting glucose 134 mg/dL. \
                           Creatinine 0.78 mg/dL, eGFR >90 mL/min/1.73m2, ALT 21 U/L, potassium 4.1 mEq/L."
                        .into(),
                    key_values: vec![
                        ("HbA1c".into(), "6.7%".into()),
                        ("Fasting glucose".into(), "134 mg/dL".into()),
                        ("eGFR".into(), ">90".into()),
                    ],
                },
                ChartRecord {
                    id: "nk-msg-03".into(),
                    kind: RecordKind::PatientMessage,
                    title: "Morning glucose drift question".into(),
                    recorded_at: "2026-11-23".into(),
                    source_label: "Patient Portal".into(),
                    body: "Hi Dr. Patel, my numbers stay flat overnight until around 5 AM when they \
                           rise to the 140s before breakfast even when I skip late snacks. Should I \
                           shift when I take metformin or wait for our visit?"
                        .into(),
                    key_values: vec![("Reported fasting peak".into(), "~145 mg/dL".into())],
                },
            ],
            follow_ups: vec![FollowUp {
                id: "nk-fu-1".into(),
                summary: "Review dawn-phenomenon CGM curve and evening metformin timing at 09:30 visit".into(),
                standing: FollowUpStanding::DueToday,
                next_check: "Today 09:30".into(),
            }],
            reply_draft: Some(ReplyDraft {
                message_record_id: "nk-msg-03".into(),
                patient_question: "Should I shift metformin timing before our visit?".into(),
                proposed_reply: "Thanks for sharing your CGM trend — your 14-day time-in-range is 78% \
                                 and your HbA1c dropped to 6.7%. Please keep metformin ER 1000 mg with \
                                 dinner as prescribed for now; we will review the early-morning rise \
                                 together during your 9:30 visit and decide whether a timing or dose \
                                 adjustment makes sense."
                    .into(),
                basis: "Grounded in nk-cgm-01 (TIR 78%, 04:00-07:30 fasting drift) and nk-lab-02 (HbA1c 6.7%, eGFR >90).".into(),
            }),
        },
        PatientChart {
            id: "pt-carlos-mendez".into(),
            display_name: "Carlos Mendez".into(),
            mrn: "DM-1089".into(),
            birth_date: "1971-09-22".into(),
            primary_focus: "Resistant hypertension & renal monitoring".into(),
            summary: "Two-week home BP log averages 138/84 mmHg on lisinopril 20 mg and \
                      amlodipine 5 mg; stable mild CKD stage 2 with normal potassium."
                .into(),
            urgency: Urgency::Moderate,
            visit: Some(VisitSlot {
                starts_at: "11:00".into(),
                reason: "Blood pressure titration check".into(),
                modality: "In clinic".into(),
            }),
            problems: vec![
                Problem {
                    code: "I10".into(),
                    label: "Essential hypertension".into(),
                    status: "Active · above target (<130/80)".into(),
                },
                Problem {
                    code: "N18.2".into(),
                    label: "Chronic kidney disease, stage 2".into(),
                    status: "Active · stable".into(),
                },
            ],
            medications: vec![
                Medication {
                    name: "Lisinopril".into(),
                    dose: "20 mg".into(),
                    schedule: "Once daily in morning".into(),
                    indication: "Hypertension & renal protection".into(),
                },
                Medication {
                    name: "Amlodipine".into(),
                    dose: "5 mg".into(),
                    schedule: "Once daily in morning".into(),
                    indication: "Hypertension".into(),
                },
            ],
            records: vec![
                ChartRecord {
                    id: "cm-note-01".into(),
                    kind: RecordKind::ProgressNote,
                    title: "Home Blood Pressure Log Review".into(),
                    recorded_at: "2026-11-21".into(),
                    source_label: "Home Cuff Sync".into(),
                    body: "24 seated morning and evening readings over 14 days: average 138/84 mmHg \
                           (range 130/78 to 146/90). No headaches, chest pain, dyspnea, or ankle edema \
                           on amlodipine 5 mg."
                        .into(),
                    key_values: vec![
                        ("14-day mean BP".into(), "138/84 mmHg".into()),
                        ("Ankle edema".into(), "None".into()),
                    ],
                },
                ChartRecord {
                    id: "cm-lab-02".into(),
                    kind: RecordKind::LabResult,
                    title: "Basic Metabolic Panel & Urine ACR".into(),
                    recorded_at: "2026-11-17".into(),
                    source_label: "Regional Reference Lab".into(),
                    body: "Creatinine 1.09 mg/dL (baseline 1.06), eGFR 74 mL/min/1.73m2, potassium \
                           4.3 mEq/L, sodium 140 mEq/L. Urine albumin-to-creatinine ratio 22 mg/g."
                        .into(),
                    key_values: vec![
                        ("eGFR".into(), "74".into()),
                        ("Potassium".into(), "4.3 mEq/L".into()),
                        ("Urine ACR".into(), "22 mg/g".into()),
                    ],
                },
            ],
            follow_ups: vec![FollowUp {
                id: "cm-fu-1".into(),
                summary: "Evaluate increasing amlodipine to 10 mg or adding low-dose thiazide".into(),
                standing: FollowUpStanding::DueToday,
                next_check: "Today 11:00".into(),
            }],
            reply_draft: None,
        },
        PatientChart {
            id: "pt-priya-nair".into(),
            display_name: "Priya Nair".into(),
            mrn: "DM-1105".into(),
            birth_date: "1994-11-03".into(),
            primary_focus: "Metabolic psychiatry & sleep-mood-CGM tracking".into(),
            summary: "PHQ-9 improved from 14 to 7 and GAD-7 is 6 after sleep-anchor stabilization; \
                      home CGM shows reduced late-afternoon glycemic swings."
                .into(),
            urgency: Urgency::Moderate,
            visit: Some(VisitSlot {
                starts_at: "14:15".into(),
                reason: "Mood & glycemic variability follow-up".into(),
                modality: "Video visit".into(),
            }),
            problems: vec![Problem {
                code: "F32.1".into(),
                label: "Major depressive episode, improving".into(),
                status: "Active · responding".into(),
            }],
            medications: vec![Medication {
                name: "Sertraline".into(),
                dose: "50 mg".into(),
                schedule: "Once daily in morning".into(),
                indication: "Mood & anxiety".into(),
            }],
            records: vec![
                ChartRecord {
                    id: "pn-note-01".into(),
                    kind: RecordKind::ProgressNote,
                    title: "Validated Mood & Sleep Cadence Summary".into(),
                    recorded_at: "2026-11-22".into(),
                    source_label: "Local Check-In Export".into(),
                    body: "PHQ-9 declined from 14 (Oct) to 7 (Nov); GAD-7 6; ASRM 3 (below activation \
                           threshold). Sleep duration averaged 7.4 hours with consistent 23:15 lights-out."
                        .into(),
                    key_values: vec![
                        ("PHQ-9".into(), "7 (prior 14)".into()),
                        ("GAD-7".into(), "6".into()),
                        ("ASRM".into(), "3".into()),
                    ],
                },
                ChartRecord {
                    id: "pn-cgm-02".into(),
                    kind: RecordKind::CgmSummary,
                    title: "14-Day Glycemic Stability & Mood Correlation".into(),
                    recorded_at: "2026-11-22".into(),
                    source_label: "Home CGM Export".into(),
                    body: "Mean glucose 104 mg/dL, CV 19.2%, TIR (70-180 mg/dL) 96%. Post-lunch spikes \
                           above 155 mg/dL dropped from 6 days/week to 1 day/week after higher-protein \
                           midday meals; afternoon anxiety dips resolved in parallel."
                        .into(),
                    key_values: vec![
                        ("Time in range".into(), "96%".into()),
                        ("CV".into(), "19.2%".into()),
                        ("Mean glucose".into(), "104 mg/dL".into()),
                    ],
                },
            ],
            follow_ups: vec![FollowUp {
                id: "pn-fu-1".into(),
                summary: "Continue current sertraline 50 mg and recheck PHQ-9/GAD-7 in 4 weeks".into(),
                standing: FollowUpStanding::Monitoring,
                next_check: "2026-12-20".into(),
            }],
            reply_draft: None,
        },
        PatientChart {
            id: "pt-david-okafor".into(),
            display_name: "David Okafor".into(),
            mrn: "DM-1138".into(),
            birth_date: "1963-02-19".into(),
            primary_focus: "Paroxysmal atrial fibrillation & anticoagulation".into(),
            summary: "Rate-controlled paroxysmal AF on apixaban 5 mg BID; normal CBC and renal \
                      function; recent transthoracic echocardiogram shows preserved LVEF 60%."
                .into(),
            urgency: Urgency::Routine,
            visit: None,
            problems: vec![Problem {
                code: "I48.0".into(),
                label: "Paroxysmal atrial fibrillation".into(),
                status: "Active · rate controlled".into(),
            }],
            medications: vec![
                Medication {
                    name: "Apixaban".into(),
                    dose: "5 mg".into(),
                    schedule: "Twice daily".into(),
                    indication: "Stroke prophylaxis".into(),
                },
                Medication {
                    name: "Metoprolol succinate".into(),
                    dose: "25 mg".into(),
                    schedule: "Once daily".into(),
                    indication: "Rate control".into(),
                },
            ],
            records: vec![
                ChartRecord {
                    id: "do-img-01".into(),
                    kind: RecordKind::Imaging,
                    title: "Transthoracic Echocardiogram".into(),
                    recorded_at: "2026-11-12".into(),
                    source_label: "Cardiology Imaging".into(),
                    body: "Left ventricular ejection fraction 60% with normal wall motion. Mild left \
                           atrial enlargement (LAVI 36 mL/m2). No hemodynamically significant valvular \
                           stenosis or regurgitation."
                        .into(),
                    key_values: vec![
                        ("LVEF".into(), "60%".into()),
                        ("LAVI".into(), "36 mL/m2".into()),
                    ],
                },
                ChartRecord {
                    id: "do-lab-02".into(),
                    kind: RecordKind::LabResult,
                    title: "CBC & Renal Panel".into(),
                    recorded_at: "2026-11-14".into(),
                    source_label: "Regional Reference Lab".into(),
                    body: "Hemoglobin 14.2 g/dL, platelets 228 K/uL, creatinine 0.94 mg/dL, \
                           eGFR 86 mL/min/1.73m2. No bleeding signs; standard apixaban dosing remains appropriate."
                        .into(),
                    key_values: vec![
                        ("Hemoglobin".into(), "14.2 g/dL".into()),
                        ("eGFR".into(), "86".into()),
                    ],
                },
            ],
            follow_ups: vec![FollowUp {
                id: "do-fu-1".into(),
                summary: "Routine 6-month anticoagulation and renal check".into(),
                standing: FollowUpStanding::Completed,
                next_check: "2027-05-15".into(),
            }],
            reply_draft: None,
        },
    ]
}

/// Returns the compact clinic-day overview for `GET /today`.
pub fn today_overview() -> TodayOverview {
    let patients = synthetic_panel()
        .into_iter()
        .map(|p| PatientSummary {
            id: p.id,
            display_name: p.display_name,
            mrn: p.mrn,
            birth_date: p.birth_date,
            primary_focus: p.primary_focus,
            urgency: p.urgency,
            visit_time: p.visit.map(|v| v.starts_at),
            record_count: p.records.len(),
            open_follow_ups: p
                .follow_ups
                .iter()
                .filter(|f| f.standing != FollowUpStanding::Completed)
                .count(),
        })
        .collect();

    TodayOverview {
        practice_name: "Dabney.moe Synthetic Clinic".into(),
        clinic_date: "2026-11-25".into(),
        synthetic_notice:
            "All charts, names, MRNs, and lab values are fictional synthetic examples.".into(),
        patients,
    }
}

/// Finds a single synthetic patient chart by exact ID.
pub fn find_patient(id: &str) -> Option<PatientChart> {
    synthetic_panel().into_iter().find(|p| p.id == id)
}

/// Most records a single answer cites.
const MAX_CITATIONS: usize = 3;

/// How much one query term counts, by the part of a record it appears in.
/// Titles, structured key names/values and the record kind say what a record
/// is *about*; body text and source labels only *mention* things, and a
/// patient's own message mentioning a lab name counts least of all.
mod weight {
    pub const TITLE: u32 = 4;
    pub const KEY_VALUE: u32 = 4;
    pub const KIND: u32 = 3;
    pub const CLINICAL_BODY: u32 = 2;
    pub const MESSAGE_BODY: u32 = 1;
    pub const SOURCE_LABEL: u32 = 1;
}

/// Words that carry no clinical meaning in a question. They never select a
/// record on their own.
const STOP_WORDS: &[&str] = &[
    "a",
    "about",
    "after",
    "all",
    "an",
    "and",
    "any",
    "are",
    "as",
    "at",
    "be",
    "been",
    "before",
    "by",
    "can",
    "could",
    "did",
    "do",
    "does",
    "for",
    "from",
    "give",
    "had",
    "has",
    "have",
    "he",
    "her",
    "hers",
    "him",
    "his",
    "how",
    "i",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "me",
    "my",
    "of",
    "on",
    "or",
    "our",
    "please",
    "patient",
    "patients",
    "recent",
    "review",
    "she",
    "should",
    "show",
    "summarize",
    "tell",
    "that",
    "the",
    "their",
    "them",
    "there",
    "they",
    "this",
    "to",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "who",
    "why",
    "will",
    "with",
    "would",
    "you",
    "your",
];

impl RecordKind {
    /// Words a physician uses to ask for this kind of record.
    const fn query_words(self) -> &'static [&'static str] {
        match self {
            Self::LabResult => &["lab", "labs", "panel", "result", "results", "bloodwork"],
            Self::ProgressNote => &["note", "notes", "progress", "visit"],
            Self::CgmSummary => &["cgm", "sensor", "glucose"],
            Self::PatientMessage => &["message", "messages", "ask", "asked", "portal", "wrote"],
            Self::Imaging => &["imaging", "image", "scan", "echo", "echocardiogram"],
        }
    }

    /// Body text weight: clinical documentation outranks a patient's own words.
    const fn body_weight(self) -> u32 {
        match self {
            Self::PatientMessage => weight::MESSAGE_BODY,
            Self::LabResult | Self::ProgressNote | Self::CgmSummary | Self::Imaging => {
                weight::CLINICAL_BODY
            }
        }
    }
}

/// Splits text into lowercase whole words. Hyphens, slashes and punctuation
/// separate words, so `PHQ-9` yields `phq` and `9`, and `is` never matches
/// inside `this`.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// Whether a query term names the same word as `word`, allowing a plural.
fn same_word(term: &str, word: &str) -> bool {
    word == term
        || word.strip_suffix('s') == Some(term)
        || term.strip_suffix('s') == Some(word)
        || word.strip_suffix("es") == Some(term)
}

fn mentions(text: &str, term: &str) -> bool {
    words(text).any(|w| same_word(term, &w))
}

/// The meaningful search terms of a question: whole words, minus stop words
/// and bare numbers, without duplicates.
fn query_terms(question: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words(question) {
        let meaningful = word.len() >= 2
            && !word.chars().all(|c| c.is_ascii_digit())
            && !STOP_WORDS.contains(&word.as_str());
        if meaningful && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// Weight of the strongest place `term` appears in `rec`, or 0 when absent.
fn term_weight(rec: &ChartRecord, term: &str) -> u32 {
    let in_kind = rec.kind.query_words().iter().any(|w| same_word(term, w))
        || mentions(rec.kind.label(), term);
    [
        (mentions(&rec.title, term), weight::TITLE),
        (
            rec.key_values
                .iter()
                .any(|(k, v)| mentions(k, term) || mentions(v, term)),
            weight::KEY_VALUE,
        ),
        (in_kind, weight::KIND),
        (mentions(&rec.body, term), rec.kind.body_weight()),
        (mentions(&rec.source_label, term), weight::SOURCE_LABEL),
    ]
    .into_iter()
    .filter_map(|(hit, w)| hit.then_some(w))
    .max()
    .unwrap_or(0)
}

/// Splits a record body into sentences at `.`, `?` or `!` followed by
/// whitespace, so decimals such as `6.7%` stay inside their sentence.
fn sentences(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let at_boundary = matches!(c, '.' | '?' | '!')
            && chars.peek().is_none_or(|(_, next)| next.is_whitespace());
        if at_boundary {
            let end = i + c.len_utf8();
            let sentence = body[start..end].trim();
            if !sentence.is_empty() {
                out.push(sentence);
            }
            start = end;
        }
    }
    let tail = body[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// The parts of a matched record that answer the question: body sentences
/// and key values naming a query term, falling back to the opening sentence
/// when the record matched only by its title or kind.
fn relevant_excerpt(rec: &ChartRecord, terms: &[String]) -> String {
    let hits = |text: &str| terms.iter().any(|t| mentions(text, t));
    let body_sentences = sentences(&rec.body);
    let mut parts: Vec<String> = body_sentences
        .iter()
        .filter(|s| hits(s))
        .map(|s| (*s).to_string())
        .collect();
    if parts.is_empty() {
        parts.extend(body_sentences.first().map(|s| (*s).to_string()));
    }
    let key_values: Vec<String> = rec
        .key_values
        .iter()
        .filter(|(k, v)| hits(k) || hits(v))
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    if !key_values.is_empty() {
        parts.push(format!("({})", key_values.join("; ")));
    }
    parts.join(" ")
}

/// Answers a physician question strictly from the selected patient's own records,
/// returning verbatim excerpts and record citations without crossing patient boundaries.
///
/// Records are matched on whole words, weighted by where the word appears
/// (see [`weight`]). When no record matches, the answer says so and cites
/// nothing rather than presenting unrelated records as evidence.
pub fn answer_chart_question(patient: &PatientChart, raw_question: &str) -> ChatAnswer {
    let question = raw_question.trim();
    let terms = query_terms(question);

    let mut scored: Vec<(u32, &ChartRecord)> = patient
        .records
        .iter()
        .map(|rec| (terms.iter().map(|t| term_weight(rec, t)).sum(), rec))
        .filter(|(score, _)| *score > 0)
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa)
            .then_with(|| a.kind.body_weight().cmp(&b.kind.body_weight()).reverse())
            .then_with(|| b.recorded_at.cmp(&a.recorded_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    let matched: Vec<&ChartRecord> = scored
        .into_iter()
        .take(MAX_CITATIONS)
        .map(|(_, r)| r)
        .collect();

    if matched.is_empty() {
        return no_match_answer(patient, question, &terms);
    }

    let citations: Vec<ChatCitation> = matched
        .iter()
        .map(|r| ChatCitation {
            record_id: r.id.clone(),
            title: r.title.clone(),
            recorded_at: r.recorded_at.clone(),
            kind: r.kind,
            excerpt: r.body.clone(),
        })
        .collect();

    let findings = matched
        .iter()
        .map(|r| {
            format!(
                "- {} [{}], {}, {}: {}",
                r.title,
                r.id,
                r.kind.label(),
                r.recorded_at,
                relevant_excerpt(r, &terms)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let noun = if matched.len() == 1 {
        "record"
    } else {
        "records"
    };
    let answer = format!(
        "{} matching {noun} in {}'s chart ({}):\n{findings}",
        matched.len(),
        patient.display_name,
        patient.mrn
    );
    let cited_ids = citations
        .iter()
        .map(|c| c.record_id.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    ChatAnswer {
        patient_id: patient.id.clone(),
        question: question.to_string(),
        grounded: true,
        answer,
        citations,
        basis: format!(
            "Local deterministic whole-word chart retrieval scoped exclusively to {} (cited: {cited_ids}).",
            patient.mrn
        ),
    }
}

fn no_match_answer(patient: &PatientChart, question: &str, terms: &[String]) -> ChatAnswer {
    let reason = if terms.is_empty() {
        "The question has no searchable clinical terms.".to_string()
    } else {
        format!(
            "None of the {} source records mention: {}.",
            patient.records.len(),
            terms.join(", ")
        )
    };
    ChatAnswer {
        patient_id: patient.id.clone(),
        question: question.to_string(),
        grounded: false,
        answer: format!(
            "No matching records in this chart for {} ({}). {reason} \
             Nothing is cited, and absence from these records is not evidence either way. \
             Try asking about a lab value, symptom, medication or message that appears in the records below.",
            patient.display_name, patient.mrn
        ),
        citations: Vec::new(),
        basis: format!(
            "Local deterministic whole-word chart retrieval scoped exclusively to {}; no record matched.",
            patient.mrn
        ),
    }
}

#[cfg(feature = "ssr")]
pub mod api {
    //! Embedded Axum HTTP API for `/clinical/api/*`.

    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::{header, HeaderValue, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use tower_http::limit::RequestBodyLimitLayer;

    use crate::{
        answer_chart_question, find_patient, synthetic_panel, today_overview, ChatRequest,
        DemoSessionResponse, HealthResponse,
    };

    const CHAT_BODY_LIMIT: usize = 64 * 1024;

    #[derive(Default)]
    struct ApiState {
        session_counter: AtomicU64,
    }

    /// Builds the clean-room clinical workspace API router mounted at `/clinical/api`.
    pub fn router() -> Router {
        let state = Arc::new(ApiState::default());
        Router::new()
            .route("/health", get(health))
            .route("/auth/demo-session", post(demo_session))
            .route("/today", get(today))
            .route("/patients", get(patients))
            .route("/patients/{id}", get(patient_detail))
            .route("/patients/{id}/chat", post(patient_chat))
            .layer(RequestBodyLimitLayer::new(CHAT_BODY_LIMIT))
            .with_state(state)
    }

    fn json_no_store<T: serde::Serialize>(status: StatusCode, body: &T) -> Response {
        let mut resp = (status, Json(body)).into_response();
        resp.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store, max-age=0"),
        );
        resp
    }

    async fn health() -> Response {
        let payload = HealthResponse {
            status: "ok".into(),
            service: "dabney-clinical-workspace".into(),
            synthetic_data: true,
            patient_count: synthetic_panel().len(),
        };
        json_no_store(StatusCode::OK, &payload)
    }

    async fn demo_session(State(state): State<Arc<ApiState>>) -> Response {
        let seq = state.session_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let payload = DemoSessionResponse {
            session_id: format!("demo-session-{seq}"),
            clinician_label: "Demo Clinician".into(),
            mode: "embedded_synthetic".into(),
            synthetic_data: true,
        };
        json_no_store(StatusCode::OK, &payload)
    }

    async fn today() -> Response {
        json_no_store(StatusCode::OK, &today_overview())
    }

    async fn patients() -> Response {
        json_no_store(StatusCode::OK, &today_overview().patients)
    }

    async fn patient_detail(Path(id): Path<String>) -> Response {
        match find_patient(&id) {
            Some(chart) => json_no_store(StatusCode::OK, &chart),
            None => (
                StatusCode::NOT_FOUND,
                [(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("private, no-store, max-age=0"),
                )],
                Json(serde_json::json!({ "error": "patient not found" })),
            )
                .into_response(),
        }
    }

    async fn patient_chat(Path(id): Path<String>, Json(req): Json<ChatRequest>) -> Response {
        if req.question.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                [(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("private, no-store, max-age=0"),
                )],
                Json(serde_json::json!({ "error": "question must not be empty" })),
            )
                .into_response();
        }
        match find_patient(&id) {
            Some(chart) => {
                let answer = answer_chart_question(&chart, &req.question);
                json_no_store(StatusCode::OK, &answer)
            }
            None => (
                StatusCode::NOT_FOUND,
                [(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("private, no-store, max-age=0"),
                )],
                Json(serde_json::json!({ "error": "patient not found" })),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_has_unique_ids_and_mrns() {
        let panel = synthetic_panel();
        assert!(panel.len() >= 4);
        let mut ids = std::collections::HashSet::new();
        let mut mrns = std::collections::HashSet::new();
        for patient in &panel {
            assert!(
                ids.insert(patient.id.clone()),
                "duplicate id {}",
                patient.id
            );
            assert!(
                mrns.insert(patient.mrn.clone()),
                "duplicate mrn {}",
                patient.mrn
            );
            assert!(!patient.records.is_empty());
        }
    }

    #[test]
    fn chat_citations_stay_strictly_within_selected_patient() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let answer = answer_chart_question(&nadia, "What is her latest HbA1c and CGM TIR?");
        assert_eq!(answer.patient_id, "pt-nadia-kulkarni");
        assert!(!answer.citations.is_empty());
        for cite in &answer.citations {
            assert!(
                nadia.records.iter().any(|r| r.id == cite.record_id),
                "citation {} escaped patient chart",
                cite.record_id
            );
        }
        assert!(answer.answer.contains("6.7%"));
        assert!(answer.grounded);
    }

    fn assert_no_match(patient_id: &str, question: &str) {
        let chart = find_patient(patient_id).expect("patient");
        let answer = answer_chart_question(&chart, question);
        assert!(
            answer.citations.is_empty(),
            "{question:?} on {patient_id} cited {:?}",
            answer
                .citations
                .iter()
                .map(|c| &c.record_id)
                .collect::<Vec<_>>()
        );
        assert!(!answer.grounded, "{question:?} must not claim grounding");
        assert!(
            answer
                .answer
                .starts_with("No matching records in this chart"),
            "{question:?}: {}",
            answer.answer
        );
        assert!(answer.basis.contains("no record matched"));
    }

    #[test]
    fn gibberish_question_cites_nothing() {
        assert_no_match("pt-priya-nair", "zzzz");
    }

    #[test]
    fn absent_statin_is_reported_honestly() {
        assert_no_match("pt-carlos-mendez", "Is he on a statin?");
    }

    #[test]
    fn absent_stroke_history_is_reported_honestly() {
        assert_no_match("pt-david-okafor", "Any history of stroke?");
    }

    #[test]
    fn stop_word_only_question_cites_nothing() {
        assert_no_match("pt-nadia-kulkarni", "What is this?");
    }

    #[test]
    fn terms_match_whole_words_not_substrings() {
        // "age" appears only inside "average" and "dosage"-like words.
        assert_no_match("pt-carlos-mendez", "age");
        assert!(!mentions("Basic Metabolic Panel & Urine ACR", "is"));
        assert!(mentions("Potassium 4.3 mEq/L", "potassium"));
        assert!(mentions("Mild left atrial enlargement", "enlargements"));
    }

    #[test]
    fn potassium_question_ranks_the_lab_first() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let answer = answer_chart_question(&nadia, "What is her potassium?");
        assert!(answer.grounded);
        assert_eq!(answer.citations[0].record_id, "nk-lab-02");
        assert!(answer.answer.contains("potassium 4.1 mEq/L"));
    }

    #[test]
    fn clinical_record_outranks_a_message_mentioning_the_same_word() {
        let mut chart = find_patient("pt-carlos-mendez").expect("carlos");
        chart.records.push(ChartRecord {
            id: "cm-msg-99".into(),
            kind: RecordKind::PatientMessage,
            title: "Diet question".into(),
            recorded_at: "2026-11-24".into(),
            source_label: "Patient Portal".into(),
            body: "Should I eat bananas for potassium before my visit?".into(),
            key_values: vec![],
        });
        let answer = answer_chart_question(&chart, "What is his potassium?");
        let ids: Vec<_> = answer
            .citations
            .iter()
            .map(|c| c.record_id.as_str())
            .collect();
        assert_eq!(ids, ["cm-lab-02", "cm-msg-99"]);
    }

    #[test]
    fn answer_body_is_built_from_the_matched_excerpts() {
        let carlos = find_patient("pt-carlos-mendez").expect("carlos");
        let edema = answer_chart_question(&carlos, "Any ankle edema on amlodipine?");
        let labs = answer_chart_question(&carlos, "Review eGFR, potassium, and urine ACR labs");
        assert_eq!(edema.citations[0].record_id, "cm-note-01");
        assert_eq!(labs.citations[0].record_id, "cm-lab-02");
        assert!(edema.answer.contains("ankle edema"));
        assert!(!edema.answer.contains("Urine albumin"));
        assert!(labs.answer.contains("eGFR 74"));
        assert_ne!(edema.answer, labs.answer);
        // The fixed summary + medication template is gone.
        assert!(!edema.answer.contains(&carlos.summary));
        assert!(!edema.answer.contains("Active medications"));
    }

    #[test]
    fn suggested_questions_stay_grounded_and_patient_scoped() {
        let cases = [
            (
                "pt-nadia-kulkarni",
                "What does the 14-day CGM show about morning fasting glucose?",
                "nk-cgm-01",
            ),
            (
                "pt-nadia-kulkarni",
                "Summarize HbA1c, eGFR, and metformin regimen",
                "nk-lab-02",
            ),
            (
                "pt-nadia-kulkarni",
                "What did the patient ask about metformin timing?",
                "nk-msg-03",
            ),
            (
                "pt-carlos-mendez",
                "What is the 14-day home blood pressure average?",
                "cm-note-01",
            ),
            (
                "pt-priya-nair",
                "How did PHQ-9, GAD-7, and ASRM scores change?",
                "pn-note-01",
            ),
            (
                "pt-priya-nair",
                "Summarize CGM time in range and post-lunch spikes",
                "pn-cgm-02",
            ),
            (
                "pt-david-okafor",
                "What did the transthoracic echocardiogram show?",
                "do-img-01",
            ),
            (
                "pt-david-okafor",
                "Review hemoglobin, renal panel, and apixaban dosing",
                "do-lab-02",
            ),
        ];
        for (patient_id, question, expected_first) in cases {
            let chart = find_patient(patient_id).expect("patient");
            let answer = answer_chart_question(&chart, question);
            assert!(answer.grounded, "{question:?}");
            assert_eq!(
                answer.citations[0].record_id, expected_first,
                "{question:?}"
            );
            assert!(answer.citations.len() <= MAX_CITATIONS);
            for cite in &answer.citations {
                assert!(
                    chart.records.iter().any(|r| r.id == cite.record_id),
                    "{question:?}: citation {} escaped {patient_id}",
                    cite.record_id
                );
            }
        }
    }

    #[test]
    fn sentences_keep_decimals_together() {
        assert_eq!(
            sentences("HbA1c 6.7% (prior 7.4%). Fasting glucose 134 mg/dL."),
            ["HbA1c 6.7% (prior 7.4%).", "Fasting glucose 134 mg/dL."]
        );
    }

    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn api_serves_health_today_patients_and_chat() {
        use axum::body::{to_bytes, Body};
        use axum::http::{header, Method, Request, StatusCode};
        use tower::ServiceExt;

        let app = api::router();

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(
            health.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );

        let session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/demo-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);

        let today = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/today")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(today.status(), StatusCode::OK);

        let chat = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/patients/pt-carlos-mendez/chat")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"question":"What is his eGFR and blood pressure?"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(chat.status(), StatusCode::OK);
        let body = to_bytes(chat.into_body(), 64 * 1024).await.unwrap();
        let parsed: ChatAnswer = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.patient_id, "pt-carlos-mendez");
        assert!(parsed.citations.iter().any(|c| c.record_id == "cm-lab-02"));
    }
}
