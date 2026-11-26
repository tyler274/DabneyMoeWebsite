//! Portable, patient-owned shareable medical summary file ("Health Passport").
//!
//! Lets a patient persist a single verified file (`dabney.health_passport.v1`)
//! to their device, cloud drive, or encrypted vault and share it with any
//! doctor, specialist, ER, caregiver, or person of their choosing—without
//! needing a shared EHR account and without ever repeating their medical story.
//!
//! Exports four complementary formats from one typed source of truth:
//! 1. Canonical re-importable JSON (`dabney.health_passport.v1`) with SHA-256 integrity hash
//! 2. Standards-compliant HL7 FHIR R4 International Patient Summary (IPS) Bundle JSON
//! 3. Self-contained offline printable HTML Health Brief (print-to-PDF ready)
//! 4. Quick-paste Markdown summary (portal messages, email, Web Share)
//!
//! Hard privacy invariant: private device-only notes (`personal_notes`) are
//! never copied into an exported Health Passport.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ehr_portal::{
    html_escape, sha256_hex, AllergyEntry, LinkedPortalConnection, PortalVendor,
};
use crate::{ChartRecord, Medication, PatientChart, PatientDemoState, Problem};

/// Canonical version identifier embedded in every persisted Health Passport JSON file.
pub const HEALTH_PASSPORT_SCHEMA: &str = "dabney.health_passport.v1";

/// Recipient-tailored view mode for a persisted or shared Health Passport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassportAudience {
    /// Structured clinical visit brief for a primary doctor or specialist.
    #[default]
    DoctorOrSpecialist,
    /// Concise high-contrast 1-page card for ER / urgent care / paramedics.
    EmergencyCard,
    /// Plain-language overview for a family member, caregiver, or anyone else.
    CaregiverOrAnyone,
    /// Complete portable backup including all chart records (still excludes private notes).
    FullPortableBackup,
}

impl PassportAudience {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DoctorOrSpecialist => "Doctor / Specialist Visit Brief",
            Self::EmergencyCard => "Emergency / Urgent Care Card",
            Self::CaregiverOrAnyone => "Caregiver / Anyone Summary",
            Self::FullPortableBackup => "Full Portable Record Backup",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::DoctorOrSpecialist => "Doctor / Specialist",
            Self::EmergencyCard => "Emergency Card",
            Self::CaregiverOrAnyone => "Family / Anyone",
            Self::FullPortableBackup => "Full Backup",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::DoctorOrSpecialist => {
                "Selected records, medications, allergies, conditions, visit note, and questions for a clinic visit."
            }
            Self::EmergencyCard => {
                "High-priority medications, allergies, active conditions, and latest key readings for urgent care."
            }
            Self::CaregiverOrAnyone => {
                "Plain-language health summary so a caregiver, friend, or new provider understands your care quickly."
            }
            Self::FullPortableBackup => {
                "All connected portal and chart records packaged into one portable file you can re-open on any device."
            }
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::DoctorOrSpecialist,
            Self::EmergencyCard,
            Self::CaregiverOrAnyone,
            Self::FullPortableBackup,
        ]
    }
}

/// Compact provenance summary of one linked doctor portal included in a passport.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportPortalSummary {
    pub organization_id: String,
    pub organization_name: String,
    pub vendor: PortalVendor,
    pub last_synced_at: String,
    pub imported_record_count: usize,
}

/// Persisted, shareable, integrity-checked patient health file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthPassport {
    pub schema_version: String,
    pub passport_id: String,
    pub generated_at: String,
    pub audience: PassportAudience,
    pub is_synthetic_demo: bool,
    pub owner_name: String,
    pub birth_date: String,
    pub identifier: String,
    pub primary_focus: String,
    pub summary_headline: String,
    pub allergies: Vec<AllergyEntry>,
    pub problems: Vec<Problem>,
    pub medications: Vec<Medication>,
    pub linked_portals: Vec<PassportPortalSummary>,
    pub records: Vec<ChartRecord>,
    pub visit_note: String,
    pub visit_questions: Vec<String>,
    pub emergency_note: String,
    pub excluded_record_count: usize,
    pub excluded_private_note_count: usize,
    pub integrity_sha256: String,
}

