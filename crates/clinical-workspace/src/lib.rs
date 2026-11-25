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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAnswer {
    pub patient_id: String,
    pub question: String,
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

/// Answers a physician question strictly from the selected patient's own records,
/// returning verbatim excerpts and record citations without crossing patient boundaries.
pub fn answer_chart_question(patient: &PatientChart, raw_question: &str) -> ChatAnswer {
    let question = raw_question.trim();
    let terms = tokenize_query(question);

    let mut scored: Vec<(usize, &ChartRecord)> = patient
        .records
        .iter()
        .map(|rec| {
            let haystack = format!(
                "{} {} {} {}",
                rec.title,
                rec.source_label,
                rec.body,
                rec.key_values
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_ascii_lowercase();
            let score = terms
                .iter()
                .filter(|t| haystack.contains(t.as_str()))
                .count();
            (score, rec)
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.recorded_at.cmp(&a.1.recorded_at))
    });

    let matched: Vec<&ChartRecord> = if scored.iter().any(|(s, _)| *s > 0) {
        scored
            .into_iter()
            .filter(|(s, _)| *s > 0)
            .take(3)
            .map(|(_, r)| r)
            .collect()
    } else {
        scored.into_iter().take(2).map(|(_, r)| r).collect()
    };

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

    let meds_line = if patient.medications.is_empty() {
        "No active medications documented.".to_string()
    } else {
        patient
            .medications
            .iter()
            .map(|m| format!("{} {} ({})", m.name, m.dose, m.schedule))
            .collect::<Vec<_>>()
            .join("; ")
    };

    let cite_bullets = citations
        .iter()
        .map(|c| {
            format!(
                "- [{}] {} ({}): {}",
                c.record_id, c.title, c.recorded_at, c.excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let answer = format!(
        "Chart findings for {} ({}):\n{}\n\nActive medications: {}\n\nGrounded records:\n{}",
        patient.display_name, patient.mrn, patient.summary, meds_line, cite_bullets
    );

    let cited_ids = citations
        .iter()
        .map(|c| c.record_id.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    ChatAnswer {
        patient_id: patient.id.clone(),
        question: question.to_string(),
        answer,
        citations,
        basis: format!(
            "Local deterministic chart retrieval scoped exclusively to {} (cited: {}).",
            patient.mrn, cited_ids
        ),
    }
}

fn tokenize_query(query: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "the", "and", "for", "with", "what", "how", "are", "was", "were", "has", "have", "does",
        "did", "this", "that", "from", "into", "about", "their", "her", "his", "any", "show",
        "tell", "please", "patient", "my", "our", "your", "why", "can", "should", "when", "been",
    ];
    let mut out = Vec::new();
    for raw in query.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '/') {
        let lower = raw.to_ascii_lowercase();
        if lower.len() >= 2 && !STOP_WORDS.contains(&lower.as_str()) && !out.contains(&lower) {
            out.push(lower);
        }
    }
    out
}

/// Simulated practice-connection state for the zero-PHI patient demo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DemoConnectionState {
    Connected {
        practice_name: String,
        clinician_name: String,
        next_visit: String,
    },
    Paused {
        practice_name: String,
    },
    Standalone,
}

impl DemoConnectionState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Connected { .. } => "Connected (demo clinic)",
            Self::Paused { .. } => "Sharing paused",
            Self::Standalone => "Standalone (no practice)",
        }
    }

    pub const fn allows_sharing(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn practice_label(&self) -> String {
        match self {
            Self::Connected {
                practice_name,
                clinician_name,
                ..
            } => format!("{practice_name} · {clinician_name}"),
            Self::Paused { practice_name } => format!("{practice_name} (sharing paused)"),
            Self::Standalone => "No practice connected (standalone demo)".into(),
        }
    }
}

/// Private note kept exclusively on-device in the patient demo (never included
/// in doctor sharing packets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientVisitNote {
    pub id: String,
    pub body: String,
    pub created_label: String,
}

/// Local-only delivery status for the simulated "For My Doctor" share packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ShareDeliveryStatus {
    Draft,
    SimulatedLocalSend {
        sent_record_count: usize,
        question_count: usize,
    },
    Withdrawn,
}

impl ShareDeliveryStatus {
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::Draft => "Draft · not shared",
            Self::SimulatedLocalSend { .. } => "Simulated send (local only)",
            Self::Withdrawn => "Withdrawn locally",
        }
    }
}

