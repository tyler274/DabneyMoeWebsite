//! Build-time seeder converting `patients/units/*/ledger.json` fixtures into `demo.sqlite`.

use crate::driver::DbError;
use crate::schema::{
    Briefings, Chats, FollowUps, PatientMemory, PatientReviews, Patients, PracticeVisits,
    SourceRecords, SpruceMatchQueue, TodayItems, UpdatesFeed,
};
use crate::spruce::{StoredMatchItem, StoredSpruceReply};
use crate::store::{
    DEMO_DATE, DEMO_NOW, DEMO_ZONE, DemoSourceRecord, DemoStore, build_cited_source,
    build_exchange_with_segments, build_item_citation, condition_fields, format_json_scalar,
    lab_fields, medication_fields, synthesize_chart_answer_with_citations, vitals_fields,
};
use physician_wire::briefings::{
    BriefingClaimWire, BriefingDecisionWire, BriefingSourceStatusWire, BriefingVisitStandingWire,
    VisitBriefingWire,
};
use physician_wire::follow_ups::{FollowUpOriginWire, FollowUpStandingWire, FollowUpWire};
use physician_wire::matching_reply::{
    CardChannelBannerWire, CardReviewStateWire, ChannelSenderRoleWire, DisplayedReplyTextWire,
    FilteredMessageWire, MatchSuggestionWire, NonPatientCategoryWire, PatientReplyConversationWire,
    ReplyCardWire, ReplyPatientMessageWire, UnmatchedMessageWire, UnmatchedUrgencyWire,
    UnreadableSpruceMessageWire,
};
use physician_wire::reviews::{
    PatientReviewOutcomeWire, PatientReviewStateWire, PatientReviewWire, ReviewActionWire,
    ReviewEvidenceRefWire, ReviewHoldReasonWire, ReviewStepStateWire, ReviewStepWire,
    ReviewTriggerWire, ReviewVersion,
};
use physician_wire::source_setup::{
    PracticeSourceWire, SourceConnectionMethodWire, SourceStandingWire, SpruceBacklogWire,
    SupportedSourceWire,
};
use physician_wire::today_updates::{SeenStandingWire, UpdateItemKindWire, UpdateItemWire};
use physician_wire::{
    AutomationRun, ChatSnapshot, CitationDestination, CitationTargetKind, CitedFieldWire,
    CitedTextPartWire, ClinicalSegment, ConnectedSource, ConnectionStatusWire, FollowUp,
    MedicalRecordNumber, MemoryDocumentWire, MessageSegment, PatientIdentity, PatientLinkWire,
    PatientMemoryResponse, PatientSynopsis, PatientWorkspaceResponse, RecordGroup, SexOrGender,
    SnapshotFeedback, SourceConnectionState, SourcePresentation, SourcePreviewContent,
    SourceSnapshot, WorkPurpose, WorkspaceMedicationRecommendation, WorkspaceMembership,
    WorkspacePlanItem, WorkspaceReply,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const PRIORITY_UNITS: [&str; 3] = ["unit-2013", "unit-2188", "unit-2797"];

/// Errors produced while discovering or reading unit fixtures.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// Underlying SQLite error.
    #[error("demo SQLite error")]
    Db(#[from] DbError),
    /// Filesystem read failure.
    #[error("failed reading unit fixture")]
    Io(#[from] std::io::Error),
    /// JSON parse failure for a unit ledger.
    #[error("failed parsing unit ledger JSON")]
    Json(#[from] serde_json::Error),
}

struct ExtractedRecord {
    id: String,
    group: &'static str,
    title: String,
    recorded_at: String,
    body: String,
    source_label: String,
    readable_parts: Vec<CitedTextPartWire>,
    fields: Vec<CitedFieldWire>,
    record_status: String,
    raw_content: Option<String>,
}

/// Synthetic workspace ID for Maya Lin (longitudinal mental health + Abbott Lingo CGM).
pub const MAYA_LIN_WORKSPACE_ID: &str = "unit-lingo-mh-1";

/// Seeds `store` from `units_dir`, prioritizing pilot units (`unit-2013`, `unit-2188`, `unit-2797`)
/// and synthetic Abbott Lingo CGM + mental health patient Maya Lin (`unit-lingo-mh-1`).
pub fn seed_from_units_dir(
    store: &DemoStore,
    units_dir: &Path,
    limit: usize,
) -> Result<usize, SeedError> {
    store.init_schema()?;
    seed_practice_connections_and_matching(store)?;
    let include_maya = limit == 0 || limit >= 4;
    let unit_limit = if include_maya && limit > 0 {
        limit - 1
    } else {
        limit
    };
    let mut imported = 0usize;
    let unit_dirs = collect_unit_dirs(units_dir, unit_limit)?;
    for (raw_idx, unit_dir) in unit_dirs.iter().enumerate() {
        if include_maya && raw_idx == 3 {
            seed_maya_lin_workspace(store)?;
            imported += 1;
        }
        let slot_idx = if include_maya && raw_idx >= 3 {
            raw_idx + 1
        } else {
            raw_idx
        };
        let unit_id = unit_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unit-unknown");
        let ledger_path = unit_dir.join("ledger.json");
        if !ledger_path.is_file() {
            continue;
        }
        let ledger: Value = serde_json::from_str(&fs::read_to_string(&ledger_path)?)?;
        if let Some(patient) = jarr(&ledger, "patients").first() {
            insert_patient_unit(store, slot_idx, unit_id, patient)?;
            imported += 1;
        }
    }
    if include_maya && unit_dirs.len() <= 3 {
        seed_maya_lin_workspace(store)?;
        imported += 1;
    }
    store
        .driver()
        .exec_ddl(&["PRAGMA page_size = 4096".into(), "VACUUM".into()])?;
    Ok(imported)
}

fn collect_unit_dirs(units_dir: &Path, limit: usize) -> Result<Vec<PathBuf>, SeedError> {
    let mut all: Vec<PathBuf> = fs::read_dir(units_dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    all.sort();
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    for c in PRIORITY_UNITS.iter().map(|p| units_dir.join(p)).chain(all) {
        if limit > 0 && ordered.len() >= limit {
            break;
        }
        if c.is_dir() && seen.insert(c.clone()) {
            ordered.push(c);
        }
    }
    Ok(ordered)
}

fn insert_patient_unit(
    store: &DemoStore,
    idx: usize,
    ws_id: &str,
    patient: &Value,
) -> Result<(), SeedError> {
    let (given, family, mrn, birth_date, summary) = (
        jstr(patient, "given"),
        jstr(patient, "family"),
        jstr(patient, "mrn"),
        jstr(patient, "birth_date"),
        jstr(patient, "summary"),
    );
    let display_name = format!("{given} {family}");
    let records = extract_source_records(ws_id, patient);
    for rec in &records {
        store.driver().insert_row(
            SourceRecords::Table,
            SourceRecords::COLS,
            [
                rec.id.as_str().into(),
                ws_id.into(),
                rec.group.into(),
                rec.title.as_str().into(),
                rec.recorded_at.as_str().into(),
                rec.body.as_str().into(),
                rec.source_label.as_str().into(),
                "".into(),
                "".into(),
                rec.record_status.as_str().into(),
                rec.raw_content.as_deref().unwrap_or("").into(),
            ],
        )?;
    }
    let (headline, replies) = if idx < 6 {
        let (h, r) = seed_featured(store, idx, ws_id, &display_name, given, summary, &records)?;
        (Some(h), r)
    } else {
        if idx < 8 {
            seed_quiet_review_update(store, ws_id, &display_name, summary)?;
        }
        (None, Vec::new())
    };
    let active_follow_ups = if idx < 4 {
        let fu = seed_follow_up_row(store, idx, ws_id)?;
        vec![fu]
    } else {
        Vec::new()
    };
    let sex = infer_sex_label(summary);
    let year = birth_date
        .split('-')
        .next()
        .and_then(|y| y.parse::<i64>().ok())
        .unwrap_or(1970);
    let workspace = PatientWorkspaceResponse {
        patient_workspace_id: ws_id.into(),
        display_name: display_name.clone(),
        membership: WorkspaceMembership::Allowed,
        time_zone: Some(DEMO_ZONE.into()),
        chats: Vec::new(),
        identity: Some(PatientIdentity {
            display_name: Some(display_name.clone()),
            birth_date: Some(birth_date.into()),
            age_years: Some((2026 - year).max(1)),
            sex_or_gender: Some(SexOrGender {
                value: Some(sex.to_lowercase()),
                label: Some(sex.into()),
            }),
            medical_record_numbers: vec![MedicalRecordNumber {
                value: Some(mrn.into()),
            }],
            email: None,
            state: Some("available".into()),
        }),
        patient_synopsis: Some(PatientSynopsis {
            version_id: format!("syn-{ws_id}-v1"),
            authored_text: None,
            segments: build_synopsis(ws_id, summary, &records),
            authored_at: Some(DEMO_NOW.into()),
        }),
        automation_run: headline.map(|h| AutomationRun {
            kind: "visit_preparation".into(),
            state: "finished".into(),
            ended_cause: None,
            purpose: Some(WorkPurpose {
                headline: Some(h.into()),
            }),
        }),
        active_follow_ups,
        has_past_work: idx < 6,
        last_reviewed_at: Some(DEMO_NOW.into()),
        connected_sources: vec![ConnectedSource {
            source_id: Some("synthetic-ehr".into()),
            source_label: Some("Primary EHR".into()),
            state: SourceConnectionState {
                state: "usable".into(),
                synchronized_at: Some(DEMO_NOW.into()),
                usable_snapshot: Some(SourceSnapshot {
                    synchronized_at: Some(DEMO_NOW.into()),
                }),
            },
        }],
        replies,
        superseded_replies: Vec::new(),
        active_plan_items: build_initial_plan(ws_id, idx),
        undecided_recommendations: build_recommendations(ws_id, idx),
        connection_status: if idx < 12 {
            ConnectionStatusWire::Connected
        } else {
            ConnectionStatusWire::NotInvited
        },
        patient_link: Some(PatientLinkWire {
            practice_id: "prac-coda-demo".into(),
            patient_workspace_id: ws_id.into(),
            duet_account_id: Some(format!("duet-{ws_id}")),
            linked: idx < 12,
            sharing_consented: idx < 10,
            connection_status: if idx < 12 {
                ConnectionStatusWire::Connected
            } else {
                ConnectionStatusWire::NotInvited
            },
            consent_version: Some("v1".into()),
            consented_at: Some(1_785_000_000),
            revoked_at: None,
            invited_email: None,
            wrong_dob_attempts: 0,
            locked_at: None,
            delivery_failed_at: None,
            delivery_error: None,
        }),
    };
    let json = serde_json::to_string(&workspace)?;
    store.driver().insert_row(
        Patients::Table,
        Patients::COLS,
        [
            ws_id.into(),
            display_name.into(),
            mrn.into(),
            birth_date.into(),
            json.into(),
        ],
    )?;
    Ok(())
}

fn jarr<'a>(val: &'a Value, key: &str) -> &'a [Value] {
    val.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn jstr<'a>(val: &'a Value, key: &str) -> &'a str {
    val.get(key).and_then(Value::as_str).unwrap_or("")
}
fn jnonempty<'a>(val: &'a Value, key: &str, fb: &'a str) -> &'a str {
    match jstr(val, key).trim() {
        "" => fb,
        s => s,
    }
}
fn date_prefix(raw: Option<&str>, fb: &str) -> String {
    raw.and_then(|s| s.get(..10))
        .filter(|s| !s.is_empty())
        .unwrap_or(fb)
        .into()
}

fn compact_raw(val: &Value) -> Option<String> {
    serde_json::to_string(val).ok()
}

fn compact_without_prose(val: &Value, omit_keys: &[&str]) -> Option<String> {
    let Some(obj) = val.as_object() else {
        return compact_raw(val);
    };
    let mut trimmed = serde_json::Map::with_capacity(obj.len());
    for (k, v) in obj {
        if !omit_keys.contains(&k.as_str()) && !v.is_null() {
            trimmed.insert(k.clone(), v.clone());
        }
    }
    serde_json::to_string(&Value::Object(trimmed)).ok()
}

fn split_paragraph_parts(first_label: Option<String>, body: &str) -> Vec<CitedTextPartWire> {
    let mut parts = Vec::new();
    for para in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let label = if parts.is_empty() {
            first_label.clone()
        } else {
            None
        };
        parts.push(CitedTextPartWire {
            label,
            text: para.to_owned(),
        });
    }
    if parts.is_empty() && !body.trim().is_empty() {
        parts.push(CitedTextPartWire {
            label: first_label,
            text: body.trim().to_owned(),
        });
    }
    parts
}