impl HealthPassport {
    /// Recomputes the canonical SHA-256 digest over all clinical & provenance
    /// fields (excluding `integrity_sha256` itself) and returns whether it
    /// matches the stored digest.
    #[must_use]
    pub fn verify_integrity(&self) -> bool {
        let expected = compute_passport_digest(self);
        self.integrity_sha256 == expected
    }

    /// Safe filename slug derived from the owner's display label.
    #[must_use]
    pub fn file_slug(&self) -> String {
        let mut slug = String::new();
        let mut prev_dash = false;
        for ch in self.owner_name.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !prev_dash && !slug.is_empty() {
                slug.push('-');
                prev_dash = true;
            }
        }
        let clean = slug.trim_end_matches('-');
        if clean.is_empty() {
            "patient".into()
        } else {
            clean.chars().take(32).collect()
        }
    }
}

fn compute_passport_digest(passport: &HealthPassport) -> String {
    let mut normalized = passport.clone();
    normalized.integrity_sha256.clear();
    let canonical = serde_json::to_string(&normalized).unwrap_or_default();
    sha256_hex(canonical.as_bytes())
}

/// Builds a verified [`HealthPassport`] strictly from the patient's chart,
/// explicit record selections, allergies, and linked portal provenance.
///
/// Guarantees:
/// - `state.personal_notes` are NEVER included in the output passport.
/// - For non-backup presets, only records explicitly selected in
///   `state.selected_record_ids` appear in `records`.
#[must_use]
pub fn build_health_passport(
    patient: &PatientChart,
    state: &PatientDemoState,
    allergies: &[AllergyEntry],
    linked_portals: &[LinkedPortalConnection],
    audience: PassportAudience,
    emergency_note: &str,
    generated_at: &str,
) -> HealthPassport {
    let records: Vec<ChartRecord> = match audience {
        PassportAudience::FullPortableBackup => patient.records.clone(),
        PassportAudience::EmergencyCard => {
            // Prioritize explicitly selected records, capped to concise urgent set (up to 4).
            patient
                .records
                .iter()
                .filter(|rec| state.selected_record_ids.iter().any(|id| id == &rec.id))
                .take(4)
                .cloned()
                .collect()
        }
        PassportAudience::DoctorOrSpecialist | PassportAudience::CaregiverOrAnyone => patient
            .records
            .iter()
            .filter(|rec| state.selected_record_ids.iter().any(|id| id == &rec.id))
            .cloned()
            .collect(),
    };

    let excluded_record_count = patient.records.len().saturating_sub(records.len());
    let portal_summaries: Vec<PassportPortalSummary> = linked_portals
        .iter()
        .filter_map(|conn| match &conn.status {
            crate::ehr_portal::PortalConnectionStatus::Connected {
                last_synced_at,
                imported_record_count,
                ..
            } => Some(PassportPortalSummary {
                organization_id: conn.organization_id.clone(),
                organization_name: conn.organization_name.clone(),
                vendor: conn.vendor,
                last_synced_at: last_synced_at.clone(),
                imported_record_count: *imported_record_count,
            }),
            _ => None,
        })
        .collect();

    let mut passport = HealthPassport {
        schema_version: HEALTH_PASSPORT_SCHEMA.to_string(),
        passport_id: format!("hp-{}-{}", patient.id, audience_slug(audience)),
        generated_at: generated_at.to_string(),
        audience,
        is_synthetic_demo: true,
        owner_name: patient.display_name.clone(),
        birth_date: patient.birth_date.clone(),
        identifier: patient.mrn.clone(),
        primary_focus: patient.primary_focus.clone(),
        summary_headline: patient.summary.clone(),
        allergies: allergies.to_vec(),
        problems: patient.problems.clone(),
        medications: patient.medications.clone(),
        linked_portals: portal_summaries,
        records,
        visit_note: state.visit_note.trim().to_string(),
        visit_questions: state
            .visit_questions
            .iter()
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
            .collect(),
        emergency_note: emergency_note.trim().to_string(),
        excluded_record_count,
        excluded_private_note_count: state.personal_notes.len(),
        integrity_sha256: String::new(),
    };
    passport.integrity_sha256 = compute_passport_digest(&passport);
    passport
}