/// Complete interactive state for a single synthetic patient's demo session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientDemoState {
    pub patient_id: String,
    pub connection: DemoConnectionState,
    pub selected_record_ids: Vec<String>,
    pub visit_questions: Vec<String>,
    pub visit_note: String,
    pub personal_notes: Vec<PatientVisitNote>,
    pub delivery_status: ShareDeliveryStatus,
}

/// Deterministic preview of the exact visit summary packet that would be shared
/// with a connected clinic—strictly limited to explicitly selected records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePacketPreview {
    pub patient_id: String,
    pub patient_display_name: String,
    pub patient_mrn: String,
    pub practice_label: String,
    pub sharing_enabled: bool,
    pub shared_records: Vec<ChartRecord>,
    pub excluded_record_count: usize,
    pub private_note_count: usize,
    pub visit_note: String,
    pub visit_questions: Vec<String>,
    pub synthetic_disclaimer: String,
}

/// Combined payload returned by `GET /clinical/api/patient-demo/{id}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientDemoSnapshot {
    pub patient: PatientChart,
    pub state: PatientDemoState,
    pub preview: SharePacketPreview,
}

/// Patient-friendly sample questions scoped to a synthetic patient profile.
pub fn default_patient_questions(patient_id: &str) -> &'static [&'static str] {
    match patient_id {
        "pt-nadia-kulkarni" => &[
            "Why do my morning glucose readings climb before breakfast?",
            "What was my latest HbA1c and kidney lab result?",
            "What did I ask Dr. Patel about metformin timing?",
        ],
        "pt-carlos-mendez" => &[
            "What was my 14-day home blood pressure average?",
            "How do my potassium and kidney labs look?",
            "Did my home log note any ankle swelling or side effects?",
        ],
        "pt-priya-nair" => &[
            "How did my PHQ-9 and GAD-7 scores change recently?",
            "How did my CGM time in range and post-lunch glucose look?",
        ],
        "pt-david-okafor" => &[
            "What did my echocardiogram report say about ejection fraction?",
            "Are my hemoglobin and renal labs normal for apixaban?",
        ],
        _ => &["Summarize my recent synthetic records"],
    }
}

/// Builds the default fictional patient demo state for a synthetic chart.
///
/// Pre-selects a subset of records (leaving at least one unselected when a
/// chart has multiple records) and seeds one private device-only note so
/// patients can immediately inspect inclusion vs. exclusion boundaries.
pub fn default_patient_demo_state(patient: &PatientChart) -> PatientDemoState {
    let selected_record_ids: Vec<String> = if patient.records.len() > 2 {
        patient
            .records
            .iter()
            .take(2)
            .map(|r| r.id.clone())
            .collect()
    } else {
        patient
            .records
            .iter()
            .take(1)
            .map(|r| r.id.clone())
            .collect()
    };

    let (clinician_name, visit_note, visit_questions, private_note_body) = match patient.id.as_str()
    {
        "pt-nadia-kulkarni" => (
            "Dr. Maya Patel (Demo)",
            "Overnight glucose stays steady until ~5 AM and rises into the 140s before breakfast. \
             Hoping to review whether keeping metformin ER 1000 mg with dinner is still best."
                .to_string(),
            vec![
                "Should I keep taking metformin ER with dinner or adjust timing?".to_string(),
                "Is my 78% 14-day CGM time-in-range on track for our next check?".to_string(),
            ],
            "Private reminder (device-only demo): Bring list of breakfast options I tested last week."
                .to_string(),
        ),
        "pt-carlos-mendez" => (
            "Dr. Maya Patel (Demo)",
            "Logged 24 seated home blood pressure readings over two weeks averaging 138/84 mmHg \
             with no headaches, dizziness, or ankle swelling."
                .to_string(),
            vec![
                "Does 138/84 mmHg mean we should adjust amlodipine or wait another month?"
                    .to_string(),
                "Are my eGFR (74) and potassium (4.3) stable on lisinopril?".to_string(),
            ],
            "Private reminder (device-only demo): Ask pharmacy about 90-day refill alignment."
                .to_string(),
        ),
        "pt-priya-nair" => (
            "Dr. Hannah Brooks (Demo)",
            "Consistent 23:15 sleep schedule and higher-protein lunches seemed to steady both \
             afternoon glucose swings and afternoon anxiety over the last two weeks."
                .to_string(),
            vec![
                "My PHQ-9 dropped from 14 to 7—what cadence should we use for next month's check-in?"
                    .to_string(),
                "Should I keep tracking post-lunch CGM trends alongside mood scores?".to_string(),
            ],
            "Private reminder (device-only demo): Note which workdays felt most stressful."
                .to_string(),
        ),
        "pt-david-okafor" => (
            "Dr. Samuel Cho (Demo)",
            "Feeling steady with no palpitations, bruising, or bleeding episodes since my last \
             echocardiogram and blood work."
                .to_string(),
            vec![
                "Do my hemoglobin (14.2) and eGFR (86) confirm standard apixaban dosing?"
                    .to_string(),
                "When should I schedule my next routine 6-month lab check?".to_string(),
            ],
            "Private reminder (device-only demo): Confirm travel dates before booking May follow-up."
                .to_string(),
        ),
        _ => (
            "Demo Clinician",
            "Synthetic sample visit note for demonstration purposes.".to_string(),
            vec!["Review my selected synthetic records at the next visit.".to_string()],
            "Private demo note kept strictly on this device.".to_string(),
        ),
    };

    let next_visit = patient
        .visit
        .as_ref()
        .map(|v| format!("Today {} · {}", v.starts_at, v.modality))
        .unwrap_or_else(|| "Asynchronous panel review".to_string());

    PatientDemoState {
        patient_id: patient.id.clone(),
        connection: DemoConnectionState::Connected {
            practice_name: "Dabney.moe Synthetic Clinic".into(),
            clinician_name: clinician_name.into(),
            next_visit,
        },
        selected_record_ids,
        visit_questions,
        visit_note,
        personal_notes: vec![PatientVisitNote {
            id: format!("{}-priv-1", patient.id),
            body: private_note_body,
            created_label: "Sample private note".into(),
        }],
        delivery_status: ShareDeliveryStatus::Draft,
    }
}

