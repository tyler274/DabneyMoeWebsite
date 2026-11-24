//! Patient-authored, clinician-reviewed collaborative Wellness & Safety Plan model
//! and structured chart record builder for Duet Connected Care.

use crate::store::DemoSourceRecord;
use physician_wire::{CitedFieldWire, CitedTextPartWire};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Source label assigned to patient-shared Wellness & Safety Plan records.
pub const WELLNESS_PLAN_SOURCE_LABEL: &str = "Patient-Shared Wellness & Safety Plan (via Duet)";

/// Validation errors returned when constructing a [`WellnessSafetyPlan`] record.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum WellnessSafetyPlanValidationError {
    /// A required header or contact summary field was blank.
    #[error("wellness & safety plan field `{0}` cannot be empty")]
    MissingField(&'static str),
    /// A required section list had no non-empty entries.
    #[error("wellness & safety plan section `{0}` must contain at least one non-empty entry")]
    EmptySection(&'static str),
    /// Non-police crisis resources must include the verified `988` Lifeline resource.
    #[error("wellness & safety plan crisis resources must include 988 Suicide & Crisis Lifeline")]
    Missing988CrisisResource,
}

/// Patient-authored, clinician-reviewed collaborative Wellness & Safety Plan shared voluntarily
/// through Duet Connected Care.
///
/// Strictly reference-only: supports collaborative outpatient review of early-warning signs,
/// patient-authored coping steps, consented personal and care-team contacts, and verified
/// non-police crisis resources (`988`) without surveillance, automated patient outreach, or
/// law-enforcement dispatch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WellnessSafetyPlan {
    /// Chart display title for this Wellness & Safety Plan.
    pub title: String,
    /// ISO date (`YYYY-MM-DD`) the patient authored or last updated the plan in Duet.
    pub authored_at: String,
    /// ISO date (`YYYY-MM-DD`) the plan was last reviewed with the clinician.
    pub reviewed_at: String,
    /// Concise sleep early-warning cue for structured citation tables (e.g. `"< 5.5"`).
    pub early_warning_sleep_cue: String,
    /// Concise standardized scale cue for structured citation tables (e.g. `"ASRM >= 6"`).
    pub early_warning_scale_cue: String,
    /// Patient-identified personal early-warning signs.
    pub early_warning_signs: Vec<String>,
    /// Ordered patient-authored self-regulation and coping steps.
    pub coping_steps: Vec<String>,
    /// Concise summary of the patient-consented trusted support contact.
    pub trusted_support_summary: String,
    /// Detailed trusted personal support contacts (consented by patient).
    pub trusted_support_contacts: Vec<String>,
    /// Concise care-team contact summary for structured citation tables.
    pub care_team_summary: String,
    /// Care-team clinic hours or turnaround guidance.
    pub care_team_hours: String,
    /// Clinician and care-team contact instructions.
    pub care_team_contacts: Vec<String>,
    /// Verified non-police crisis resources (must include `988`).
    pub crisis_resources: Vec<String>,
}

impl WellnessSafetyPlan {
    /// Validates required metadata, non-empty collaborative sections, and `988` crisis coverage.
    pub fn validate(&self) -> Result<(), WellnessSafetyPlanValidationError> {
        for (name, val) in [
            ("title", self.title.as_str()),
            ("authored_at", self.authored_at.as_str()),
            ("reviewed_at", self.reviewed_at.as_str()),
            (
                "early_warning_sleep_cue",
                self.early_warning_sleep_cue.as_str(),
            ),
            (
                "early_warning_scale_cue",
                self.early_warning_scale_cue.as_str(),
            ),
            (
                "trusted_support_summary",
                self.trusted_support_summary.as_str(),
            ),
            ("care_team_summary", self.care_team_summary.as_str()),
            ("care_team_hours", self.care_team_hours.as_str()),
        ] {
            if val.trim().is_empty() {
                return Err(WellnessSafetyPlanValidationError::MissingField(name));
            }
        }
        for (name, items) in [
            ("early_warning_signs", self.early_warning_signs.as_slice()),
            ("coping_steps", self.coping_steps.as_slice()),
            (
                "trusted_support_contacts",
                self.trusted_support_contacts.as_slice(),
            ),
            ("care_team_contacts", self.care_team_contacts.as_slice()),
            ("crisis_resources", self.crisis_resources.as_slice()),
        ] {
            if !items.iter().any(|s| !s.trim().is_empty()) {
                return Err(WellnessSafetyPlanValidationError::EmptySection(name));
            }
        }
        if !self
            .crisis_resources
            .iter()
            .any(|line| line.contains("988"))
        {
            return Err(WellnessSafetyPlanValidationError::Missing988CrisisResource);
        }
        Ok(())
    }