const fn audience_slug(audience: PassportAudience) -> &'static str {
    match audience {
        PassportAudience::DoctorOrSpecialist => "visit",
        PassportAudience::EmergencyCard => "emergency",
        PassportAudience::CaregiverOrAnyone => "caregiver",
        PassportAudience::FullPortableBackup => "backup",
    }
}

/// Parses and integrity-checks a persisted `dabney.health_passport.v1` JSON file.
pub fn parse_health_passport_json(raw: &str) -> Result<HealthPassport, String> {
    let passport: HealthPassport = serde_json::from_str(raw)
        .map_err(|err| format!("Could not parse Dabney Health Passport JSON: {err}"))?;
    if passport.schema_version != HEALTH_PASSPORT_SCHEMA {
        return Err(format!(
            "Unsupported Health Passport schema `{}` (expected `{HEALTH_PASSPORT_SCHEMA}`).",
            passport.schema_version
        ));
    }
    if !passport.verify_integrity() {
        return Err(
            "Health Passport SHA-256 integrity check failed: file was modified or truncated."
                .into(),
        );
    }
    Ok(passport)
}

/// Exports a [`HealthPassport`] as a standards-compliant HL7 FHIR R4
/// International Patient Summary (IPS) document `Bundle` (`LOINC 60591-5`).
#[must_use]
pub fn render_passport_fhir_ips_bundle(passport: &HealthPassport) -> Value {
    let patient_ref = format!("Patient/{}", passport.passport_id);
    let mut entries = Vec::new();
    let mut section_entries = Vec::new();

    entries.push(json!({
        "fullUrl": format!("urn:uuid:{}", passport.passport_id),
        "resource": {
            "resourceType": "Patient",
            "id": passport.passport_id,
            "name": [{ "text": passport.owner_name }],
            "birthDate": passport.birth_date,
            "identifier": [{ "value": passport.identifier }]
        }
    }));

    for (idx, alg) in passport.allergies.iter().enumerate() {
        let id = format!("allergy-{idx}");
        section_entries.push(json!({ "reference": format!("AllergyIntolerance/{id}") }));
        entries.push(json!({
            "resource": {
                "resourceType": "AllergyIntolerance",
                "id": id,
                "patient": { "reference": patient_ref },
                "criticality": alg.criticality.to_ascii_lowercase(),
                "recordedDate": alg.recorded_at,
                "code": { "text": alg.substance },
                "reaction": [{ "manifestation": [{ "text": alg.reaction }] }]
            }
        }));
    }

    for (idx, prob) in passport.problems.iter().enumerate() {
        let id = format!("condition-{idx}");
        section_entries.push(json!({ "reference": format!("Condition/{id}") }));
        entries.push(json!({
            "resource": {
                "resourceType": "Condition",
                "id": id,
                "subject": { "reference": patient_ref },
                "clinicalStatus": { "text": prob.status },
                "code": {
                    "coding": [{ "code": prob.code, "display": prob.label }],
                    "text": prob.label
                }
            }
        }));
    }

    for (idx, med) in passport.medications.iter().enumerate() {
        let id = format!("med-{idx}");
        section_entries.push(json!({ "reference": format!("MedicationStatement/{id}") }));
        entries.push(json!({
            "resource": {
                "resourceType": "MedicationStatement",
                "id": id,
                "status": "active",
                "subject": { "reference": patient_ref },
                "medicationCodeableConcept": { "text": med.name },
                "dosage": [{ "text": format!("{} ({})", med.dose, med.schedule) }],
                "reasonCode": [{ "text": med.indication }]
            }
        }));
    }

    for rec in &passport.records {
        let id = rec.id.clone();
        section_entries.push(json!({ "reference": format!("Observation/{id}") }));
        let value_text = if rec.key_values.is_empty() {
            rec.body.clone()
        } else {
            let kv = rec
                .key_values
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("; ");
            format!("{kv} — {}", rec.body)
        };
        entries.push(json!({
            "resource": {
                "resourceType": "Observation",
                "id": id,
                "status": "final",
                "subject": { "reference": patient_ref },
                "code": { "text": rec.title },
                "effectiveDateTime": rec.recorded_at,
                "valueString": value_text
            }
        }));
    }

    // Prepend the IPS Composition document header (LOINC 60591-5).
    let composition = json!({
        "resource": {
            "resourceType": "Composition",
            "id": format!("comp-{}", passport.passport_id),
            "status": "final",
            "type": {
                "coding": [{
                    "system": "http://loinc.org",
                    "code": "60591-5",
                    "display": "Patient summary Document"
                }],
                "text": format!("Dabney.moe Portable Health Passport ({})", passport.audience.label())
            },
            "subject": { "reference": patient_ref },
            "date": passport.generated_at,
            "title": format!("Portable Health Passport — {}", passport.owner_name),
            "section": [{
                "title": "Patient-Selected Clinical Summary",
                "entry": section_entries
            }]
        }
    });
    entries.insert(0, composition);

    json!({
        "resourceType": "Bundle",
        "id": passport.passport_id,
        "type": "document",
        "timestamp": passport.generated_at,
        "meta": {
            "profile": ["http://hl7.org/fhir/uv/ips/StructureDefinition/Bundle-uv-ips"]
        },
        "entry": entries
    })
}