/// Projects the exact clinician-visible packet from a synthetic patient's chart
/// and explicit sharing selections.
///
/// Guarantees:
/// - Only records belonging to `patient` AND present in `state.selected_record_ids`
///   appear in `shared_records` (foreign/unknown IDs are ignored).
/// - `personal_notes` are never copied into the packet (`private_note_count` is
///   surfaced only to reassure the patient they remain excluded).
pub fn build_doctor_share_preview(
    patient: &PatientChart,
    state: &PatientDemoState,
) -> SharePacketPreview {
    let shared_records: Vec<ChartRecord> = patient
        .records
        .iter()
        .filter(|rec| state.selected_record_ids.iter().any(|id| id == &rec.id))
        .cloned()
        .collect();
    let excluded_record_count = patient.records.len().saturating_sub(shared_records.len());

    SharePacketPreview {
        patient_id: patient.id.clone(),
        patient_display_name: patient.display_name.clone(),
        patient_mrn: patient.mrn.clone(),
        practice_label: state.connection.practice_label(),
        sharing_enabled: state.connection.allows_sharing(),
        shared_records,
        excluded_record_count,
        private_note_count: state.personal_notes.len(),
        visit_note: state.visit_note.trim().to_string(),
        visit_questions: state
            .visit_questions
            .iter()
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
            .collect(),
        synthetic_disclaimer: "SYNTHETIC DEMO PACKET — ZERO PHI. Fictional sample data only; nothing is transmitted off this device.".into(),
    }
}