    /// Renders compact, scannable [`CitedFieldWire`] rows for `CitedValuesTable` and `SourcePane`.
    pub fn to_cited_fields(
        &self,
    ) -> Result<Vec<CitedFieldWire>, WellnessSafetyPlanValidationError> {
        self.validate()?;
        Ok(vec![
            CitedFieldWire {
                key: "safety_plan_status".into(),
                name: "Plan Review Status".into(),
                value: "Clinician-reviewed".into(),
                unit: None,
                range: Some("Annual / as-needed".into()),
                status: Some("Reviewed".into()),
            },
            CitedFieldWire {
                key: "early_warning_sleep".into(),
                name: "Early-Warning Sleep Cue".into(),
                value: self.early_warning_sleep_cue.trim().to_owned(),
                unit: Some("h/night".into()),
                range: Some("Patient-agreed cue".into()),
                status: None,
            },
            CitedFieldWire {
                key: "early_warning_scale".into(),
                name: "Early-Warning Scale Cue".into(),
                value: self.early_warning_scale_cue.trim().to_owned(),
                unit: None,
                range: Some("Patient-agreed cue".into()),
                status: None,
            },
            CitedFieldWire {
                key: "trusted_support_contact".into(),
                name: "Trusted Support Contact".into(),
                value: self.trusted_support_summary.trim().to_owned(),
                unit: None,
                range: Some("Patient-consented".into()),
                status: Some("Consented".into()),
            },
            CitedFieldWire {
                key: "care_team_contact".into(),
                name: "Care-Team Contact".into(),
                value: self.care_team_summary.trim().to_owned(),
                unit: None,
                range: Some(self.care_team_hours.trim().to_owned()),
                status: Some("Active".into()),
            },
            CitedFieldWire {
                key: "crisis_resource_988".into(),
                name: "Non-Police Crisis Support".into(),
                value: "Call or text 988".into(),
                unit: None,
                range: Some("24/7 Suicide & Crisis Lifeline".into()),
                status: Some("Verified".into()),
            },
        ])
    }

    /// Builds a structured `group = "forms"` [`DemoSourceRecord`] with exact `body` /
    /// `readable_parts` parity and compact single-line JSON `raw_content`.
    pub fn to_source_record(
        &self,
        record_id: String,
        workspace_id: &str,
    ) -> Result<DemoSourceRecord, WellnessSafetyPlanValidationError> {
        let fields = self.to_cited_fields()?;

        let consent_text = format!(
            "Patient-authored in Duet on {} and clinician-reviewed on {} with voluntary sharing consent.\n\
             Collaborative outpatient reference only: not continuous monitoring, not automated outreach, and never law-enforcement dispatch.",
            self.authored_at.trim(),
            self.reviewed_at.trim(),
        );
        let warning_text = join_bullets(&self.early_warning_signs);
        let coping_text = join_numbered(&self.coping_steps);
        let contacts_text = format!(
            "{}\n{}",
            join_bullets(&self.trusted_support_contacts),
            join_bullets(&self.care_team_contacts),
        );
        let crisis_text = join_bullets(&self.crisis_resources);

        let readable_parts = vec![
            CitedTextPartWire {
                label: Some("Plan Review & Consent".into()),
                text: consent_text,
            },
            CitedTextPartWire {
                label: Some("Personal Early-Warning Signs".into()),
                text: warning_text,
            },
            CitedTextPartWire {
                label: Some("Patient-Authored Coping Steps".into()),
                text: coping_text,
            },
            CitedTextPartWire {
                label: Some("Trusted Personal & Care-Team Contacts".into()),
                text: contacts_text,
            },
            CitedTextPartWire {
                label: Some("Verified Non-Police Crisis Resources".into()),
                text: crisis_text,
            },
        ];

        // Join `readable_parts` directly so every line/sentence in `body` appears verbatim
        // in `readable_parts` for clickable `[1]`/`[N]` passage citations.
        let body = readable_parts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let raw_content = json!({
            "source": "duet_wellness_safety_plan",
            "plan": self,
        })
        .to_string();

        Ok(DemoSourceRecord {
            id: record_id,
            workspace_id: workspace_id.to_owned(),
            group: "forms".into(),
            title: self.title.trim().to_owned(),
            recorded_at: self.reviewed_at.trim().to_owned(),
            body,
            source_label: WELLNESS_PLAN_SOURCE_LABEL.into(),
            readable_parts,
            fields,
            record_status: "final".into(),
            raw_content: Some(raw_content),
        })
    }
}

