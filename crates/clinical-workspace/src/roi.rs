//! Automated HIPAA Release of Information (ROI) & Right of Access engine.
//!
//! Supports both:
//! - **45 CFR § 164.524 (HIPAA Right of Access):** Patient-directed request for
//!   electronic copies (FHIR R4 / C-CDA / PDF) with federal 30-calendar-day
//!   fulfillment deadlines and cost-based fee caps.
//! - **45 CFR § 164.508 (Third-Party Authorization):** Patient-authorized
//!   release directly from a prior clinic to a new doctor, specialist, or chosen
//!   recipient.
//!
//! Models signing, validation, batch dispatch, 30-day statutory tracking,
//! automated Day-15/Day-30 follow-up reminders, and revocation using explicit
//! Rust types (fail-closed before signing or dispatching).

use serde::{Deserialize, Serialize};

use crate::ehr_portal::{
    html_escape, percent_encode, sha256_hex, PortalOrganization, PortalVendor,
};
use crate::{PatientChart, PART2_REDISCLOSURE_NOTICE_LONG};

/// Alias for [`RoiLegalBasis`] representing the statutory authorization kind.
pub type RoiAuthorizationKind = RoiLegalBasis;

/// Federal legal basis for a Release of Information request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoiLegalBasis {
    /// Patient-directed right to electronic copies under 45 CFR § 164.524.
    #[default]
    HipaaRightOfAccess164524,
    /// Patient authorization to disclose records to another person/clinician under 45 CFR § 164.508.
    ThirdPartyAuthorization164508,
    /// Substance Use Disorder (SUD) program written consent under 42 CFR Part 2 (§ 2.31 & § 2.32).
    Part2SudAuthorization42Cfr2,
}

impl RoiLegalBasis {
    pub const fn label(self) -> &'static str {
        match self {
            Self::HipaaRightOfAccess164524 => "HIPAA Right of Access (45 CFR § 164.524)",
            Self::ThirdPartyAuthorization164508 => {
                "Third-Party ROI Authorization (45 CFR § 164.508)"
            }
            Self::Part2SudAuthorization42Cfr2 => {
                "42 CFR Part 2 SUD Authorization (42 CFR § 2.31 & § 2.32)"
            }
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::HipaaRightOfAccess164524 => "Right of Access (§ 164.524)",
            Self::ThirdPartyAuthorization164508 => "Third-Party ROI (§ 164.508)",
            Self::Part2SudAuthorization42Cfr2 => "42 CFR Part 2 SUD (§ 2.31)",
        }
    }

    pub const fn is_part2_sud(self) -> bool {
        matches!(self, Self::Part2SudAuthorization42Cfr2)
    }

    pub const fn legal_summary(self) -> &'static str {
        match self {
            Self::HipaaRightOfAccess164524 => {
                "Under 45 CFR § 164.524(b)(2) & (c)(2), covered entities must provide electronic copies in the requested form/format within 30 calendar days, subject to reasonable cost-based fee limits."
            }
            Self::ThirdPartyAuthorization164508 => {
                "Under 45 CFR § 164.508, this signed authorization directs the releasing clinic to disclose the selected records to the named recipient until the expiration date or earlier revocation."
            }
            Self::Part2SudAuthorization42Cfr2 => {
                "Under 42 CFR § 2.31 (2024 Final Rule, 89 FR 12626), this written consent authorizes disclosure of Substance Use Disorder (SUD) records strictly as described below and must be accompanied by the statutory 42 CFR § 2.32 Prohibition on Redisclosure notice and a copy or clear explanation of the consent scope (§ 2.32(b)). Records disclosed for Treatment, Payment, or Health Care Operations (TPO) may be redisclosed by a HIPAA covered entity or business associate as permitted under HIPAA, except for use in proceedings against the patient without specific consent or court order. Refusal to sign will not condition treatment, payment, enrollment, or eligibility for benefits."
            }
        }
    }
}

/// Automated or manual channel used to send an ROI packet to a clinic's Health
/// Information Management (HIM / Medical Records) department.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoiDeliveryChannel {
    #[default]
    DirectEmail,
    EFaxAutomation,
    PatientPortalRequest,
    PrintableMailPacket,
}

impl RoiDeliveryChannel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DirectEmail => "Automated HIM Email",
            Self::EFaxAutomation => "Automated e-Fax Gateway",
            Self::PatientPortalRequest => "Patient Portal ROI Upload",
            Self::PrintableMailPacket => "Printable Signed PDF / Mail",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::DirectEmail,
            Self::EFaxAutomation,
            Self::PatientPortalRequest,
            Self::PrintableMailPacket,
        ]
    }
}

/// Specific clinical record category requested on an ROI form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoiRecordCategory {
    LabResults,
    ImagingReports,
    ProgressAndConsultNotes,
    MedicationAndAllergyLists,
    CgmAndDeviceData,
    BillingAndClaims,
    /// Requires explicit opt-in acknowledgment per 45 CFR § 164.508(a)(2).
    SensitivePsychotherapyAndBehavioral,
    /// Substance Use Disorder (SUD) program records protected under 42 CFR Part 2.
    Part2SubstanceUseDisorder,
    /// Separately maintained SUD counseling notes requiring separate written consent (42 CFR § 2.31(b)).
    Part2SudCounselingNotes,
}