/// Answers a patient-demo question strictly from the selected synthetic patient's
/// own records using whole-word matching. Unlike fallback summaries, unmatched
/// queries return zero citations and an explicit no-match notice.
pub fn answer_patient_question(patient: &PatientChart, raw_question: &str) -> ChatAnswer {
    let question = raw_question.trim();
    let terms = tokenize_query(question);

    let mut scored: Vec<(usize, &ChartRecord)> = patient
        .records
        .iter()
        .map(|rec| {
            let haystack = format!(
                "{} {} {} {}",
                rec.title,
                rec.source_label,
                rec.body,
                rec.key_values
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let words: Vec<String> = haystack
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '/')
                .filter(|w| !w.is_empty())
                .map(|w| w.to_ascii_lowercase())
                .collect();
            let score = terms
                .iter()
                .filter(|term| {
                    words.iter().any(|word| {
                        word == term.as_str()
                            || (term.len() >= 4 && word.starts_with(term.as_str()))
                    })
                })
                .count();
            (score, rec)
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.recorded_at.cmp(&a.1.recorded_at))
    });

    let matched: Vec<&ChartRecord> = scored
        .into_iter()
        .filter(|(score, _)| *score > 0)
        .take(3)
        .map(|(_, rec)| rec)
        .collect();

    if matched.is_empty() {
        return ChatAnswer {
            patient_id: patient.id.clone(),
            question: question.to_string(),
            answer: format!(
                "No matching records were found in {}'s synthetic sample chart for that question. \
                 Try asking about labs, CGM trends, medications, or recent notes listed on this profile.",
                patient.display_name
            ),
            citations: Vec::new(),
            basis: format!(
                "Local deterministic retrieval scoped exclusively to {} (0 matching records).",
                patient.mrn
            ),
        };
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

    let findings = citations
        .iter()
        .map(|c| {
            format!(
                "- [{}] {} ({}): {}",
                c.record_id, c.title, c.recorded_at, c.excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let cited_ids = citations
        .iter()
        .map(|c| c.record_id.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    ChatAnswer {
        patient_id: patient.id.clone(),
        question: question.to_string(),
        answer: format!(
            "From {}'s synthetic sample records (demo only, not medical advice):\n{}",
            patient.display_name, findings
        ),
        citations,
        basis: format!(
            "Local deterministic retrieval scoped exclusively to {} (cited: {}).",
            patient.mrn, cited_ids
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
        answer_chart_question, build_doctor_share_preview, default_patient_demo_state,
        find_patient, synthetic_panel, today_overview, ChatRequest, DemoSessionResponse,
        HealthResponse, PatientDemoSnapshot,
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
            .route("/patient-demo/{id}", get(patient_demo_snapshot))
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

    async fn patient_demo_snapshot(Path(id): Path<String>) -> Response {
        match find_patient(&id) {
            Some(patient) => {
                let state = default_patient_demo_state(&patient);
                let preview = build_doctor_share_preview(&patient, &state);
                json_no_store(
                    StatusCode::OK,
                    &PatientDemoSnapshot {
                        patient,
                        state,
                        preview,
                    },
                )
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
    }

    #[test]
    fn patient_demo_share_preview_enforces_selection_and_excludes_private_notes() {
        for patient in synthetic_panel() {
            let mut state = default_patient_demo_state(&patient);
            // Inject a foreign record ID from another patient to prove boundary enforcement.
            state.selected_record_ids.push("foreign-record-99".into());
            let preview = build_doctor_share_preview(&patient, &state);
            assert_eq!(preview.patient_id, patient.id);
            assert!(preview.sharing_enabled);
            assert!(!preview.shared_records.is_empty());
            assert!(preview
                .shared_records
                .iter()
                .all(|rec| patient.records.iter().any(|own| own.id == rec.id)));
            assert_eq!(
                preview.shared_records.len() + preview.excluded_record_count,
                patient.records.len()
            );
            assert_eq!(preview.private_note_count, state.personal_notes.len());
            assert!(preview.synthetic_disclaimer.contains("ZERO PHI"));

            // Pausing sharing disables packet sharing without mutating selections.
            state.connection = DemoConnectionState::Paused {
                practice_name: "Dabney.moe Synthetic Clinic".into(),
            };
            let paused = build_doctor_share_preview(&patient, &state);
            assert!(!paused.sharing_enabled);
        }
    }

    #[test]
    fn patient_question_retrieval_returns_zero_citations_on_unmatched_query() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let matched = answer_patient_question(&nadia, "What was my latest HbA1c?");
        assert!(!matched.citations.is_empty());
        assert!(matched.citations.iter().any(|c| c.record_id == "nk-lab-02"));

        let unmatched = answer_patient_question(&nadia, "zzzz nonexistent statin");
        assert!(unmatched.citations.is_empty());
        assert!(unmatched.answer.contains("No matching records"));
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

        let demo = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/patient-demo/pt-nadia-kulkarni")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(demo.status(), StatusCode::OK);
        assert_eq!(
            demo.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let demo_body = to_bytes(demo.into_body(), 64 * 1024).await.unwrap();
        let snapshot: PatientDemoSnapshot = serde_json::from_slice(&demo_body).unwrap();
        assert_eq!(snapshot.patient.id, "pt-nadia-kulkarni");
        assert!(snapshot.preview.sharing_enabled);
        assert!(!snapshot.preview.shared_records.is_empty());

        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/patient-demo/pt-nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }
}