fn join_bullets(items: &[String]) -> String {
    items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn join_numbered(items: &[String]) -> String {
    items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(idx, s)| format!("{}. {s}", idx + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> WellnessSafetyPlan {
        WellnessSafetyPlan {
            title: "Clinician-Reviewed Wellness & Safety Plan".into(),
            authored_at: "2026-08-14".into(),
            reviewed_at: "2026-08-18".into(),
            early_warning_sleep_cue: "< 5.5".into(),
            early_warning_scale_cue: "ASRM >= 6".into(),
            early_warning_signs: vec![
                "Sleep under 5.5 hours for two or more nights in a row.".into(),
                "ASRM score 6 or higher with racing plans or reduced need for sleep.".into(),
            ],
            coping_steps: vec![
                "Begin 22:00 screen-free wind-down and dim-light routine.".into(),
                "Take a 10-minute walk and follow with 4-7-8 paced breathing.".into(),
            ],
            trusted_support_summary: "Rohan Patel (Spouse) · +1-415-555-0184".into(),
            trusted_support_contacts: vec![
                "Trusted support: Rohan Patel (Spouse), +1-415-555-0184 (rohan.patel@example.test); patient-consented check-in partner.".into(),
            ],
            care_team_summary: "Dr. Hannah Alvarez (Clinic)".into(),
            care_team_hours: "Mon–Fri 08:00–17:00".into(),
            care_team_contacts: vec![
                "Primary & collaborative behavioral clinician: Dr. Hannah Alvarez, +1-415-555-0100 (Mon–Fri 08:00–17:00; non-urgent portal messages answered within 1 business day).".into(),
            ],
            crisis_resources: vec![
                "988 Suicide & Crisis Lifeline: call or text 988 (24/7 free, confidential, non-police crisis counseling).".into(),
                "Crisis Text Line: text HOME to 741741 (24/7 confidential text support).".into(),
            ],
        }
    }

    #[test]
    fn builds_structured_forms_record_with_exact_body_readable_parts_parity() {
        let plan = sample_plan();
        let rec = plan
            .to_source_record("rec-test-wellness".into(), "unit-wellness-asrm-1")
            .expect("valid wellness plan");
        assert_eq!(rec.group, "forms");
        assert_eq!(rec.record_status, "final");
        assert_eq!(rec.source_label, WELLNESS_PLAN_SOURCE_LABEL);
        assert_eq!(rec.readable_parts.len(), 5);
        assert_eq!(rec.fields.len(), 6);
        assert!(
            rec.fields
                .iter()
                .any(|f| f.key == "crisis_resource_988" && f.value.contains("988"))
        );
        let raw = rec.raw_content.as_deref().expect("compact raw_content");
        assert!(!raw.contains('\n'));
        assert!(serde_json::from_str::<serde_json::Value>(raw).is_ok());

        for line in rec.body.lines().map(str::trim).filter(|l| !l.is_empty()) {
            assert!(
                rec.readable_parts.iter().any(|p| p.text.contains(line)),
                "every body line must appear verbatim in readable_parts: {line:?}"
            );
        }
    }

    #[test]
    fn rejects_missing_988_or_empty_sections() {
        let mut no_988 = sample_plan();
        no_988.crisis_resources = vec!["Local peer warm line: +1-415-555-0199.".into()];
        assert_eq!(
            no_988.validate(),
            Err(WellnessSafetyPlanValidationError::Missing988CrisisResource)
        );

        let mut empty_coping = sample_plan();
        empty_coping.coping_steps.clear();
        assert_eq!(
            empty_coping.validate(),
            Err(WellnessSafetyPlanValidationError::EmptySection(
                "coping_steps"
            ))
        );
    }
}