impl RoiRecordCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LabResults => "Laboratory & Pathology Results",
            Self::ImagingReports => "Imaging Reports & Radiology",
            Self::ProgressAndConsultNotes => "Progress, Consult & Discharge Notes",
            Self::MedicationAndAllergyLists => "Medications, Problems & Allergy Lists",
            Self::CgmAndDeviceData => "CGM, Wearable & Remote Monitoring Summaries",
            Self::BillingAndClaims => "Itemized Billing & Claims Statements",
            Self::SensitivePsychotherapyAndBehavioral => {
                "Psychotherapy / Behavioral Health Notes (explicit opt-in)"
            }
            Self::Part2SubstanceUseDisorder => {
                "42 CFR Part 2 Substance Use Disorder (SUD) Program Records"
            }
            Self::Part2SudCounselingNotes => {
                "42 CFR § 2.31(b) SUD Counseling Notes (separate written consent)"
            }
        }
    }

    pub const fn is_sensitive_opt_in(self) -> bool {
        matches!(
            self,
            Self::SensitivePsychotherapyAndBehavioral
                | Self::Part2SubstanceUseDisorder
                | Self::Part2SudCounselingNotes
        )
    }

    pub const fn is_part2_sud(self) -> bool {
        matches!(
            self,
            Self::Part2SubstanceUseDisorder | Self::Part2SudCounselingNotes
        )
    }

    pub const fn standard_default() -> &'static [Self] {
        &[
            Self::LabResults,
            Self::ImagingReports,
            Self::ProgressAndConsultNotes,
            Self::MedicationAndAllergyLists,
        ]
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::LabResults,
            Self::ImagingReports,
            Self::ProgressAndConsultNotes,
            Self::MedicationAndAllergyLists,
            Self::CgmAndDeviceData,
            Self::BillingAndClaims,
            Self::SensitivePsychotherapyAndBehavioral,
            Self::Part2SubstanceUseDisorder,
            Self::Part2SudCounselingNotes,
        ]
    }
}

/// Time window requested from the releasing provider.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoiDateRange {
    Last12Months,
    #[default]
    Last3Years,
    AllAvailableHistory,
    Custom {
        from: String,
        to: String,
    },
}

impl RoiDateRange {
    pub fn label(&self) -> String {
        match self {
            Self::Last12Months => "Last 12 months".into(),
            Self::Last3Years => "Last 3 years".into(),
            Self::AllAvailableHistory => "All available records".into(),
            Self::Custom { from, to } => format!("{from} through {to}"),
        }
    }
}

/// Requested electronic delivery format under 45 CFR § 164.524(c)(2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoiElectronicFormat {
    #[default]
    FhirR4Json,
    CcdaXml,
    PdfElectronic,
    AnyStandardElectronic,
}

impl RoiElectronicFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FhirR4Json => "FHIR R4 JSON / USCDI Bundle",
            Self::CcdaXml => "C-CDA XML Continuity Document",
            Self::PdfElectronic => "Searchable Electronic PDF",
            Self::AnyStandardElectronic => "Any Standard Electronic Format (FHIR / C-CDA / PDF)",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::FhirR4Json,
            Self::CcdaXml,
            Self::PdfElectronic,
            Self::AnyStandardElectronic,
        ]
    }
}

/// Destination receiving the released records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoiRecipient {
    PatientSelf {
        delivery_destination: String,
    },
    ThirdParty {
        recipient_name: String,
        relationship_or_clinic: String,
        destination: String,
    },
}

impl RoiRecipient {
    pub fn summary(&self) -> String {
        match self {
            Self::PatientSelf {
                delivery_destination,
            } => format!("Patient directly ({delivery_destination})"),
            Self::ThirdParty {
                recipient_name,
                relationship_or_clinic,
                destination,
            } => format!("{recipient_name} — {relationship_or_clinic} ({destination})"),
        }
    }
}

/// Typed lifecycle state of one ROI authorization request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RoiStatus {
    #[default]
    Draft,
    SignedReadyToSend {
        signed_by: String,
        signed_at: String,
    },
    Dispatched {
        reference_id: String,
        channel: RoiDeliveryChannel,
        dispatched_at: String,
        due_by_30d: String,
        transport_note: String,
    },
    Acknowledged {
        reference_id: String,
        acknowledged_at: String,
    },
    Fulfilled {
        reference_id: String,
        fulfilled_at: String,
    },
    Revoked {
        revoked_at: String,
    },
}

impl RoiStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Draft => "Draft · unsigned",
            Self::SignedReadyToSend { .. } => "Signed · queued for send",
            Self::Dispatched { .. } => "Dispatched · 30d clock active",
            Self::Acknowledged { .. } => "Acknowledged by HIM",
            Self::Fulfilled { .. } => "Records received",
            Self::Revoked { .. } => "Revoked by patient",
        }
    }

    pub fn reference_id(&self) -> Option<&str> {
        match self {
            Self::Dispatched { reference_id, .. }
            | Self::Acknowledged { reference_id, .. }
            | Self::Fulfilled { reference_id, .. } => Some(reference_id.as_str()),
            _ => None,
        }
    }
}

/// Complete Release of Information (ROI) request targeting one clinic or
/// hospital Health Information Management (HIM) office.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoiRequest {
    pub id: String,
    pub patient_id: String,
    pub patient_name: String,
    pub patient_birth_date: String,
    pub patient_contact_email: String,
    pub clinic_name: String,
    pub clinic_him_email: String,
    pub clinic_him_fax: String,
    pub clinic_address: String,
    pub clinic_portal_vendor: Option<PortalVendor>,
    pub legal_basis: RoiLegalBasis,
    pub delivery_channel: RoiDeliveryChannel,
    pub electronic_format: RoiElectronicFormat,
    pub date_range: RoiDateRange,
    pub categories: Vec<RoiRecordCategory>,
    pub recipient: RoiRecipient,
    pub expiration_date: String,
    pub purpose_note: String,
    pub attestation_accepted: bool,
    pub status: RoiStatus,
    /// 42 CFR § 2.31(a)(2): Specific disclosing SUD program / class authorized to disclose.
    #[serde(default)]
    pub part2_disclosing_program: String,
    /// 42 CFR § 2.31(a)(3): Specific and meaningful description of SUD information to disclose.
    #[serde(default)]
    pub part2_specific_description: String,
    /// 42 CFR § 2.31(a)(10): Acknowledgment of TPO redisclosure potential and refusal consequences.
    #[serde(default)]
    pub part2_tpo_redisclosure_acknowledged: bool,
    /// 42 CFR § 2.31(b): Separate explicit written consent for separately maintained SUD counseling notes.
    #[serde(default)]
    pub part2_separate_counseling_notes_consent: bool,
}