/// Renders a crisp plain-text / Markdown health summary suitable for copying
/// into a portal message, email, text, or Web Share sheet.
#[must_use]
pub fn render_passport_markdown(passport: &HealthPassport) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str(&format!(
        "# Dabney.moe Portable Health Passport — {}\n",
        passport.owner_name
    ));
    out.push_str(&format!(
        "- **Format:** {} (`{}`)\n",
        passport.audience.label(),
        passport.schema_version
    ));
    out.push_str(&format!(
        "- **DOB:** {} | **ID:** {} | **Generated:** {}\n",
        passport.birth_date, passport.identifier, passport.generated_at
    ));
    if passport.is_synthetic_demo {
        out.push_str("- **Notice:** SYNTHETIC DEMO FILE (Zero PHI).\n");
    }
    out.push_str(&format!("\n## Summary\n{}\n", passport.summary_headline));

    if !passport.emergency_note.is_empty() {
        out.push_str(&format!(
            "\n## Emergency / Key Instructions\n{}\n",
            passport.emergency_note
        ));
    }

    out.push_str("\n## Allergies & Intolerances\n");
    if passport.allergies.is_empty() {
        out.push_str("- None documented in this file.\n");
    } else {
        for a in &passport.allergies {
            out.push_str(&format!(
                "- **{}** ({}): {} — *{} ({})*\n",
                a.substance, a.criticality, a.reaction, a.source_label, a.recorded_at
            ));
        }
    }

    out.push_str("\n## Active Medications\n");
    if passport.medications.is_empty() {
        out.push_str("- None listed.\n");
    } else {
        for m in &passport.medications {
            out.push_str(&format!(
                "- **{} {}** — {} ({})\n",
                m.name, m.dose, m.schedule, m.indication
            ));
        }
    }

    out.push_str("\n## Active Conditions & Focus Areas\n");
    for p in &passport.problems {
        out.push_str(&format!("- [{}] {} ({})\n", p.code, p.label, p.status));
    }

    if !passport.visit_note.is_empty() || !passport.visit_questions.is_empty() {
        out.push_str("\n## Patient Note & Visit Questions\n");
        if !passport.visit_note.is_empty() {
            out.push_str(&format!("{}\n\n", passport.visit_note));
        }
        for (i, q) in passport.visit_questions.iter().enumerate() {
            out.push_str(&format!("{}. {q}\n", i + 1));
        }
    }

    out.push_str(&format!(
        "\n## Selected Clinical Records ({} included, {} excluded, {} private notes omitted)\n",
        passport.records.len(),
        passport.excluded_record_count,
        passport.excluded_private_note_count
    ));
    for r in &passport.records {
        out.push_str(&format!(
            "- **[{} · {} · {}]** {} — {}\n",
            r.recorded_at,
            r.kind.label(),
            r.source_label,
            r.title,
            r.body
        ));
    }

    if !passport.linked_portals.is_empty() {
        out.push_str("\n## Connected Doctor Portals (Provenance)\n");
        for lp in &passport.linked_portals {
            out.push_str(&format!(
                "- {} ({}) · synced {} · {} record(s)\n",
                lp.organization_name,
                lp.vendor.label(),
                lp.last_synced_at,
                lp.imported_record_count
            ));
        }
    }

    out.push_str(&format!(
        "\n---\nSHA-256 Integrity: `{}`\nGenerated locally by Dabney.moe (not medical advice; patient-owned portable summary).\n",
        passport.integrity_sha256
    ));
    out
}