fn extract_document_record(
    ws_id: &str,
    ei: usize,
    di: usize,
    dt: &str,
    d: &Value,
) -> Option<ExtractedRecord> {
    let body = jstr(d, "body").trim();
    if body.is_empty() {
        return None;
    }
    let kind = jnonempty(d, "type", "Clinical Note");
    let title = jnonempty(d, "title", kind);
    let grp = if kind.to_lowercase().contains("outside") {
        "documents"
    } else {
        "notes"
    };
    let source_label = if grp == "documents" {
        "Outside Records"
    } else {
        "Practice EHR"
    };
    let author = jstr(d, "author").trim();
    let header = match (!kind.eq_ignore_ascii_case(title), !author.is_empty()) {
        (true, true) => Some(format!("{kind} · {author}")),
        (true, false) => Some(kind.to_owned()),
        (false, true) => Some(author.to_owned()),
        (false, false) => None,
    };
    Some(ExtractedRecord {
        id: format!("{ws_id}-doc-{ei}-{di}"),
        group: grp,
        title: title.into(),
        recorded_at: date_prefix(d.get("date").and_then(Value::as_str), dt),
        body: body.into(),
        source_label: source_label.into(),
        readable_parts: split_paragraph_parts(header, body),
        fields: Vec::new(),
        record_status: "final".into(),
        raw_content: compact_without_prose(d, &["body"]),
    })
}