impl RoiRequest {
    /// Whether this ROI involves 42 CFR Part 2 Substance Use Disorder records
    /// and therefore requires full § 2.31 elements + § 2.32 redisclosure notice.
    #[must_use]
    pub fn requires_part2_consent(&self) -> bool {
        self.legal_basis.is_part2_sud()
            || self.categories.iter().any(|c| c.is_part2_sud())
            || self
                .clinic_portal_vendor
                .is_some_and(PortalVendor::is_part2_sensitive)
    }

    /// Whether this ROI requests separately maintained SUD counseling notes
    /// (42 CFR § 2.31(b)).
    #[must_use]
    pub fn includes_part2_counseling_notes(&self) -> bool {
        self.categories
            .contains(&RoiRecordCategory::Part2SudCounselingNotes)
    }
}

/// Outcome of dispatching a single [`RoiRequest`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoiDispatchItemOutcome {
    pub request_id: String,
    pub clinic_name: String,
    pub dispatched: bool,
    pub reference_id: Option<String>,
    pub detail: String,
}

/// Outcome of a batch ROI dispatch run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoiBatchDispatchReport {
    pub total_queued: usize,
    pub dispatched_count: usize,
    pub skipped_or_failed_count: usize,
    pub items: Vec<RoiDispatchItemOutcome>,
}

/// Validates that `req` has all mandatory fields before the patient signs or
/// dispatches it (fail-closed).
pub fn validate_roi_request(req: &RoiRequest) -> Result<(), String> {
    if req.patient_name.trim().is_empty() {
        return Err("Patient full name is required on a HIPAA ROI request.".into());
    }
    if req.clinic_name.trim().is_empty() {
        return Err("Releasing clinic or hospital name is required.".into());
    }
    if req.categories.is_empty() {
        return Err("Select at least one clinical record category to request.".into());
    }
    match req.delivery_channel {
        RoiDeliveryChannel::DirectEmail => {
            let email = req.clinic_him_email.trim();
            if email.is_empty() || !email.contains('@') {
                return Err(format!(
                    "{}: a valid HIM / Medical Records email is required for Automated Email dispatch.",
                    req.clinic_name
                ));
            }
        }
        RoiDeliveryChannel::EFaxAutomation => {
            let digits = req
                .clinic_him_fax
                .chars()
                .filter(char::is_ascii_digit)
                .count();
            if digits < 10 {
                return Err(format!(
                    "{}: a 10+ digit HIM fax number is required for Automated e-Fax dispatch.",
                    req.clinic_name
                ));
            }
        }
        RoiDeliveryChannel::PatientPortalRequest | RoiDeliveryChannel::PrintableMailPacket => {}
    }
    match &req.recipient {
        RoiRecipient::PatientSelf {
            delivery_destination,
        } => {
            if delivery_destination.trim().is_empty() {
                return Err(
                    "Patient delivery destination (email or Dabney.moe vault) is required.".into(),
                );
            }
        }
        RoiRecipient::ThirdParty {
            recipient_name,
            destination,
            ..
        } => {
            if recipient_name.trim().is_empty() || destination.trim().is_empty() {
                return Err(
                    "Third-party recipient name and destination contact are required.".into(),
                );
            }
        }
    }

    if req.requires_part2_consent() {
        if !req.legal_basis.is_part2_sud() {
            return Err(
                "42 CFR Part 2 SUD records require a 42 CFR Part 2 SUD Authorization (§ 2.31); a general HIPAA authorization is not sufficient."
                    .into(),
            );
        }
        if req.part2_disclosing_program.trim().is_empty() && req.clinic_name.trim().is_empty() {
            return Err(
                "42 CFR § 2.31(a)(2) requires identifying the specific disclosing SUD program."
                    .into(),
            );
        }
        if req.part2_specific_description.trim().is_empty() {
            return Err(
                "42 CFR § 2.31(a)(3) requires a specific and meaningful description of the SUD information to be disclosed."
                    .into(),
            );
        }
        if req.purpose_note.trim().is_empty() {
            return Err(
                "42 CFR § 2.31(a)(5) requires stating the purpose of use or disclosure.".into(),
            );
        }
        if req.expiration_date.trim().is_empty() {
            return Err(
                "42 CFR § 2.31(a)(7) requires an expiration date, event, or condition (e.g., YYYY-MM-DD or 'end of treatment')."
                    .into(),
            );
        }
        if req.includes_part2_counseling_notes() && !req.part2_separate_counseling_notes_consent {
            return Err(
                "42 CFR § 2.31(b): Separately maintained SUD counseling notes require separate explicit written consent."
                    .into(),
            );
        }
    }
    Ok(())
}

/// Applies the patient's electronic signature and explicit attestation, moving a
/// validated `Draft` request into `SignedReadyToSend`.
pub fn sign_roi_request(
    req: &mut RoiRequest,
    signature_name: &str,
    signed_at: &str,
) -> Result<(), String> {
    validate_roi_request(req)?;
    let sig = signature_name.trim();
    if sig.len() < 2 {
        return Err("Type your full name to electronically sign the ROI authorization.".into());
    }
    if !req.attestation_accepted {
        return Err(
            "Confirm the explicit patient authorization attestation checkbox before signing."
                .into(),
        );
    }
    if req.requires_part2_consent() && !req.part2_tpo_redisclosure_acknowledged {
        return Err(
            "42 CFR § 2.31(a)(10): Confirm the Part 2 redisclosure & refusal-consequences notice before signing."
                .into(),
        );
    }
    req.status = RoiStatus::SignedReadyToSend {
        signed_by: sig.to_string(),
        signed_at: signed_at.to_string(),
    };
    Ok(())
}