/// Renders a standalone, offline, print-to-PDF-ready HTML Health Passport Brief
/// that any doctor, ER clinician, caregiver, or recipient can open in any
/// browser without creating an account.
#[must_use]
pub fn render_passport_html(passport: &HealthPassport) -> String {
    let accent = match passport.audience {
        PassportAudience::EmergencyCard => "#b91c1c",
        PassportAudience::DoctorOrSpecialist => "#0f766e",
        PassportAudience::CaregiverOrAnyone => "#0369a1",
        PassportAudience::FullPortableBackup => "#166534",
    };
    let banner_bg = match passport.audience {
        PassportAudience::EmergencyCard => "#fef2f2",
        _ => "#f0fdfa",
    };

    let mut allergies_html = String::new();
    if passport.allergies.is_empty() {
        allergies_html.push_str("<li>None documented in this portable summary.</li>");
    } else {
        for a in &passport.allergies {
            allergies_html.push_str(&format!(
                "<li><strong>{}</strong> <span class=\"pill\">{}</span> — {} <span class=\"muted\">({} · {})</span></li>",
                html_escape(&a.substance),
                html_escape(&a.criticality),
                html_escape(&a.reaction),
                html_escape(&a.source_label),
                html_escape(&a.recorded_at),
            ));
        }
    }

    let mut meds_html = String::new();
    for m in &passport.medications {
        meds_html.push_str(&format!(
            "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td><td>{}</td></tr>",
            html_escape(&m.name),
            html_escape(&m.dose),
            html_escape(&m.schedule),
            html_escape(&m.indication),
        ));
    }

    let mut probs_html = String::new();
    for p in &passport.problems {
        probs_html.push_str(&format!(
            "<li><code>{}</code> <strong>{}</strong> <span class=\"muted\">({})</span></li>",
            html_escape(&p.code),
            html_escape(&p.label),
            html_escape(&p.status),
        ));
    }

    let mut records_html = String::new();
    for r in &passport.records {
        let mut kvs = String::new();
        for (k, v) in &r.key_values {
            kvs.push_str(&format!(
                "<span class=\"kv\"><strong>{}:</strong> {}</span> ",
                html_escape(k),
                html_escape(v),
            ));
        }
        records_html.push_str(&format!(
            "<article class=\"rec\"><div class=\"rec-head\"><strong>{}</strong><span class=\"muted\">{} · {} · {}</span></div><p>{}</p>{}</article>",
            html_escape(&r.title),
            html_escape(r.kind.label()),
            html_escape(&r.recorded_at),
            html_escape(&r.source_label),
            html_escape(&r.body),
            if kvs.is_empty() {
                String::new()
            } else {
                format!("<div class=\"kvs\">{kvs}</div>")
            }
        ));
    }

    let mut questions_html = String::new();
    for q in &passport.visit_questions {
        questions_html.push_str(&format!("<li>{}</li>", html_escape(q)));
    }

    let emergency_block = if passport.emergency_note.is_empty() {
        String::new()
    } else {
        format!(
            "<section class=\"box warn\"><h2>Emergency &amp; Care Instructions</h2><p>{}</p></section>",
            html_escape(&passport.emergency_note)
        )
    };

    let demo_notice = if passport.is_synthetic_demo {
        "<div class=\"demo-banner\">SYNTHETIC DEMO PASSPORT — ZERO REAL PHI · Fictional sample data for demonstration only.</div>"
    } else {
        ""
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Portable Health Passport — {name}</title>
<style>
:root {{ --accent: {accent}; --bg: {banner_bg}; }}
* {{ box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; color: #0f172a; background: #fff; margin: 0; padding: 24px; line-height: 1.5; }}
.wrap {{ max-width: 860px; margin: 0 auto; }}
header {{ border-bottom: 3px solid var(--accent); padding-bottom: 14px; margin-bottom: 18px; display: flex; justify-content: space-between; align-items: flex-start; gap: 16px; flex-wrap: wrap; }}
h1 {{ margin: 0; font-size: 24px; color: #01081b; }}
h2 {{ font-size: 14px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--accent); margin: 0 0 8px 0; }}
.eyebrow {{ font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.1em; color: var(--accent); margin: 0 0 4px 0; }}
.meta {{ font-size: 12px; color: #475569; }}
.demo-banner {{ background: #ecfdf5; border: 1px solid #10b981; color: #065f46; padding: 8px 12px; border-radius: 8px; font-size: 12px; font-weight: 600; margin-bottom: 16px; }}
.box {{ border: 1px solid #cbd5e1; border-radius: 10px; padding: 14px 16px; margin-bottom: 14px; }}
.box.highlight {{ background: var(--bg); border-color: var(--accent); }}
.box.warn {{ background: #fff7ed; border-color: #ea580c; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 14px; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
th, td {{ text-align: left; padding: 6px 8px; border-bottom: 1px solid #e2e8f0; vertical-align: top; }}
th {{ font-size: 11px; text-transform: uppercase; color: #475569; }}
ul, ol {{ margin: 0; padding-left: 20px; font-size: 13px; }}
li {{ margin-bottom: 4px; }}
.pill {{ display: inline-block; padding: 1px 7px; border-radius: 99px; font-size: 11px; font-weight: 600; background: #fee2e2; color: #991b1b; }}
.muted {{ color: #64748b; font-size: 12px; }}
.rec {{ border-top: 1px solid #e2e8f0; padding-top: 10px; margin-top: 10px; }}
.rec:first-of-type {{ border-top: 0; padding-top: 0; margin-top: 0; }}
.rec-head {{ display: flex; justify-content: space-between; gap: 8px; flex-wrap: wrap; font-size: 13px; }}
.rec p {{ margin: 4px 0 0 0; font-size: 13px; color: #1e293b; }}
.kvs {{ margin-top: 6px; display: flex; flex-wrap: wrap; gap: 6px; }}
.kv {{ background: #f1f5f9; border: 1px solid #cbd5e1; border-radius: 6px; padding: 2px 8px; font-size: 12px; }}
footer {{ margin-top: 20px; padding-top: 12px; border-top: 1px solid #cbd5e1; font-size: 11px; color: #64748b; font-family: ui-monospace, SFMono-Regular, monospace; }}
@media print {{ body {{ padding: 0; }} .box {{ break-inside: avoid; }} }}
</style>
</head>
<body>
<div class="wrap">
{demo_notice}
<header>
  <div>
    <p class="eyebrow">Dabney.moe Portable Health Passport · {audience}</p>
    <h1>{name}</h1>
    <p class="meta">DOB: {dob} · ID: {ident} · Focus: {focus}</p>
  </div>
  <div class="meta" style="text-align:right;">
    <div><strong>Generated:</strong> {generated}</div>
    <div>{included} selected record(s) · {excluded} omitted · {priv_omitted} private note(s) excluded</div>
  </div>
</header>

<section class="box highlight">
  <h2>At-a-Glance Clinical Summary</h2>
  <p style="margin:0;font-size:14px;">{summary}</p>
</section>

{emergency_block}

<div class="grid">
  <section class="box">
    <h2>Allergies &amp; Intolerances</h2>
    <ul>{allergies_html}</ul>
  </section>
  <section class="box">
    <h2>Active Conditions</h2>
    <ul>{probs_html}</ul>
  </section>
</div>

<section class="box">
  <h2>Active Medications</h2>
  <table>
    <thead><tr><th>Medication</th><th>Dose</th><th>Schedule</th><th>Indication</th></tr></thead>
    <tbody>{meds_html}</tbody>
  </table>
</section>

<section class="box">
  <h2>Patient Note &amp; Questions</h2>
  <p style="margin:0 0 8px 0;font-size:13px;">{visit_note}</p>
  <ol>{questions_html}</ol>
</section>

<section class="box">
  <h2>Selected Longitudinal Records ({included})</h2>
  {records_html}
</section>

<footer>
  Schema: {schema} · Integrity SHA-256: {sha} · Patient-controlled portable file (no cloud account required to view or print). Not medical advice.
</footer>
</div>
</body>
</html>"#,
        accent = accent,
        banner_bg = banner_bg,
        demo_notice = demo_notice,
        audience = html_escape(passport.audience.label()),
        name = html_escape(&passport.owner_name),
        dob = html_escape(&passport.birth_date),
        ident = html_escape(&passport.identifier),
        focus = html_escape(&passport.primary_focus),
        generated = html_escape(&passport.generated_at),
        included = passport.records.len(),
        excluded = passport.excluded_record_count,
        priv_omitted = passport.excluded_private_note_count,
        summary = html_escape(&passport.summary_headline),
        emergency_block = emergency_block,
        allergies_html = allergies_html,
        probs_html = probs_html,
        meds_html = meds_html,
        visit_note = html_escape(&passport.visit_note),
        questions_html = questions_html,
        records_html = records_html,
        schema = html_escape(&passport.schema_version),
        sha = html_escape(&passport.integrity_sha256),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ehr_portal::{default_allergies_for_patient, ingest_fhir_r4_bundle};
    use crate::{default_patient_demo_state, find_patient};

    #[test]
    fn passport_roundtrips_json_verifies_sha256_and_never_leaks_private_notes() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let mut state = default_patient_demo_state(&nadia);
        state.personal_notes[0].body = "SUPER-SECRET-PRIVATE-DIARY-ENTRY".into();
        let allergies = default_allergies_for_patient(&nadia.id);

        let passport = build_health_passport(
            &nadia,
            &state,
            &allergies,
            &[],
            PassportAudience::DoctorOrSpecialist,
            "Call partner Alex at 555-0199 in emergency.",
            "2026-11-26T04:20:00Z",
        );

        assert!(passport.verify_integrity());
        assert_eq!(passport.excluded_private_note_count, 1);

        let json_str = serde_json::to_string_pretty(&passport).unwrap();
        assert!(!json_str.contains("SUPER-SECRET-PRIVATE-DIARY-ENTRY"));

        let restored = parse_health_passport_json(&json_str).expect("valid passport");
        assert_eq!(restored, passport);

        // Tampering with the serialized JSON breaks the SHA-256 check.
        let tampered = json_str.replace("Nadia Kulkarni", "Tampered Name");
        assert!(parse_health_passport_json(&tampered).is_err());
    }

    #[test]
    fn passport_exports_valid_fhir_ips_and_escaped_html() {
        let mut nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let mut state = default_patient_demo_state(&nadia);
        state.visit_note = "<script>alert('xss')</script> Check fasting trend".into();
        nadia.display_name = "Nadia <Kulkarni>".into();
        let allergies = default_allergies_for_patient(&nadia.id);

        let passport = build_health_passport(
            &nadia,
            &state,
            &allergies,
            &[],
            PassportAudience::EmergencyCard,
            "Allergic to sulfa",
            "2026-11-26T04:20:00Z",
        );

        let html = render_passport_html(&passport);
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;alert"));
        assert!(html.contains("Emergency / Urgent Care Card"));

        let ips = render_passport_fhir_ips_bundle(&passport);
        assert_eq!(ips["resourceType"], "Bundle");
        assert_eq!(ips["type"], "document");
        let ips_str = serde_json::to_string(&ips).unwrap();
        let reimported = ingest_fhir_r4_bundle("IPS Passport", "ips", &ips_str).unwrap();
        assert!(!reimported.records.is_empty());
        assert!(!reimported.medications.is_empty());
        assert!(!reimported.problems.is_empty());
        assert!(!reimported.allergies.is_empty());
    }
}