fn extract_condition_record(
    ws_id: &str,
    ei: usize,
    dt: &str,
    reason: &str,
    conds: &[Value],
) -> Option<ExtractedRecord> {
    if conds.is_empty() {
        return None;
    }
    let id = format!("{ws_id}-prob-{ei}");
    let body = conds
        .iter()
        .map(|c| {
            let label = jstr(c, "label");
            let action = jnonempty(c, "action", "recorded");
            let note = jstr(c, "note").trim();
            if note.is_empty() {
                format!("{label} ({action})")
            } else {
                format!("{label} ({action}) — {note}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let onset = conds
        .first()
        .and_then(|c| c.get("onset").and_then(Value::as_str))
        .unwrap_or(dt);
    let fields = condition_fields(&id, conds);
    Some(ExtractedRecord {
        id,
        group: "problems",
        title: format!("Diagnoses — {reason}"),
        recorded_at: onset.into(),
        readable_parts: vec![CitedTextPartWire {
            label: Some("Diagnoses".into()),
            text: body.clone(),
        }],
        body,
        source_label: "Practice EHR".into(),
        fields,
        record_status: "final".into(),
        raw_content: compact_raw(&json!({ "conditions": conds })),
    })
}

fn extract_medication_record(
    ws_id: &str,
    ei: usize,
    dt: &str,
    ev_id: &str,
    meds: &[Value],
) -> Option<ExtractedRecord> {
    if meds.is_empty() {
        return None;
    }
    let id = format!("{ws_id}-med-{ei}");
    let body = meds
        .iter()
        .map(|m| {
            let drug = jnonempty(m, "drug", jstr(m, "drug_text"));
            let action = jnonempty(m, "action", "active");
            let sig = jstr(m, "sig").trim();
            let med_reason = jstr(m, "reason").trim();
            if med_reason.is_empty() {
                format!("{drug} [{action}]: {sig}")
            } else {
                format!("{drug} [{action}]: {sig} ({med_reason})")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fields = medication_fields(&id, meds);
    Some(ExtractedRecord {
        id,
        group: "medications",
        title: format!("Medications ({ev_id})"),
        recorded_at: dt.into(),
        readable_parts: vec![CitedTextPartWire {
            label: Some("Medication list".into()),
            text: body.clone(),
        }],
        body,
        source_label: "Practice EHR".into(),
        fields,
        record_status: "final".into(),
        raw_content: compact_raw(&json!({ "medications": meds })),
    })
}

fn extract_lab_record(
    ws_id: &str,
    ei: usize,
    li: usize,
    dt: &str,
    lab: &Value,
) -> Option<ExtractedRecord> {
    let results = jarr(lab, "results");
    if results.is_empty() {
        return None;
    }
    let id = format!("{ws_id}-lab-{ei}-{li}");
    let fields = lab_fields(&id, results);
    let body = fields
        .iter()
        .map(|f| match &f.unit {
            Some(u) => format!("{}: {} {u}", f.name, f.value),
            None => format!("{}: {}", f.name, f.value),
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(ExtractedRecord {
        id,
        group: "results",
        title: jnonempty(lab, "panel", "Laboratory results").into(),
        recorded_at: date_prefix(lab.get("collected").and_then(Value::as_str), dt),
        readable_parts: vec![CitedTextPartWire {
            label: Some("Results".into()),
            text: body.clone(),
        }],
        body,
        source_label: "Practice EHR".into(),
        fields,
        record_status: "final".into(),
        raw_content: compact_raw(lab),
    })
}

fn extract_report_record(
    ws_id: &str,
    ei: usize,
    ri: usize,
    dt: &str,
    r: &Value,
) -> Option<ExtractedRecord> {
    let (text, conc) = (jstr(r, "text").trim(), jstr(r, "conclusion").trim());
    if text.is_empty() && conc.is_empty() {
        return None;
    }
    let mut readable_parts = Vec::new();
    let body = if conc.is_empty() {
        readable_parts.push(CitedTextPartWire {
            label: Some("Findings".into()),
            text: text.into(),
        });
        text.into()
    } else if text.is_empty() {
        readable_parts.push(CitedTextPartWire {
            label: Some("Conclusion".into()),
            text: conc.into(),
        });
        format!("Conclusion: {conc}")
    } else {
        readable_parts.push(CitedTextPartWire {
            label: Some("Findings".into()),
            text: text.into(),
        });
        readable_parts.push(CitedTextPartWire {
            label: Some("Conclusion".into()),
            text: conc.into(),
        });
        format!("{text}\n\nConclusion: {conc}")
    };
    Some(ExtractedRecord {
        id: format!("{ws_id}-rep-{ei}-{ri}"),
        group: "results",
        title: jnonempty(r, "study", "Diagnostic report").into(),
        recorded_at: date_prefix(r.get("performed").and_then(Value::as_str), dt),
        body,
        source_label: "Practice EHR".into(),
        readable_parts,
        fields: Vec::new(),
        record_status: "final".into(),
        raw_content: compact_without_prose(r, &["text", "conclusion"]),
    })
}

fn extract_vitals_record(
    ws_id: &str,
    ei: usize,
    dt: &str,
    reason: &str,
    v: &Value,
) -> Option<ExtractedRecord> {
    let bp = v.get("bp").and_then(Value::as_array)?;
    if bp.len() < 2 {
        return None;
    }
    let id = format!("{ws_id}-vit-{ei}");
    let fields = vitals_fields(&id, v);
    let mut summary_parts = vec![format!("BP {}/{} mmHg", bp[0], bp[1])];
    if let Some(hr) = v.get("hr").and_then(Value::as_i64) {
        summary_parts.push(format!("HR {hr} bpm"));
    }
    for (json_key, label, unit) in [
        ("temp_c", "Temperature", "°C"),
        ("spo2", "Oxygen saturation", "%"),
        ("rr", "Respiratory rate", "/min"),
        ("weight_kg", "Weight", "kg"),
        ("height_cm", "Height", "cm"),
    ] {
        if let Some((val, _)) = format_json_scalar(v.get(json_key)) {
            summary_parts.push(format!("{label} {val} {unit}"));
        }
    }
    let body = summary_parts.join(", ");
    Some(ExtractedRecord {
        id,
        group: "vitals",
        title: format!("Vitals ({reason})"),
        recorded_at: dt.into(),
        readable_parts: vec![CitedTextPartWire {
            label: Some("Vital signs".into()),
            text: body.clone(),
        }],
        body,
        source_label: "Practice EHR".into(),
        fields,
        record_status: "final".into(),
        raw_content: compact_raw(v),
    })
}

fn extract_source_records(ws_id: &str, patient: &Value) -> Vec<ExtractedRecord> {
    let mut out = Vec::new();
    for (ei, ev) in jarr(patient, "events").iter().enumerate() {
        let dt = date_prefix(ev.get("start").and_then(Value::as_str), "2024-01-01");
        let reason = jnonempty(ev, "reason", "Clinical contact");
        for (di, d) in jarr(ev, "documents").iter().enumerate() {
            if let Some(rec) = extract_document_record(ws_id, ei, di, &dt, d) {
                out.push(rec);
            }
        }
        if let Some(rec) = extract_condition_record(ws_id, ei, &dt, reason, jarr(ev, "conditions"))
        {
            out.push(rec);
        }
        if let Some(rec) =
            extract_medication_record(ws_id, ei, &dt, jstr(ev, "id"), jarr(ev, "medications"))
        {
            out.push(rec);
        }
        for (li, lab) in jarr(ev, "labs").iter().enumerate() {
            if let Some(rec) = extract_lab_record(ws_id, ei, li, &dt, lab) {
                out.push(rec);
            }
        }
        for (ri, r) in jarr(ev, "reports").iter().enumerate() {
            if let Some(rec) = extract_report_record(ws_id, ei, ri, &dt, r) {
                out.push(rec);
            }
        }
        let vit = ev.get("vitals").unwrap_or(&Value::Null);
        let v_obj = match vit {
            Value::Array(a) => a.first(),
            Value::Object(_) => Some(vit),
            _ => None,
        };
        if let Some(v) = v_obj
            && let Some(rec) = extract_vitals_record(ws_id, ei, &dt, reason, v)
        {
            out.push(rec);
        }
    }
    out
}

fn build_synopsis(ws_id: &str, summary: &str, records: &[ExtractedRecord]) -> Vec<ClinicalSegment> {
    let mut segs = vec![ClinicalSegment::Text {
        text: format!("{} ", summary.trim()),
    }];
    for rec in records.iter().rev().take(2) {
        segs.push(ClinicalSegment::Citation {
            state: "resolved".into(),
            display_label: Some(rec.title.clone()),
            source: Some("ehr".into()),
            source_id: Some(ws_id.into()),
            handle: Some(rec.id.clone()),
            source_presentation: Some(SourcePresentation {
                content: Some(SourcePreviewContent {
                    text: rec.body.clone(),
                    as_of: rec.recorded_at.clone(),
                }),
            }),
            destination: Some(CitationDestination {
                kind: "source_native".into(),
                source_label: Some(rec.source_label.clone()),
                uri: None,
            }),
        });
    }
    segs
}

fn build_initial_plan(ws_id: &str, idx: usize) -> Vec<WorkspacePlanItem> {
    const PLANS: [(&str, &str); 3] = [
        (
            "Metformin 1000 mg twice daily",
            "Continue current glycemic therapy; review A1c and renal function at upcoming visit.",
        ),
        (
            "Abdominal aortic aneurysm surveillance",
            "Confirm outside vascular ultrasound timing and smoking cessation counseling.",
        ),
        (
            "Home blood pressure log review",
            "Track twice-daily cuff readings on current losartan regimen.",
        ),
    ];
    PLANS
        .get(idx)
        .map(|(t, d)| {
            vec![WorkspacePlanItem {
                item_id: format!("plan-seed-{idx}"),
                patient_workspace_id: ws_id.into(),
                title: (*t).into(),
                detail: (*d).into(),
                decided_at: DEMO_NOW.into(),
                source_recommendation_id: None,
            }]
        })
        .unwrap_or_default()
}

fn build_recommendations(ws_id: &str, idx: usize) -> Vec<WorkspaceMedicationRecommendation> {
    const RECS: [(&str, &str, &str, &str); 2] = [
        (
            "statin",
            "High-intensity statin review (atorvastatin 40 mg)",
            "LDL remains 130 mg/dL on atorvastatin 20 mg in the setting of type 2 diabetes and PAD.",
            "Lipid panel & chart review",
        ),
        (
            "copd",
            "Step-up COPD inhaler therapy (LAMA/LABA)",
            "Worsening exertional dyspnea on olodaterol monotherapy across recent outside notes.",
            "Pulmonary clinic fax",
        ),
    ];
    RECS.get(idx)
        .map(|(slug, m, r, s)| {
            vec![WorkspaceMedicationRecommendation {
                recommendation_id: format!("rec-{ws_id}-{slug}"),
                patient_workspace_id: ws_id.into(),
                medication: (*m).into(),
                rationale: (*r).into(),
                source_label: (*s).into(),
                decision_state: "undecided".into(),
            }]
        })
        .unwrap_or_default()
}

fn to_demo_record(ws_id: &str, rec: &ExtractedRecord) -> DemoSourceRecord {
    DemoSourceRecord {
        id: rec.id.clone(),
        workspace_id: ws_id.to_owned(),
        group: rec.group.to_owned(),
        title: rec.title.clone(),
        recorded_at: rec.recorded_at.clone(),
        body: rec.body.clone(),
        source_label: rec.source_label.clone(),
        readable_parts: rec.readable_parts.clone(),
        fields: rec.fields.clone(),
        record_status: rec.record_status.clone(),
        raw_content: rec.raw_content.clone(),
    }
}

fn seed_practice_connections_and_matching(store: &DemoStore) -> Result<(), SeedError> {
    let sources = [
        PracticeSourceWire {
            source_id: SupportedSourceWire::Hint,
            label: SupportedSourceWire::Hint.label().into(),
            summary: "Direct Primary Care membership roster, notes, and demographics.".into(),
            guidance: "Connected via demo practice walkthrough.".into(),
            prerequisites: vec!["Active Hint practice administrator session".into()],
            supported_methods: vec![
                SourceConnectionMethodWire::BrowserSession,
                SourceConnectionMethodWire::SimulatedPractice,
            ],
            standing: SourceStandingWire::UpToDate {
                connected_at: "2026-08-20T14:00:00Z".into(),
                method: SourceConnectionMethodWire::SimulatedPractice,
                account_label: "Coda Primary Care Demo".into(),
                imported_records: 48,
                last_read_at: DEMO_NOW.into(),
            },
            backlog: SpruceBacklogWire::NotApplicable,
            unlinked_spruce_messages: 0,
            retained_linked_records: 48,
            retained_reply_drafts: 0,
        },
        PracticeSourceWire {
            source_id: SupportedSourceWire::ElationFhir,
            label: SupportedSourceWire::ElationFhir.label().into(),
            summary: "Longitudinal problems, medications, labs, vitals, and outside PDFs.".into(),
            guidance: "FHIR R4 clinical pull active in demo SQLite.".into(),
            prerequisites: vec!["Practice FHIR client credentials".into()],
            supported_methods: vec![
                SourceConnectionMethodWire::CredentialSignIn,
                SourceConnectionMethodWire::SimulatedPractice,
            ],
            standing: SourceStandingWire::UpToDate {
                connected_at: "2026-08-20T14:05:00Z".into(),
                method: SourceConnectionMethodWire::SimulatedPractice,
                account_label: "Elation FHIR Demo".into(),
                imported_records: 64,
                last_read_at: DEMO_NOW.into(),
            },
            backlog: SpruceBacklogWire::NotApplicable,
            unlinked_spruce_messages: 0,
            retained_linked_records: 64,
            retained_reply_drafts: 0,
        },
        PracticeSourceWire {
            source_id: SupportedSourceWire::Spruce,
            label: SupportedSourceWire::Spruce.label().into(),
            summary: "Inbound patient SMS, portal threads, attachments, and clinic faxes.".into(),
            guidance: "Spruce channel active for matched replies and triage.".into(),
            prerequisites: vec!["Practice Spruce organization webhook & API key".into()],
            supported_methods: vec![
                SourceConnectionMethodWire::CredentialSignIn,
                SourceConnectionMethodWire::SimulatedPractice,
            ],
            standing: SourceStandingWire::UpToDate {
                connected_at: "2026-08-20T14:10:00Z".into(),
                method: SourceConnectionMethodWire::SimulatedPractice,
                account_label: "Spruce Clinic Inbox".into(),
                imported_records: 14,
                last_read_at: DEMO_NOW.into(),
            },
            backlog: SpruceBacklogWire::NonePending,
            unlinked_spruce_messages: 1,
            retained_linked_records: 14,
            retained_reply_drafts: 1,
        },
    ];
    for source in &sources {
        store.save_source(source)?;
    }

    let unmatched = UnmatchedMessageWire {
        message_id: "msg-match-1".into(),
        conversation_id: "conv-unmatched-1".into(),
        sender_display_name: "Jordan Kowalczyk (Caregiver)".into(),
        sender_address: Some("+14155550192".into()),
        body_text:
            "Checking if Gary needs to hold evening metformin before tomorrow's fasting lipid lab?"
                .into(),
        attachments: Vec::new(),
        spruce_app_url: Some("https://app.sprucehealth.com/conversations/conv-unmatched-1".into()),
        waiting_since: "2026-08-27T18:10:00.000Z".into(),
        urgency: UnmatchedUrgencyWire::Ordinary,
        suggestion: MatchSuggestionWire::SinglePatient {
            patient_workspace_id: "unit-2013".into(),
            patient_display_name: "Gary Kowalczyk".into(),
            birth_date: None,
            mrn: None,
            evidence_summary:
                "Caregiver shared last name Kowalczyk and referenced tomorrow's lipid lab.".into(),
        },
    };
    let filtered = FilteredMessageWire {
        message_id: "msg-filt-1".into(),
        conversation_id: "conv-filt-1".into(),
        sender_display_name: "Summit Medical Supply".into(),
        sender_address: Some("billing@summitmedsupply.example".into()),
        body_text: "Quarterly DME catalog pricing update for clinic administrators.".into(),
        spruce_app_url: None,
        category: NonPatientCategoryWire::VendorOrSpam,
        rationale: "Commercial vendor outreach with no patient clinical inquiry.".into(),
        filtered_at: "2026-08-27T16:40:00.000Z".into(),
    };
    let unreadable = UnreadableSpruceMessageWire {
        message_id: "msg-unrd-1".into(),
        conversation_id: "conv-unrd-1".into(),
        failure_reason: "Encrypted multi-page fax PDF could not be decoded locally.".into(),
        spruce_app_url: Some("https://app.sprucehealth.com/conversations/conv-unrd-1".into()),
        kept_at: "2026-08-27T15:20:00.000Z".into(),
    };
    for (id, conv, bucket, ts, item) in [
        (
            "msg-match-1",
            "conv-unmatched-1",
            "unmatched",
            "2026-08-27T18:10:00.000Z",
            StoredMatchItem::Unmatched(unmatched),
        ),
        (
            "msg-filt-1",
            "conv-filt-1",
            "filtered",
            "2026-08-27T16:40:00.000Z",
            StoredMatchItem::Filtered(filtered),
        ),
        (
            "msg-unrd-1",
            "conv-unrd-1",
            "unreadable",
            "2026-08-27T15:20:00.000Z",
            StoredMatchItem::Unreadable(unreadable),
        ),
    ] {
        store.driver().insert_row(
            SpruceMatchQueue::Table,
            SpruceMatchQueue::COLS,
            [
                id.into(),
                conv.into(),
                bucket.into(),
                ts.into(),
                serde_json::to_string(&item)?.into(),
            ],
        )?;
    }
    Ok(())
}

fn seed_quiet_review_update(
    store: &DemoStore,
    ws_id: &str,
    display_name: &str,
    summary: &str,
) -> Result<(), SeedError> {
    let item = UpdateItemWire {
        item_id: format!("review:quiet-{ws_id}"),
        version_id: "1".into(),
        patient_workspace_id: ws_id.into(),
        patient_display_name: display_name.into(),
        kind: UpdateItemKindWire::QuietReview,
        headline: "Periodic chart review — no new physician action needed".into(),
        detail: truncate_detail(summary),
        occurred_at: "2026-08-27T13:30:00.000Z".into(),
        standing: SeenStandingWire::Unseen,
    };
    store.driver().insert_row(
        UpdatesFeed::Table,
        UpdatesFeed::COLS,
        [
            item.item_id.as_str().into(),
            ws_id.into(),
            "quiet".into(),
            item.occurred_at.as_str().into(),
            serde_json::to_string(&item)?.into(),
        ],
    )?;
    Ok(())
}

fn seed_follow_up_row(store: &DemoStore, idx: usize, ws_id: &str) -> Result<FollowUp, SeedError> {
    let summary = match idx {
        0 => "Recheck fasting lipid panel and BMP 4 weeks after statin review.",
        1 => "Confirm inhaler spacer technique and pulse oximetry at follow-up.",
        _ => "Review interval labs and home blood pressure log before next visit.",
    };
    let fu = FollowUpWire {
        follow_up_id: format!("fu-{ws_id}"),
        patient_workspace_id: ws_id.into(),
        version: 1,
        summary: summary.into(),
        standing: FollowUpStandingWire::Scheduled {
            due_at: "2026-09-03T13:00:00Z".into(),
            due_date_input: "2026-09-03".into(),
        },
        origin: FollowUpOriginWire::CodaCreated {
            run_id: format!("run-{ws_id}-1"),
        },
        latest_note: Some("Scheduled during pre-visit preparation.".into()),
        updated_at: DEMO_NOW.into(),
    };
    store.driver().insert_row(
        FollowUps::Table,
        FollowUps::COLS,
        [
            fu.follow_up_id.as_str().into(),
            ws_id.into(),
            fu.standing.badge_label().into(),
            fu.updated_at.as_str().into(),
            serde_json::to_string(&fu)?.into(),
        ],
    )?;
    Ok(FollowUp {
        follow_up_id: fu.follow_up_id,
        description: Some(fu.summary),
    })
}

fn seed_featured(
    store: &DemoStore,
    idx: usize,
    ws_id: &str,
    display_name: &str,
    given: &str,
    summary: &str,
    records: &[ExtractedRecord],
) -> Result<(&'static str, Vec<WorkspaceReply>), SeedError> {
    const ITEMS: [(&str, &str, &str, &str); 4] = [
        (
            "now",
            "concern",
            "LDL 130 on atorvastatin 20 mg with PAD & diabetes",
            "2026-08-27T17:15:00.000Z",
        ),
        (
            "now",
            "drafted_reply",
            "Patient messaged about exertional shortness of breath",
            "2026-08-27T17:45:00.000Z",
        ),
        (
            "today",
            "concern",
            "Home BP readings trending 142/88 ahead of follow-up",
            "2026-08-27T15:30:00.000Z",
        ),
        (
            "today",
            "preparation",
            "Visit preparation summary ready",
            "2026-08-27T14:00:00.000Z",
        ),
    ];
    let (urgency, kind, reason, waited) = ITEMS[idx.min(3)];
    let is_spruce_row = idx == 1;
    let conv_id = format!("conv-{ws_id}-1");
    let item_id = if is_spruce_row {
        format!("spruce-reply:{conv_id}")
    } else {
        format!("today-{ws_id}")
    };
    let chat_id = format!("chat-{ws_id}-1");
    let row_json = json!({
        "patient_workspace_id": ws_id,
        "patient_display_name": display_name,
        "item_id": item_id,
        "version_id": format!("ver-{ws_id}-1"),
        "waiting_since": waited,
        "urgency": urgency,
        "kind": kind,
        "title": reason,
        "reason": reason,
        "can_mark_done": !is_spruce_row,
        "linked_chat_id": chat_id,
        "spruce_conversation_id": if is_spruce_row { json!(conv_id) } else { Value::Null },
    });
    store.driver().insert_row(
        TodayItems::Table,
        TodayItems::COLS,
        [
            item_id.into(),
            ws_id.into(),
            urgency.into(),
            waited.into(),
            kind.into(),
            row_json.to_string().into(),
            0.into(),
            0.into(),
        ],
    )?;

    let (hr, min) = (19 + (idx % 4), if idx >= 4 { 45 } else { 30 });
    let starts = format!("2026-08-27T{hr:02}:{min:02}:00.000Z");
    let ends = format!("2026-08-27T{hr:02}:55:00.000Z");
    let briefing_id = format!("briefing-{ws_id}");
    let visit_label = if idx == 0 {
        "Cardiometabolic follow-up"
    } else {
        "Primary care follow-up"
    };
    let visit_json = json!({
        "patient_display_name": display_name,
        "patient_workspace_id": ws_id,
        "label": visit_label,
        "starts_at": starts,
        "ends_at": ends,
        "status": "scheduled",
        "patient_time_zone": DEMO_ZONE,
        "preparation": {
            "state": "prepared",
            "briefing_id": briefing_id,
            "last_attempt": { "outcome": "completed" }
        }
    });
    store.driver().insert_row(
        PracticeVisits::Table,
        PracticeVisits::COLS,
        [
            format!("visit-{ws_id}").into(),
            starts.as_str().into(),
            visit_json.to_string().into(),
        ],
    )?;

    let demo_records: Vec<DemoSourceRecord> =
        records.iter().map(|r| to_demo_record(ws_id, r)).collect();
    seed_visit_briefing(
        store,
        ws_id,
        display_name,
        &briefing_id,
        visit_label,
        &starts,
        &ends,
        summary,
        &demo_records,
    )?;

    let (bucket, update_kind, update_time) = match idx {
        0 => ("today", UpdateItemKindWire::ConcernRaised, waited),
        1 => ("today", UpdateItemKindWire::AssessedReview, waited),
        2 => ("today", UpdateItemKindWire::VisitBriefing, waited),
        3 | 4 => (
            "yesterday",
            UpdateItemKindWire::AssessedReview,
            "2026-08-26T18:10:00.000Z",
        ),
        _ => ("quiet", UpdateItemKindWire::QuietReview, waited),
    };
    let update_item = UpdateItemWire {
        item_id: format!("upd-{ws_id}"),
        version_id: "1".into(),
        patient_workspace_id: ws_id.into(),
        patient_display_name: display_name.into(),
        kind: update_kind,
        headline: reason.into(),
        detail: truncate_detail(summary),
        occurred_at: update_time.into(),
        standing: SeenStandingWire::Unseen,
    };
    store.driver().insert_row(
        UpdatesFeed::Table,
        UpdatesFeed::COLS,
        [
            update_item.item_id.as_str().into(),
            ws_id.into(),
            bucket.into(),
            update_time.into(),
            serde_json::to_string(&update_item)?.into(),
        ],
    )?;

    let replies = if idx < 2 {
        let draft = if idx == 0 {
            format!(
                "Hi {given}, thank you for checking in. Before your upcoming visit, please bring your current pill bottles so we can review your cholesterol and blood pressure regimen together."
            )
        } else {
            format!(
                "Hi {given}, I reviewed your note about shortness of breath on exertion. Please let our clinic know if symptoms occur at rest, and we will review stepping up your daily inhaler."
            )
        };
        if idx == 1 {
            seed_spruce_reply_card(
                store,
                ws_id,
                display_name,
                &conv_id,
                waited,
                &draft,
                &demo_records,
            )?;
        }
        vec![WorkspaceReply {
            reply_work_id: format!("reply-{ws_id}"),
            patient_workspace_id: ws_id.into(),
            patient_display_name: display_name.into(),
            channel: if idx == 0 { "email" } else { "spruce" }.into(),
            headline: reason.into(),
            draft_text: draft,
            urgency: "routine".into(),
            created_at: waited.into(),
            state: "ready".into(),
            superseded_by_input: None,
            superseded_reason: None,
            draft_revision: Some(1),
            revision_run: None,
        }]
    } else {
        Vec::new()
    };

    if idx < 4 {
        seed_patient_reviews(store, idx, ws_id, reason, summary, &demo_records)?;
    }

    let mem = PatientMemoryResponse {
        patient_workspace_id: ws_id.into(),
        durable: true,
        summary: Some(MemoryDocumentWire {
            version: 1,
            saved_at: DEMO_DATE.into(),
            markdown: format!("# {display_name}\n\n{}", summary.trim()),
        }),
        direction: Some(MemoryDocumentWire {
            version: 1,
            saved_at: DEMO_DATE.into(),
            markdown: format!("Priority focus: {reason}"),
        }),
    };
    store.driver().insert_row(
        PatientMemory::Table,
        PatientMemory::COLS,
        [ws_id.into(), 0.into(), serde_json::to_string(&mem)?.into()],
    )?;

    let cited_recs: Vec<_> = demo_records.iter().rev().take(2).cloned().collect();
    let (reads, query, answer_text, segments, basis, sources) = if cited_recs.is_empty() {
        (
            Vec::new(),
            "",
            format!("{summary} Ask any question below to query this patient's SQLite chart."),
            vec![MessageSegment::Text {
                text: format!(
                    "{summary} Ask any question below to query this patient's SQLite chart."
                ),
            }],
            Vec::new(),
            BTreeMap::new(),
        )
    } else {
        let reads = cited_recs
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    RecordGroup::Notes,
                    r.title.clone(),
                    r.recorded_at.clone(),
                )
            })
            .collect();
        let (_, segments, basis, sources) = synthesize_chart_answer_with_citations(
            "recent summary labs medications",
            &cited_recs,
            &format!("seed-{ws_id}"),
        );
        let latest = &cited_recs[0];
        let text = format!(
            "{summary}\n\nLatest documented record: {} ({}). Ask any question below to run live SQLite chart search across {} records.",
            latest.title,
            latest.recorded_at,
            demo_records.len()
        );
        (
            reads,
            "recent summary labs medications",
            text,
            segments,
            basis,
            sources,
        )
    };
    let snap = ChatSnapshot {
        sources,
        queued: Vec::new(),
        archived: false,
        transcript: vec![build_exchange_with_segments(
            "seed",
            ws_id,
            (
                format!("msg-seed-{ws_id}"),
                "Summarize key active risks and latest chart updates.".into(),
                1_787_850_000,
            ),
            query,
            &reads,
            records.len() > 1,
            (answer_text, segments, basis),
        )],
        feedback: SnapshotFeedback::Loaded,
    };
    store.driver().insert_row(
        Chats::Table,
        Chats::COLS,
        [
            chat_id.into(),
            ws_id.into(),
            "Pre-visit chart brief".into(),
            waited.into(),
            serde_json::to_string(&snap)?.into(),
        ],
    )?;
    Ok((reason, replies))
}

#[allow(clippy::too_many_arguments)]
fn seed_visit_briefing(
    store: &DemoStore,
    ws_id: &str,
    display_name: &str,
    briefing_id: &str,
    visit_label: &str,
    starts_at: &str,
    ends_at: &str,
    summary: &str,
    records: &[DemoSourceRecord],
) -> Result<(), SeedError> {
    let mut sources = BTreeMap::new();
    let mut cite_seg = Vec::new();
    let mut basis_segments = vec![MessageSegment::Text {
        text: "Prepared from saved browser SQLite chart records".into(),
    }];
    if let Some(rec) = records.last() {
        let key = format!("b1-{ws_id}");
        let cited = build_cited_source(&key, 1, rec);
        let label = format!("{} ({})", rec.title, rec.recorded_at);
        let target = if cited.quote.is_empty() {
            CitationTargetKind::WholeRecord
        } else {
            CitationTargetKind::Passage
        };
        let quote = cited.quote.clone();
        cite_seg.push(MessageSegment::Text {
            text: format!("{} ", summary.trim()),
        });
        cite_seg.push(MessageSegment::Citation {
            source_key: key.clone(),
            number: 1,
            label: label.clone(),
            quote: quote.clone(),
            target,
        });
        basis_segments.push(MessageSegment::Text {
            text: format!(": {label} "),
        });
        basis_segments.push(MessageSegment::Citation {
            source_key: key.clone(),
            number: 1,
            label,
            quote,
            target,
        });
        basis_segments.push(MessageSegment::Text {
            text: format!(
                ". Currency & scope: reflects synthetic EHR fixtures documented through {}.",
                rec.recorded_at
            ),
        });
        sources.insert(key, cited);
    } else {
        cite_seg.push(MessageSegment::Text {
            text: summary.trim().to_owned(),
        });
        basis_segments.push(MessageSegment::Text { text: ".".into() });
    }
    let briefing = VisitBriefingWire {
        briefing_id: briefing_id.to_owned(),
        patient_workspace_id: ws_id.to_owned(),
        patient_display_name: display_name.to_owned(),
        source_id: "demo-calendar".into(),
        appointment_id: format!("visit-{ws_id}"),
        visit_label: visit_label.to_owned(),
        starts_at: starts_at.to_owned(),
        ends_at: ends_at.to_owned(),
        prepared_at: DEMO_NOW.into(),
        visit_standing: BriefingVisitStandingWire::Current,
        visit_card_orientation: truncate_detail(summary),
        source_freshness: vec![BriefingSourceStatusWire::Updated {
            source_id: "synthetic-ehr".into(),
            source_label: "Practice EHR".into(),
            last_good_at: DEMO_NOW.into(),
        }],
        limits: Vec::new(),
        trajectory: vec![BriefingClaimWire {
            segments: cite_seg.clone(),
            uncertainty: None,
        }],
        meaningful_changes: vec![BriefingClaimWire {
            segments: vec![MessageSegment::Text {
                text: "Prepared directly from the seeded chart records in browser SQLite.".into(),
            }],
            uncertainty: None,
        }],
        unresolved_issues: Vec::new(),
        visit_decisions: vec![BriefingDecisionWire {
            decision: "Confirm medication adherence and next surveillance interval".into(),
            rationale_segments: cite_seg,
            options_or_questions: vec![
                "Verify home monitoring readings and tolerance of current regimen.".into(),
            ],
            uncertainty: None,
        }],
        basis_segments,
        sources,
    };
    store.driver().insert_row(
        Briefings::Table,
        Briefings::COLS,
        [
            briefing_id.into(),
            ws_id.into(),
            serde_json::to_string(&briefing)?.into(),
        ],
    )?;
    Ok(())
}

fn seed_spruce_reply_card(
    store: &DemoStore,
    ws_id: &str,
    display_name: &str,
    conv_id: &str,
    waited: &str,
    draft: &str,
    records: &[DemoSourceRecord],
) -> Result<(), SeedError> {
    let citations = records.iter().take(2).map(build_item_citation).collect();
    let stored = StoredSpruceReply {
        surface: PatientReplyConversationWire::OpenCard(Box::new(ReplyCardWire {
            conversation_id: conv_id.to_owned(),
            patient_workspace_id: ws_id.to_owned(),
            patient_display_name: display_name.to_owned(),
            primary_sender_name: display_name.to_owned(),
            primary_sender_role: ChannelSenderRoleWire::Patient,
            non_patient_sender_names: Vec::new(),
            spruce_app_url: Some(format!("https://app.sprucehealth.com/conversations/{conv_id}")),
            reply_id: 201,
            conversation_version: 1,
            proposal_version: 1,
            has_open_concern: true,
            patient_messages: vec![ReplyPatientMessageWire {
                message_id: format!("msg-spruce-{ws_id}-1"),
                sequence: 1,
                sender_display_name: display_name.to_owned(),
                sender_role: ChannelSenderRoleWire::Patient,
                text: "I have been more short of breath walking up the stairs this week and my rescue inhaler hasn't helped much.".into(),
                sent_at: waited.to_owned(),
                arrived_after_proposal: false,
            }],
            practice_messages: Vec::new(),
            displayed_reply: DisplayedReplyTextWire::CodaProposal {
                proposal_version: 1,
                text: draft.to_owned(),
            },
            revision_run: None,
            coda_update_offer: None,
            notices: Vec::new(),
            basis_and_limitations:
                "Grounded in outpatient pulmonary note and active inhaler list in browser SQLite."
                    .into(),
            citations,
            other_physician_copies: Vec::new(),
            copied_by_me: false,
            review_state: CardReviewStateWire::Idle,
            channel_banner: CardChannelBannerWire::Connected,
            pending_move: None,
        })),
        prior_open_card: None,
        prior_move_source_ws: None,
    };
    store.save_stored_reply(conv_id, ws_id, waited, &stored)?;
    Ok(())
}

fn seed_patient_reviews(
    store: &DemoStore,
    idx: usize,
    ws_id: &str,
    reason: &str,
    summary: &str,
    records: &[DemoSourceRecord],
) -> Result<(), SeedError> {
    let mut sources = BTreeMap::new();
    let mut evidence = Vec::new();
    if let Some(rec) = records.last() {
        let key = format!("rev-{ws_id}-s1");
        let cited = build_cited_source(&key, 1, rec);
        evidence.push(ReviewEvidenceRefWire {
            source_key: key.clone(),
            number: 1,
            candidate_id: rec.id.clone(),
            revision: "1".into(),
            source_label: rec.source_label.clone(),
            display_label: rec.title.clone(),
            as_of: Some(rec.recorded_at.clone()),
            excerpt: cited.quote.clone(),
        });
        sources.insert(key, cited);
    }
    let completed = PatientReviewWire {
        review_id: format!("rev-{ws_id}-1"),
        version: ReviewVersion("rv-1".into()),
        work_id: format!("wi-rev-{ws_id}-1"),
        triggers: vec![ReviewTriggerWire::ChangedRecords {
            workspace_version: 1,
            changed_records: vec![reason.into()],
        }],
        accepted_inputs: Vec::new(),
        state: PatientReviewStateWire::Completed {
            outcome: PatientReviewOutcomeWire::Assessed {
                summary: format!("{reason}. {}", truncate_detail(summary)),
                segments: vec![MessageSegment::Text {
                    text: format!("{reason}. {}", truncate_detail(summary)),
                }],
                basis: vec![MessageSegment::Text {
                    text: "Synthesized from chart records stored in browser SQLite.".into(),
                }],
            },
            completed_at: DEMO_NOW.into(),
        },
        steps: vec![ReviewStepWire {
            label: "Read latest clinical chart records".into(),
            detail: Some(format!("{} record(s)", records.len())),
            state: ReviewStepStateWire::Done,
        }],
        evidence,
        sources,
        actions: vec![ReviewActionWire {
            summary: format!("Prepared physician review item: {reason}"),
        }],
        unfinished_work: Vec::new(),
        queued_at: "2026-08-27T16:50:00Z".into(),
        updated_at: DEMO_NOW.into(),
    };
    store.driver().insert_row(
        PatientReviews::Table,
        PatientReviews::COLS,
        [
            completed.review_id.as_str().into(),
            ws_id.into(),
            completed.updated_at.as_str().into(),
            serde_json::to_string(&completed)?.into(),
        ],
    )?;
    if idx == 0 {
        let held = PatientReviewWire {
            review_id: format!("rev-{ws_id}-held"),
            version: ReviewVersion("rv-1".into()),
            work_id: format!("wi-rev-{ws_id}-held"),
            triggers: vec![ReviewTriggerWire::Periodic {
                workspace_version: 1,
                last_reviewed_at: Some("2026-08-20T12:00:00Z".into()),
            }],
            accepted_inputs: Vec::new(),
            state: PatientReviewStateWire::Held {
                reason: ReviewHoldReasonWire::Limit,
                held_at: "2026-08-27T18:45:00Z".into(),
                partial_text: Some(
                    "Reviewed interval metabolic panel; awaiting outside ultrasound check.".into(),
                ),
            },
            steps: vec![ReviewStepWire {
                label: "Check interval labs and vascular imaging".into(),
                detail: Some("Turn limit reached".into()),
                state: ReviewStepStateWire::Stopped,
            }],
            evidence: Vec::new(),
            sources: BTreeMap::new(),
            actions: Vec::new(),
            unfinished_work: Vec::new(),
            queued_at: "2026-08-27T18:40:00Z".into(),
            updated_at: "2026-08-27T18:45:00Z".into(),
        };
        store.driver().insert_row(
            PatientReviews::Table,
            PatientReviews::COLS,
            [
                held.review_id.as_str().into(),
                ws_id.into(),
                held.updated_at.as_str().into(),
                serde_json::to_string(&held)?.into(),
            ],
        )?;
    }
    Ok(())
}

fn truncate_detail(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= 140 {
        t.to_owned()
    } else {
        format!("{}…", t.chars().take(139).collect::<String>())
    }
}

fn infer_sex_label(summary: &str) -> &'static str {
    let l = summary.to_ascii_lowercase();
    if l.contains(" she ") || l.contains(" her ") {
        "Female"
    } else if l.contains(" he ") || l.contains(" his ") || l.contains(" him ") {
        "Male"
    } else {
        "Unspecified"
    }
}

#[allow(clippy::too_many_lines)]
fn seed_maya_lin_workspace(store: &DemoStore) -> Result<(), SeedError> {
    let ws_id = MAYA_LIN_WORKSPACE_ID;
    let display_name = "Maya Lin";
    let mrn = "MRN00004412";
    let birth_date = "1992-04-18";
    let summary = "34-year-old woman with generalized anxiety disorder (GAD-7 12/21, down from 15/21) and mild depressive symptoms (PHQ-9 8/27, down from 11/27) on escitalopram 10 mg daily, sharing 14-day Abbott Lingo CGM biosensor data and daily mental health check-ins through Duet Connected Care.";

    let cgm_14d_body = "14-day Abbott Lingo CGM biosensor summary (2026-08-13 to 2026-08-26, n = 1,344 readings, 99% sensor wear): mean glucose 108.4 mg/dL, nadir 68.0 mg/dL, peak 176.0 mg/dL, SD 24.9 mg/dL, glycemic variability CV 23.0%, Time in Tight Range (70–140 mg/dL) 81.5%, Time in Range (70–180 mg/dL) 98.8%, 11 post-prandial excursions >140 mg/dL, 3 brief reactive dips <70 mg/dL, mean Lingo Count 64.0 pts/day.";
    let corr_body = "Abbott Lingo CGM & Duet symptom correlation log (2026-08-19 to 2026-08-25): high-glycemic lunches (sweetened oat latte + white-rice bowl) produced peaks of 168–176 mg/dL within 50 minutes followed by rapid drops to 68–73 mg/dL between 2:30 PM and 3:30 PM, coinciding with acute palpitations, shakiness, and afternoon anxiety ratings of 8/10 on 5 of 7 days. On protein-first lunches (eggs, salmon, greens), post-meal glucose stayed <= 124 mg/dL and afternoon anxiety stayed 3/10.";
    let scales_body = "Longitudinal PHQ-9 & GAD-7 mental health assessments (2026-08-24 vs 2026-07-10 baseline): GAD-7 total score 12/21 (Moderate, improved from 15/21 Severe on 2026-07-10); PHQ-9 total score 8/27 (Mild, improved from 11/27 Moderate on 2026-07-10; item-9 suicidal ideation score 0). Residual anxiety clusters in mid-afternoon workdays.";
    let checkins_body = "7-day Duet daily mood, anxiety, energy, and sleep check-in rollup (2026-08-20 to 2026-08-26): mean mood 6.5/10, mean afternoon anxiety 6.8/10 (8/10 after <6.0 h sleep vs 3.5/10 after >=7.5 h sleep), mean daytime energy 5.8/10, mean sleep duration 6.4 h. Nights <6.0 h preceded higher next-day Lingo Count (78 pts vs 38 pts).";
    let meds_body = "Psychiatry & integrative primary care follow-up note (2026-08-12): Escitalopram 10 mg PO daily started 2026-06-15, well tolerated without GI or sexual side effects. Biweekly CBT ongoing. Patient wearing Abbott Lingo upper-arm biosensor to evaluate afternoon adrenergic spells versus panic episodes; trial pairing carbohydrates with >=25 g protein + 10-minute post-lunch walk.";

    let demo_records = vec![
        DemoSourceRecord {
            id: format!("rec-{ws_id}-plan"),
            workspace_id: ws_id.into(),
            group: "medications".into(),
            title: "Escitalopram 10 mg & Behavioral Care Plan".into(),
            recorded_at: "2026-08-12".into(),
            body: meds_body.into(),
            source_label: "Primary EHR".into(),
            readable_parts: vec![CitedTextPartWire {
                label: Some("Care Plan".into()),
                text: meds_body.into(),
            }],
            fields: vec![CitedFieldWire {
                key: "escitalopram_dose".into(),
                name: "Escitalopram".into(),
                value: "10".into(),
                unit: Some("mg daily".into()),
                range: None,
                status: Some("Active".into()),
            }],
            record_status: "final".into(),
            raw_content: None,
        },
        DemoSourceRecord {
            id: format!("rec-{ws_id}-phq-gad"),
            workspace_id: ws_id.into(),
            group: "results".into(),
            title: "Longitudinal GAD-7 & PHQ-9 Screening".into(),
            recorded_at: "2026-08-24".into(),
            body: scales_body.into(),
            source_label: "Duet Mental Health Check-In".into(),
            readable_parts: vec![CitedTextPartWire {
                label: Some("Standardized Scales".into()),
                text: scales_body.into(),
            }],
            fields: vec![
                CitedFieldWire {
                    key: "gad7_score".into(),
                    name: "GAD-7 (Moderate)".into(),
                    value: "12".into(),
                    unit: Some("/21".into()),
                    range: Some("0–4 Minimal (prior 15/21)".into()),
                    status: Some("Moderate".into()),
                },
                CitedFieldWire {
                    key: "phq9_score".into(),
                    name: "PHQ-9 (Mild)".into(),
                    value: "8".into(),
                    unit: Some("/27".into()),
                    range: Some("0–4 Minimal (prior 11/27)".into()),
                    status: Some("Mild".into()),
                },
            ],
            record_status: "final".into(),
            raw_content: None,
        },
        DemoSourceRecord {
            id: format!("rec-{ws_id}-corr"),
            workspace_id: ws_id.into(),
            group: "notes".into(),
            title: "Post-Prandial Lingo Spikes & Afternoon Anxiety Correlation".into(),
            recorded_at: "2026-08-25".into(),
            body: corr_body.into(),
            source_label: "Abbott Lingo CGM + Duet".into(),
            readable_parts: vec![CitedTextPartWire {
                label: Some("Symptom & Glucose Correlation".into()),
                text: corr_body.into(),
            }],
            fields: vec![
                CitedFieldWire {
                    key: "afternoon_anxiety_spike".into(),
                    name: "Afternoon Anxiety (post-spike days)".into(),
                    value: "8".into(),
                    unit: Some("/10".into()),
                    range: Some("1–4".into()),
                    status: Some("Elevated".into()),
                },
                CitedFieldWire {
                    key: "reactive_glucose_nadir".into(),
                    name: "Post-Lunch Reactive Nadir".into(),
                    value: "68.0".into(),
                    unit: Some("mg/dL".into()),
                    range: Some(">= 70 mg/dL".into()),
                    status: Some("Low".into()),
                },
            ],
            record_status: "final".into(),
            raw_content: None,
        },
        DemoSourceRecord {
            id: format!("rec-{ws_id}-checkins"),
            workspace_id: ws_id.into(),
            group: "results".into(),
            title: "7-Day Mood, Anxiety, Energy & Sleep Check-Ins".into(),
            recorded_at: "2026-08-26".into(),
            body: checkins_body.into(),
            source_label: "Duet Mental Health Check-In".into(),
            readable_parts: vec![CitedTextPartWire {
                label: Some("7-Day Check-In Rollup".into()),
                text: checkins_body.into(),
            }],
            fields: vec![
                CitedFieldWire {
                    key: "mean_mood".into(),
                    name: "7-Day Mean Mood".into(),
                    value: "6.5".into(),
                    unit: Some("/10".into()),
                    range: Some("6–10".into()),
                    status: Some("Normal".into()),
                },
                CitedFieldWire {
                    key: "mean_sleep".into(),
                    name: "7-Day Mean Sleep".into(),
                    value: "6.4".into(),
                    unit: Some("h".into()),
                    range: Some("7.0–9.0 h".into()),
                    status: Some("Low".into()),
                },
            ],
            record_status: "final".into(),
            raw_content: None,
        },
        DemoSourceRecord {
            id: format!("rec-{ws_id}-cgm-14d"),
            workspace_id: ws_id.into(),
            group: "results".into(),
            title: "14-Day Abbott Lingo CGM Biosensor Summary".into(),
            recorded_at: "2026-08-26".into(),
            body: cgm_14d_body.into(),
            source_label: "Abbott Lingo CGM".into(),
            readable_parts: vec![CitedTextPartWire {
                label: Some("14-Day Lingo Biosensor Summary".into()),
                text: cgm_14d_body.into(),
            }],
            fields: vec![
                CitedFieldWire {
                    key: "lingo_mean_glucose".into(),
                    name: "Mean Glucose (14d Lingo)".into(),
                    value: "108.4".into(),
                    unit: Some("mg/dL".into()),
                    range: Some("70–115 mg/dL".into()),
                    status: Some("Normal".into()),
                },
                CitedFieldWire {
                    key: "lingo_peak_glucose".into(),
                    name: "Peak Glucose".into(),
                    value: "176.0".into(),
                    unit: Some("mg/dL".into()),
                    range: Some("<= 140 mg/dL".into()),
                    status: Some("Elevated".into()),
                },
                CitedFieldWire {
                    key: "lingo_cv_percent".into(),
                    name: "Glycemic Variability (CV)".into(),
                    value: "23.0".into(),
                    unit: Some("%".into()),
                    range: Some("< 20.0%".into()),
                    status: Some("Elevated".into()),
                },
                CitedFieldWire {
                    key: "lingo_tir_70_140".into(),
                    name: "Time in Tight Range (70–140)".into(),
                    value: "81.5".into(),
                    unit: Some("%".into()),
                    range: Some(">= 85.0%".into()),
                    status: Some(crate::lingo::tight_tir_status(81.5).into()),
                },
                CitedFieldWire {
                    key: "lingo_daily_count".into(),
                    name: "Mean Daily Lingo Count".into(),
                    value: "64.0".into(),
                    unit: Some("pts/day".into()),
                    range: Some("<= 45 pts/day".into()),
                    status: Some("Elevated".into()),
                },
            ],
            record_status: "final".into(),
            raw_content: None,
        },
    ];

    for rec in &demo_records {
        store.insert_structured_source_record(rec)?;
    }

    let reason = "Abbott Lingo CGM post-lunch spikes (176 mg/dL) & reactive dips correlating with afternoon GAD-7 anxiety";
    let waited = "2026-08-27T16:20:00.000Z";
    let item_id = format!("today-{ws_id}");
    let chat_id = format!("chat-{ws_id}-1");
    let row_json = json!({
        "patient_workspace_id": ws_id,
        "patient_display_name": display_name,
        "item_id": item_id,
        "version_id": format!("ver-{ws_id}-1"),
        "waiting_since": waited,
        "urgency": "today",
        "kind": "concern",
        "title": reason,
        "reason": reason,
        "can_mark_done": true,
        "linked_chat_id": chat_id,
        "spruce_conversation_id": Value::Null,
    });
    store.driver().insert_row(
        TodayItems::Table,
        TodayItems::COLS,
        [
            item_id.as_str().into(),
            ws_id.into(),
            "today".into(),
            waited.into(),
            "concern".into(),
            row_json.to_string().into(),
            0.into(),
            0.into(),
        ],
    )?;

    let starts = "2026-08-27T21:00:00.000Z";
    let ends = "2026-08-27T21:30:00.000Z";
    let briefing_id = format!("briefing-{ws_id}");
    let visit_label = "Mental health & Lingo CGM review";
    let visit_json = json!({
        "patient_display_name": display_name,
        "patient_workspace_id": ws_id,
        "label": visit_label,
        "starts_at": starts,
        "ends_at": ends,
        "status": "scheduled",
        "patient_time_zone": DEMO_ZONE,
        "preparation": {
            "state": "prepared",
            "briefing_id": briefing_id,
            "last_attempt": { "outcome": "completed" }
        }
    });
    store.driver().insert_row(
        PracticeVisits::Table,
        PracticeVisits::COLS,
        [
            format!("visit-{ws_id}").into(),
            starts.into(),
            visit_json.to_string().into(),
        ],
    )?;

    seed_visit_briefing(
        store,
        ws_id,
        display_name,
        &briefing_id,
        visit_label,
        starts,
        ends,
        summary,
        &demo_records,
    )?;

    let update_item = UpdateItemWire {
        item_id: format!("upd-{ws_id}"),
        version_id: "1".into(),
        patient_workspace_id: ws_id.into(),
        patient_display_name: display_name.into(),
        kind: UpdateItemKindWire::AssessedReview,
        headline: reason.into(),
        detail: truncate_detail(summary),
        occurred_at: waited.into(),
        standing: SeenStandingWire::Unseen,
    };
    store.driver().insert_row(
        UpdatesFeed::Table,
        UpdatesFeed::COLS,
        [
            update_item.item_id.as_str().into(),
            ws_id.into(),
            "today".into(),
            waited.into(),
            serde_json::to_string(&update_item)?.into(),
        ],
    )?;

    seed_patient_reviews(store, 3, ws_id, reason, summary, &demo_records)?;

    let fu = FollowUpWire {
        follow_up_id: format!("fu-{ws_id}-1"),
        patient_workspace_id: ws_id.into(),
        version: 1,
        summary: "2-week Abbott Lingo CGM & GAD-7 check-in after protein-paired lunch trial".into(),
        standing: FollowUpStandingWire::Scheduled {
            due_at: "2026-09-10T13:00:00Z".into(),
            due_date_input: "2026-09-10".into(),
        },
        origin: FollowUpOriginWire::CodaCreated {
            run_id: format!("run-{ws_id}-1"),
        },
        latest_note: Some(
            "Reassess afternoon anxiety ratings, post-lunch glucose peaks <=140 mg/dL, and sleep continuity.".into(),
        ),
        updated_at: DEMO_NOW.into(),
    };
    store.driver().insert_row(
        FollowUps::Table,
        FollowUps::COLS,
        [
            fu.follow_up_id.as_str().into(),
            ws_id.into(),
            fu.standing.badge_label().into(),
            fu.updated_at.as_str().into(),
            serde_json::to_string(&fu)?.into(),
        ],
    )?;

    let mem = PatientMemoryResponse {
        patient_workspace_id: ws_id.into(),
        durable: true,
        summary: Some(MemoryDocumentWire {
            version: 1,
            saved_at: DEMO_DATE.into(),
            markdown: format!("# {display_name}\n\n{summary}"),
        }),
        direction: Some(MemoryDocumentWire {
            version: 1,
            saved_at: DEMO_DATE.into(),
            markdown: "Correlate afternoon anxiety spells with Abbott Lingo post-prandial glucose drops and <6 h sleep nights; maintain escitalopram 10 mg daily and biweekly CBT.".into(),
        }),
    };
    store.driver().insert_row(
        PatientMemory::Table,
        PatientMemory::COLS,
        [ws_id.into(), 0.into(), serde_json::to_string(&mem)?.into()],
    )?;

    let cited_recs: Vec<_> = demo_records.iter().rev().take(3).cloned().collect();
    let reads: Vec<_> = cited_recs
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                RecordGroup::Results,
                r.title.clone(),
                r.recorded_at.clone(),
            )
        })
        .collect();
    let query = "lingo glucose spikes afternoon anxiety gad7 sleep";
    let (answer_text, segments, basis, sources) =
        synthesize_chart_answer_with_citations(query, &cited_recs, &format!("seed-{ws_id}"));
    let snap = ChatSnapshot {
        sources,
        queued: Vec::new(),
        archived: false,
        transcript: vec![build_exchange_with_segments(
            "seed",
            ws_id,
            (
                format!("msg-seed-{ws_id}"),
                "Correlate Maya's Abbott Lingo CGM patterns with her GAD-7, PHQ-9, and afternoon anxiety check-ins.".into(),
                1_787_850_120,
            ),
            query,
            &reads,
            true,
            (answer_text, segments, basis),
        )],
        feedback: SnapshotFeedback::Loaded,
    };
    store.driver().insert_row(
        Chats::Table,
        Chats::COLS,
        [
            chat_id.into(),
            ws_id.into(),
            "Mental health & Lingo CGM brief".into(),
            waited.into(),
            serde_json::to_string(&snap)?.into(),
        ],
    )?;

    let synopsis_rec = &demo_records[4];
    let workspace = PatientWorkspaceResponse {
        patient_workspace_id: ws_id.into(),
        display_name: display_name.into(),
        membership: WorkspaceMembership::Allowed,
        time_zone: Some(DEMO_ZONE.into()),
        chats: Vec::new(),
        identity: Some(PatientIdentity {
            display_name: Some(display_name.into()),
            birth_date: Some(birth_date.into()),
            age_years: Some(34),
            sex_or_gender: Some(SexOrGender {
                value: Some("female".into()),
                label: Some("Female".into()),
            }),
            medical_record_numbers: vec![MedicalRecordNumber {
                value: Some(mrn.into()),
            }],
            email: None,
            state: Some("available".into()),
        }),
        patient_synopsis: Some(PatientSynopsis {
            version_id: format!("syn-{ws_id}-v1"),
            authored_text: None,
            segments: vec![
                ClinicalSegment::Text {
                    text: format!("{summary} "),
                },
                ClinicalSegment::Citation {
                    state: "resolved".into(),
                    display_label: Some(synopsis_rec.title.clone()),
                    source: Some("abbott-lingo-cgm".into()),
                    source_id: Some(ws_id.into()),
                    handle: Some(synopsis_rec.id.clone()),
                    source_presentation: Some(SourcePresentation {
                        content: Some(SourcePreviewContent {
                            text: synopsis_rec.body.clone(),
                            as_of: synopsis_rec.recorded_at.clone(),
                        }),
                    }),
                    destination: Some(CitationDestination {
                        kind: "source_native".into(),
                        source_label: Some(synopsis_rec.source_label.clone()),
                        uri: None,
                    }),
                },
            ],
            authored_at: Some(DEMO_NOW.into()),
        }),
        automation_run: Some(AutomationRun {
            kind: "visit_preparation".into(),
            state: "finished".into(),
            ended_cause: None,
            purpose: Some(WorkPurpose {
                headline: Some(reason.into()),
            }),
        }),
        active_follow_ups: vec![FollowUp {
            follow_up_id: fu.follow_up_id,
            description: Some(fu.summary),
        }],
        has_past_work: true,
        last_reviewed_at: Some(DEMO_NOW.into()),
        connected_sources: vec![
            ConnectedSource {
                source_id: Some("synthetic-ehr".into()),
                source_label: Some("Primary EHR".into()),
                state: SourceConnectionState {
                    state: "usable".into(),
                    synchronized_at: Some(DEMO_NOW.into()),
                    usable_snapshot: Some(SourceSnapshot {
                        synchronized_at: Some(DEMO_NOW.into()),
                    }),
                },
            },
            ConnectedSource {
                source_id: Some("abbott-lingo-cgm".into()),
                source_label: Some("Abbott Lingo CGM (via Duet)".into()),
                state: SourceConnectionState {
                    state: "usable".into(),
                    synchronized_at: Some(DEMO_NOW.into()),
                    usable_snapshot: Some(SourceSnapshot {
                        synchronized_at: Some(DEMO_NOW.into()),
                    }),
                },
            },
        ],
        replies: Vec::new(),
        superseded_replies: Vec::new(),
        active_plan_items: vec![WorkspacePlanItem {
            item_id: format!("plan-{ws_id}-1"),
            patient_workspace_id: ws_id.into(),
            title: "Protein-paired lunch + 10-min walk trial".into(),
            detail: "Pair lunch carbohydrates with >=25 g protein + 10-minute walk and correlate afternoon anxiety check-ins against Abbott Lingo CGM.".into(),
            decided_at: DEMO_NOW.into(),
            source_recommendation_id: None,
        }],
        undecided_recommendations: build_recommendations(ws_id, 3),
        connection_status: ConnectionStatusWire::Connected,
        patient_link: Some(PatientLinkWire {
            practice_id: "prac-coda-demo".into(),
            patient_workspace_id: ws_id.into(),
            duet_account_id: Some(format!("duet-{ws_id}")),
            linked: true,
            sharing_consented: true,
            connection_status: ConnectionStatusWire::Connected,
            consent_version: Some("v1".into()),
            consented_at: Some(1_785_000_000),
            revoked_at: None,
            invited_email: None,
            wrong_dob_attempts: 0,
            locked_at: None,
            delivery_failed_at: None,
            delivery_error: None,
        }),
    };
    store.driver().insert_row(
        Patients::Table,
        Patients::COLS,
        [
            ws_id.into(),
            display_name.into(),
            mrn.into(),
            birth_date.into(),
            serde_json::to_string(&workspace)?.into(),
        ],
    )?;
    Ok(())
}