/// Marks a single signed ROI request as dispatched via the specified transport
/// mode and assigns a deterministic reference ID + 30-day HIPAA deadline.
pub fn dispatch_roi_request(
    req: &mut RoiRequest,
    dispatched_at: &str,
    due_by_30d: &str,
    transport_note: &str,
) -> Result<String, String> {
    validate_roi_request(req)?;
    if !matches!(req.status, RoiStatus::SignedReadyToSend { .. }) {
        return Err(format!(
            "{}: request must be signed by the patient before dispatch (current status: {}).",
            req.clinic_name,
            req.status.label()
        ));
    }
    let digest = sha256_hex(
        format!(
            "{}|{}|{}|{}",
            req.id, req.clinic_name, req.patient_name, dispatched_at
        )
        .as_bytes(),
    );
    let reference_id = format!("ROI-{}", &digest[..10].to_ascii_uppercase());
    req.status = RoiStatus::Dispatched {
        reference_id: reference_id.clone(),
        channel: req.delivery_channel,
        dispatched_at: dispatched_at.to_string(),
        due_by_30d: due_by_30d.to_string(),
        transport_note: transport_note.to_string(),
    };
    Ok(reference_id)
}

/// Automates batch dispatch across every `SignedReadyToSend` request in
/// `requests`, preserving already-dispatched or unsigned requests untouched.
pub fn dispatch_roi_batch(
    requests: &mut [RoiRequest],
    dispatched_at: &str,
    due_by_30d: &str,
    transport_note: &str,
) -> RoiBatchDispatchReport {
    let mut report = RoiBatchDispatchReport {
        total_queued: 0,
        dispatched_count: 0,
        skipped_or_failed_count: 0,
        items: Vec::new(),
    };

    for req in requests.iter_mut() {
        if !matches!(req.status, RoiStatus::SignedReadyToSend { .. }) {
            continue;
        }
        report.total_queued += 1;
        match dispatch_roi_request(req, dispatched_at, due_by_30d, transport_note) {
            Ok(ref_id) => {
                report.dispatched_count += 1;
                report.items.push(RoiDispatchItemOutcome {
                    request_id: req.id.clone(),
                    clinic_name: req.clinic_name.clone(),
                    dispatched: true,
                    reference_id: Some(ref_id.clone()),
                    detail: format!(
                        "Dispatched via {} (Ref {ref_id} · 30-day statutory due date {due_by_30d}).",
                        req.delivery_channel.label()
                    ),
                });
            }
            Err(err) => {
                report.skipped_or_failed_count += 1;
                report.items.push(RoiDispatchItemOutcome {
                    request_id: req.id.clone(),
                    clinic_name: req.clinic_name.clone(),
                    dispatched: false,
                    reference_id: None,
                    detail: err,
                });
            }
        }
    }

    report
}

/// Revokes an ROI request per the patient's federal right to revoke at any time
/// under 45 CFR § 164.508(b)(5).
pub fn revoke_roi_request(req: &mut RoiRequest, revoked_at: &str) {
    req.status = RoiStatus::Revoked {
        revoked_at: revoked_at.to_string(),
    };
}

/// Seeds realistic zero-PHI starter ROI requests for a synthetic patient so
/// visitors can immediately inspect, sign, and test batch ROI automation.
#[must_use]
pub fn default_roi_requests_for_patient(patient: &PatientChart) -> Vec<RoiRequest> {
    vec![
        RoiRequest {
            id: format!("roi-{}-1", patient.id),
            patient_id: patient.id.clone(),
            patient_name: patient.display_name.clone(),
            patient_birth_date: patient.birth_date.clone(),
            patient_contact_email: "patient-demo@dabney-demo.invalid".into(),
            clinic_name: "Stanford Health Care — Health Information Management".into(),
            clinic_him_email: "ROI@stanfordhealthcare.org".into(),
            clinic_him_fax: "+1-650-725-9821".into(),
            clinic_address: "300 Pasteur Dr, Stanford, CA 94305".into(),
            clinic_portal_vendor: Some(PortalVendor::EpicMyChart),
            legal_basis: RoiLegalBasis::HipaaRightOfAccess164524,
            delivery_channel: RoiDeliveryChannel::DirectEmail,
            electronic_format: RoiElectronicFormat::FhirR4Json,
            date_range: RoiDateRange::Last3Years,
            categories: RoiRecordCategory::standard_default().to_vec(),
            recipient: RoiRecipient::PatientSelf {
                delivery_destination: "patient-demo@dabney-demo.invalid (Dabney.moe Portable Passport)".into(),
            },
            expiration_date: "2027-02-24".into(),
            purpose_note: "Patient-directed continuity of care so I do not have to repeat labs and specialist history.".into(),
            attestation_accepted: true,
            status: RoiStatus::SignedReadyToSend {
                signed_by: patient.display_name.clone(),
                signed_at: "2026-11-26T04:00:00Z".into(),
            },
            part2_disclosing_program: String::new(),
            part2_specific_description: String::new(),
            part2_tpo_redisclosure_acknowledged: false,
            part2_separate_counseling_notes_consent: false,
        },
        RoiRequest {
            id: format!("roi-{}-2", patient.id),
            patient_id: patient.id.clone(),
            patient_name: patient.display_name.clone(),
            patient_birth_date: patient.birth_date.clone(),
            patient_contact_email: "patient-demo@dabney-demo.invalid".into(),
            clinic_name: "Cedars-Sinai Medical Center — Release of Information".into(),
            clinic_him_email: "releaseofinfo@cshs.org".into(),
            clinic_him_fax: "+1-310-423-0113".into(),
            clinic_address: "8700 Beverly Blvd, Los Angeles, CA 90048".into(),
            clinic_portal_vendor: Some(PortalVendor::EpicMyChart),
            legal_basis: RoiLegalBasis::ThirdPartyAuthorization164508,
            delivery_channel: RoiDeliveryChannel::EFaxAutomation,
            electronic_format: RoiElectronicFormat::PdfElectronic,
            date_range: RoiDateRange::Last12Months,
            categories: vec![
                RoiRecordCategory::LabResults,
                RoiRecordCategory::ImagingReports,
                RoiRecordCategory::ProgressAndConsultNotes,
            ],
            recipient: RoiRecipient::ThirdParty {
                recipient_name: "Dr. Maya Patel (Demo)".into(),
                relationship_or_clinic: "Dabney.moe Synthetic Clinic".into(),
                destination: "records@dabney-demo.invalid".into(),
            },
            expiration_date: "2027-02-24".into(),
            purpose_note: "Share recent imaging and consult notes ahead of upcoming follow-up visit.".into(),
            attestation_accepted: false,
            status: RoiStatus::Draft,
            part2_disclosing_program: String::new(),
            part2_specific_description: String::new(),
            part2_tpo_redisclosure_acknowledged: false,
            part2_separate_counseling_notes_consent: false,
        },
    ]
}

/// Builds a pre-filled ROI fallback draft targeting any directory
/// [`PortalOrganization`] — especially non-FHIR or behavioral-health EHRs such
/// as KipuEMR (automatically configured with 42 CFR Part 2 § 2.31 fields),
/// SimplePractice, and Hint Health.
#[must_use]
pub fn roi_draft_for_portal_org(
    patient: &PatientChart,
    org: &PortalOrganization,
    draft_id: &str,
) -> RoiRequest {
    let is_part2 = org.vendor.is_part2_sensitive() || org.transport.part2_sensitive;
    let delivery_channel = if org.him_email.is_some() {
        RoiDeliveryChannel::DirectEmail
    } else if org.him_fax.is_some() {
        RoiDeliveryChannel::EFaxAutomation
    } else {
        RoiDeliveryChannel::PrintableMailPacket
    };
    let electronic_format = if org.supports_smart_on_fhir() {
        RoiElectronicFormat::FhirR4Json
    } else {
        RoiElectronicFormat::CcdaXml
    };
    let categories = if is_part2 {
        vec![
            RoiRecordCategory::Part2SubstanceUseDisorder,
            RoiRecordCategory::MedicationAndAllergyLists,
            RoiRecordCategory::LabResults,
        ]
    } else {
        RoiRecordCategory::standard_default().to_vec()
    };

    RoiRequest {
        id: draft_id.to_string(),
        patient_id: patient.id.clone(),
        patient_name: patient.display_name.clone(),
        patient_birth_date: patient.birth_date.clone(),
        patient_contact_email: "patient-demo@dabney-demo.invalid".into(),
        clinic_name: format!("{} — Health Information Management", org.name),
        clinic_him_email: org.him_email.clone().unwrap_or_default(),
        clinic_him_fax: org.him_fax.clone().unwrap_or_default(),
        clinic_address: org.him_address.clone().unwrap_or_default(),
        clinic_portal_vendor: Some(org.vendor),
        legal_basis: if is_part2 {
            RoiLegalBasis::Part2SudAuthorization42Cfr2
        } else {
            RoiLegalBasis::HipaaRightOfAccess164524
        },
        delivery_channel,
        electronic_format,
        date_range: RoiDateRange::Last3Years,
        categories,
        recipient: RoiRecipient::PatientSelf {
            delivery_destination: "patient-demo@dabney-demo.invalid (Dabney.moe Portable Passport)"
                .into(),
        },
        expiration_date: "2027-02-24".into(),
        purpose_note: if is_part2 {
            "At the request of the patient for continuity of care (42 CFR Part 2 consent).".into()
        } else {
            format!(
                "Patient-directed electronic copy fallback for {} ({}).",
                org.vendor.label(),
                org.transport.primary_capability.badge_label()
            )
        },
        attestation_accepted: false,
        status: RoiStatus::Draft,
        part2_disclosing_program: if is_part2 {
            org.name.clone()
        } else {
            String::new()
        },
        part2_specific_description: if is_part2 {
            "Outpatient SUD admission summary, medication orders, vitals, lab results, and care plan (excluding separate SUD counseling notes).".into()
        } else {
            String::new()
        },
        part2_tpo_redisclosure_acknowledged: false,
        part2_separate_counseling_notes_consent: false,
    }
}

/// Renders a plain-text HIPAA ROI cover letter ready for HIM email, e-fax cover
/// sheet, or patient portal message submission.
#[must_use]
pub fn render_roi_cover_text(req: &RoiRequest) -> String {
    let cats = req
        .categories
        .iter()
        .map(|c| format!("- {}", c.label()))
        .collect::<Vec<_>>()
        .join("\n");
    let sig_line = match &req.status {
        RoiStatus::SignedReadyToSend {
            signed_by,
            signed_at,
        } => format!("Electronically signed by: {signed_by} on {signed_at}"),
        RoiStatus::Dispatched {
            reference_id,
            dispatched_at,
            due_by_30d,
            ..
        } => format!(
            "Dispatched ref {reference_id} on {dispatched_at} (30-day statutory deadline: {due_by_30d})"
        ),
        RoiStatus::Acknowledged {
            reference_id,
            acknowledged_at,
        } => format!("Ref {reference_id} — Acknowledged by HIM on {acknowledged_at}"),
        RoiStatus::Fulfilled {
            reference_id,
            fulfilled_at,
        } => format!("Ref {reference_id} — Fulfilled on {fulfilled_at}"),
        RoiStatus::Revoked { revoked_at } => format!("REVOKED by patient on {revoked_at}"),
        RoiStatus::Draft => "Status: DRAFT (not yet signed by patient)".into(),
    };

    let part2_block = if req.requires_part2_consent() {
        let program = if req.part2_disclosing_program.trim().is_empty() {
            req.clinic_name.as_str()
        } else {
            req.part2_disclosing_program.as_str()
        };
        let counseling_line = if req.includes_part2_counseling_notes() {
            "SEPARATE SUD COUNSELING NOTES CONSENT (42 CFR § 2.31(b)): Expressly granted separately."
        } else {
            "SEPARATE SUD COUNSELING NOTES (42 CFR § 2.31(b)): EXCLUDED (not covered by this consent)."
        };
        format!(
            "\n42 CFR PART 2 CONSENT SCOPE (§ 2.31 & § 2.32(b))\n\
             - Disclosing Program: {program}\n\
             - Specific SUD Information Description: {}\n\
             - {counseling_line}\n\n\
             42 CFR § 2.32(a) PROHIBITION ON REDISCLOSURE:\n\
             {PART2_REDISCLOSURE_NOTICE_LONG}\n",
            req.part2_specific_description
        )
    } else {
        String::new()
    };

    format!(
        "TO: Health Information Management / Medical Records — {clinic}\n\
         CHANNEL: {channel}\n\
         LEGAL BASIS: {basis}\n\n\
         PATIENT INFORMATION\n\
         - Full Name: {patient}\n\
         - Date of Birth: {dob}\n\
         - Contact Email: {contact}\n\n\
         RECORDS REQUESTED ({range})\n\
         {cats}\n\n\
         REQUESTED ELECTRONIC FORMAT: {format}\n\
         SEND RECORDS TO: {recipient}\n\
         PURPOSE: {purpose}\n\
         AUTHORIZATION EXPIRATION: {expires}\n\
         {part2_block}\n\
         STATUTORY NOTICE:\n\
         {legal_notice}\n\
         I understand I may revoke this authorization in writing at any time, except to the extent action has already been taken in reliance on it.\n\n\
         {sig_line}\n\
         (SYNTHETIC ZERO-PHI DEMO FORM when generated from Dabney.moe demo profiles.)",
        clinic = req.clinic_name,
        channel = req.delivery_channel.label(),
        basis = req.legal_basis.label(),
        patient = req.patient_name,
        dob = req.patient_birth_date,
        contact = req.patient_contact_email,
        range = req.date_range.label(),
        cats = cats,
        format = req.electronic_format.label(),
        recipient = req.recipient.summary(),
        purpose = req.purpose_note,
        expires = req.expiration_date,
        part2_block = part2_block,
        legal_notice = req.legal_basis.legal_summary(),
        sig_line = sig_line,
    )
}

/// Builds a pre-filled `mailto:` URI targeting the clinic's HIM email address.
#[must_use]
pub fn build_roi_mailto_uri(req: &RoiRequest) -> Option<String> {
    let email = req.clinic_him_email.trim();
    if email.is_empty() || !email.contains('@') {
        return None;
    }
    let subject = format!(
        "{} — {} (DOB {})",
        req.legal_basis.short_label(),
        req.patient_name,
        req.patient_birth_date
    );
    let body = render_roi_cover_text(req);
    Some(format!(
        "mailto:{email}?subject={}&body={}",
        percent_encode(&subject),
        percent_encode(&body)
    ))
}

/// Generates an automated Day-15 polite status check or Day-30 statutory
/// 45 CFR § 164.524(b)(2) follow-up reminder letter for a dispatched ROI request.
#[must_use]
pub fn render_roi_followup_letter(req: &RoiRequest, days_elapsed: u32) -> String {
    let ref_id = req.status.reference_id().unwrap_or("UNASSIGNED");
    if days_elapsed >= 30 {
        format!(
            "SUBJECT: 30-Day Statutory Follow-Up — HIPAA Right of Access Request ({ref_id})\n\n\
             Dear Health Information Management Team at {},\n\n\
             It has been {days_elapsed} calendar days since I submitted my signed {} request (Reference {ref_id}) for {} (DOB {}).\n\
             Under 45 CFR § 164.524(b)(2), covered entities are required to act on a patient's request for access within 30 calendar days (or provide a single written extension notice stating the reason and completion date).\n\n\
             Please transmit the requested {} records ({}) to {} as soon as possible, or confirm receipt and expected delivery date by reply.\n\n\
             Thank you,\n{}",
            req.clinic_name,
            req.legal_basis.label(),
            req.patient_name,
            req.patient_birth_date,
            req.electronic_format.label(),
            req.date_range.label(),
            req.recipient.summary(),
            req.patient_name,
        )
    } else {
        format!(
            "SUBJECT: Status Check (Day {days_elapsed}) — Medical Records Request ({ref_id})\n\n\
             Dear Health Information Management Team at {},\n\n\
             I am following up politely on my signed {} request (Reference {ref_id}) submitted {days_elapsed} day(s) ago for {} (DOB {}).\n\
             Could you please confirm receipt and let me know when the {} copies ({}) will be delivered to {}?\n\n\
             Thank you for your help,\n{}",
            req.clinic_name,
            req.legal_basis.label(),
            req.patient_name,
            req.patient_birth_date,
            req.electronic_format.label(),
            req.date_range.label(),
            req.recipient.summary(),
            req.patient_name,
        )
    }
}

/// Renders a complete, print-to-PDF ready standalone HTML HIPAA Release of
/// Information / Right of Access authorization form (XSS-escaped).
#[must_use]
pub fn render_roi_form_html(req: &RoiRequest) -> String {
    let mut checkboxes = String::new();
    for cat in RoiRecordCategory::all() {
        let checked = req.categories.contains(cat);
        let mark = if checked { "&#9745;" } else { "&#9744;" };
        let extra = if cat.is_sensitive_opt_in() {
            " class=\"sensitive\""
        } else {
            ""
        };
        checkboxes.push_str(&format!(
            "<li{extra}><span class=\"chk\">{mark}</span> {}</li>",
            html_escape(cat.label())
        ));
    }

    let sig_block = match &req.status {
        RoiStatus::SignedReadyToSend {
            signed_by,
            signed_at,
        } => format!(
            "<strong>Electronically Signed:</strong> {} &nbsp;|&nbsp; <strong>Timestamp:</strong> {}",
            html_escape(signed_by),
            html_escape(signed_at)
        ),
        RoiStatus::Dispatched {
            reference_id,
            dispatched_at,
            due_by_30d,
            ..
        } => format!(
            "<strong>Signed &amp; Dispatched</strong> · Ref: <code>{}</code> · Sent: {} · 30-Day Federal Due Date: <strong>{}</strong>",
            html_escape(reference_id),
            html_escape(dispatched_at),
            html_escape(due_by_30d)
        ),
        RoiStatus::Revoked { revoked_at } => {
            format!(
                "<strong style=\"color:#b91c1c\">REVOKED BY PATIENT</strong> on {}",
                html_escape(revoked_at)
            )
        }
        other => format!("<strong>Status:</strong> {}", html_escape(other.label())),
    };

    let part2_html_block = if req.requires_part2_consent() {
        let program = if req.part2_disclosing_program.trim().is_empty() {
            req.clinic_name.as_str()
        } else {
            req.part2_disclosing_program.as_str()
        };
        let counseling_status = if req.includes_part2_counseling_notes() {
            "Separately consented under 42 CFR § 2.31(b)."
        } else {
            "Excluded — separate written consent required under 42 CFR § 2.31(b)."
        };
        format!(
            r#"<div class="part2-box">
    <h2>42 CFR Part 2 Written Consent Scope (§ 2.31 &amp; § 2.32(b))</h2>
    <div><strong>Disclosing Program (§ 2.31(a)(2)):</strong> {program}</div>
    <div><strong>Specific SUD Information (§ 2.31(a)(3)):</strong> {desc}</div>
    <div><strong>SUD Counseling Notes (§ 2.31(b)):</strong> {counseling}</div>
    <p class="part2-notice"><strong>42 CFR § 2.32(a) Prohibition on Redisclosure:</strong> {notice}</p>
  </div>"#,
            program = html_escape(program),
            desc = html_escape(&req.part2_specific_description),
            counseling = html_escape(counseling_status),
            notice = html_escape(PART2_REDISCLOSURE_NOTICE_LONG),
        )
    } else {
        String::new()
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>HIPAA Release of Information — {clinic}</title>
<style>
* {{ box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; color: #0f172a; margin: 0; padding: 24px; line-height: 1.5; }}
.sheet {{ max-width: 820px; margin: 0 auto; border: 1px solid #cbd5e1; border-radius: 10px; padding: 24px; }}
header {{ border-bottom: 2px solid #01081b; padding-bottom: 12px; margin-bottom: 16px; }}
h1 {{ margin: 4px 0 0 0; font-size: 20px; }}
h2 {{ font-size: 12px; text-transform: uppercase; letter-spacing: 0.08em; color: #0f766e; margin: 16px 0 6px 0; }}
.eyebrow {{ font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em; color: #0f766e; margin: 0; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 12px; }}
.card {{ border: 1px solid #e2e8f0; border-radius: 8px; padding: 12px; font-size: 13px; }}
ul.checks {{ list-style: none; padding: 0; margin: 0; font-size: 13px; }}
ul.checks li {{ padding: 4px 0; }}
ul.checks li.sensitive {{ border-top: 1px dashed #cbd5e1; margin-top: 6px; padding-top: 8px; font-weight: 600; color: #7c2d12; }}
.chk {{ font-size: 15px; margin-right: 6px; }}
.legal {{ background: #f8fafc; border: 1px solid #cbd5e1; border-radius: 8px; padding: 12px; font-size: 12px; color: #334155; margin-top: 14px; }}
.part2-box {{ background: #fff7ed; border: 2px solid #c2410c; border-radius: 8px; padding: 14px; font-size: 12px; color: #431407; margin-top: 14px; }}
.part2-box h2 {{ color: #9a3412; margin-top: 0; }}
.part2-notice {{ margin: 10px 0 0 0; padding-top: 8px; border-top: 1px solid #fdba74; font-size: 11px; line-height: 1.5; }}
.sig {{ border-top: 2px solid #01081b; margin-top: 16px; padding-top: 12px; font-size: 13px; }}
@media print {{ body {{ padding: 0; }} .sheet {{ border: 0; padding: 0; }} }}
</style>
</head>
<body>
<div class="sheet">
  <header>
    <p class="eyebrow">Dabney.moe Automated Release of Information · {basis}</p>
    <h1>Authorization &amp; Request for Electronic Medical Records</h1>
  </header>

  <div class="grid">
    <div class="card">
      <h2>Patient</h2>
      <div><strong>Name:</strong> {patient}</div>
      <div><strong>DOB:</strong> {dob}</div>
      <div><strong>Contact:</strong> {contact}</div>
    </div>
    <div class="card">
      <h2>Releasing Provider (HIM)</h2>
      <div><strong>Clinic / Hospital:</strong> {clinic}</div>
      <div><strong>HIM Email:</strong> {him_email}</div>
      <div><strong>HIM Fax:</strong> {him_fax}</div>
      <div><strong>Address:</strong> {him_addr}</div>
    </div>
  </div>

  <h2>Records Requested ({date_range})</h2>
  <div class="card">
    <ul class="checks">{checkboxes}</ul>
  </div>

  <div class="grid" style="margin-top:12px;">
    <div class="card">
      <h2>Delivery Format &amp; Channel</h2>
      <div><strong>Format:</strong> {format}</div>
      <div><strong>Channel:</strong> {channel}</div>
      <div><strong>Expires:</strong> {expires}</div>
    </div>
    <div class="card">
      <h2>Authorized Recipient</h2>
      <div>{recipient}</div>
      <div style="margin-top:6px;"><strong>Purpose:</strong> {purpose}</div>
    </div>
  </div>

  {part2_html_block}

  <div class="legal">
    <strong>Federal Right &amp; Revocation Notice:</strong> {legal_summary}
    I understand that I have the right to revoke this authorization in writing at any time (except to the extent the releasing entity has already acted on it), and that treatment, payment, enrollment, or eligibility for benefits may not be conditioned on signing this authorization.
  </div>

  <div class="sig">{sig_block}</div>
</div>
</body>
</html>"#,
        basis = html_escape(req.legal_basis.label()),
        patient = html_escape(&req.patient_name),
        dob = html_escape(&req.patient_birth_date),
        contact = html_escape(&req.patient_contact_email),
        clinic = html_escape(&req.clinic_name),
        him_email = html_escape(&req.clinic_him_email),
        him_fax = html_escape(&req.clinic_him_fax),
        him_addr = html_escape(&req.clinic_address),
        date_range = html_escape(&req.date_range.label()),
        checkboxes = checkboxes,
        format = html_escape(req.electronic_format.label()),
        channel = html_escape(req.delivery_channel.label()),
        expires = html_escape(&req.expiration_date),
        recipient = html_escape(&req.recipient.summary()),
        purpose = html_escape(&req.purpose_note),
        part2_html_block = part2_html_block,
        legal_summary = html_escape(req.legal_basis.legal_summary()),
        sig_block = sig_block,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find_patient;

    #[test]
    fn roi_signing_and_batch_dispatch_fail_closed_when_incomplete() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let mut reqs = default_roi_requests_for_patient(&nadia);
        assert_eq!(reqs.len(), 2);

        // Request 1 is already signed; Request 2 is an unsigned draft.
        // Dispatching without signing Request 2 dispatches ONLY Request 1.
        let report = dispatch_roi_batch(
            &mut reqs,
            "2026-11-26T04:30:00Z",
            "2026-12-26",
            "Local sandbox dispatch",
        );
        assert_eq!(report.total_queued, 1);
        assert_eq!(report.dispatched_count, 1);
        assert!(matches!(reqs[0].status, RoiStatus::Dispatched { .. }));
        assert!(matches!(reqs[1].status, RoiStatus::Draft));

        // Signing Request 2 without attestation checked must fail closed.
        assert!(sign_roi_request(&mut reqs[1], "Nadia Kulkarni", "2026-11-26T04:31:00Z").is_err());

        // Checking attestation + signing succeeds and dispatches cleanly.
        reqs[1].attestation_accepted = true;
        sign_roi_request(&mut reqs[1], "Nadia Kulkarni", "2026-11-26T04:31:00Z").unwrap();
        let report2 = dispatch_roi_batch(
            &mut reqs,
            "2026-11-26T04:32:00Z",
            "2026-12-26",
            "Automated e-fax gateway",
        );
        assert_eq!(report2.dispatched_count, 1);
        assert!(reqs[1].status.reference_id().is_some());
    }

    #[test]
    fn roi_renders_escaped_html_mailto_and_day30_followup() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let mut reqs = default_roi_requests_for_patient(&nadia);
        reqs[0].clinic_name = "Clinic <script>x</script>".into();
        let ref_id = dispatch_roi_request(
            &mut reqs[0],
            "2026-11-26T04:30:00Z",
            "2026-12-26",
            "Sandbox email",
        )
        .unwrap();

        let html = render_roi_form_html(&reqs[0]);
        assert!(!html.contains("<script>x</script>"));
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(html.contains(&ref_id));

        let mailto = build_roi_mailto_uri(&reqs[0]).expect("mailto");
        assert!(mailto.starts_with("mailto:ROI@stanfordhealthcare.org?subject="));

        let day15 = render_roi_followup_letter(&reqs[0], 15);
        assert!(day15.contains("Day 15"));
        let day30 = render_roi_followup_letter(&reqs[0], 30);
        assert!(day30.contains("45 CFR § 164.524(b)(2)"));
        assert!(day30.contains(&ref_id));
    }

    #[test]
    fn part2_sud_roi_enforces_231_elements_232_notice_and_separate_counseling_consent() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let kipu = crate::ehr_portal::find_portal_organization("org-kipu-sandbox").expect("kipu");
        let mut req = roi_draft_for_portal_org(&nadia, &kipu, "roi-kipu-1");
        assert!(req.requires_part2_consent());
        assert_eq!(req.legal_basis, RoiLegalBasis::Part2SudAuthorization42Cfr2);

        // General HIPAA authorization is rejected for Part 2 SUD records.
        let mut bad_basis = req.clone();
        bad_basis.legal_basis = RoiLegalBasis::HipaaRightOfAccess164524;
        assert!(validate_roi_request(&bad_basis).is_err());

        // Missing specific description (§ 2.31(a)(3)) fails closed.
        let mut missing_desc = req.clone();
        missing_desc.part2_specific_description.clear();
        assert!(validate_roi_request(&missing_desc).is_err());

        // Requesting SUD counseling notes without separate consent (§ 2.31(b)) fails closed.
        let mut with_notes = req.clone();
        with_notes
            .categories
            .push(RoiRecordCategory::Part2SudCounselingNotes);
        assert!(validate_roi_request(&with_notes).is_err());
        with_notes.part2_separate_counseling_notes_consent = true;
        assert!(validate_roi_request(&with_notes).is_ok());

        // Signing without § 2.31(a)(10) TPO redisclosure acknowledgment fails closed.
        req.attestation_accepted = true;
        assert!(sign_roi_request(&mut req, "Nadia Kulkarni", "2026-11-26T04:31:00Z").is_err());

        req.part2_tpo_redisclosure_acknowledged = true;
        sign_roi_request(&mut req, "Nadia Kulkarni", "2026-11-26T04:31:00Z").unwrap();

        let cover = render_roi_cover_text(&req);
        assert!(cover.contains("42 CFR § 2.32(a) PROHIBITION ON REDISCLOSURE"));
        assert!(cover.contains("protected by Federal confidentiality rules (42 CFR part 2)"));
        let html = render_roi_form_html(&req);
        assert!(html.contains("42 CFR Part 2 Written Consent Scope"));
        assert!(html.contains("42 CFR part 2"));
    }
}
