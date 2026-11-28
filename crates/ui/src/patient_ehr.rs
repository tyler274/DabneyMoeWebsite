//! Patient-owned EHR Portal Link-Through (SMART on FHIR R4 + PKCE), Portable
//! Shareable Health Passport File, and Automated HIPAA Release of Information
//! (ROI) UI components for `/clinical/patient`.
//!
//! Runs identically in native SSR, browser WebAssembly, and Tauri desktop/mobile
//! with zero external telemetry and zero cloud PHI egress.

use std::collections::HashMap;

#[cfg(any(feature = "csr", feature = "hydrate"))]
use clinical_workspace::parse_health_passport_json;
use clinical_workspace::{
    build_health_passport_with_options, build_paper_attachment_record, build_roi_mailto_uri,
    build_smart_authorize_url, data_download_uri, dispatch_roi_batch, find_portal_organization,
    ingest_clinical_document, merge_fhir_import_into_chart, parse_pasted_email_or_summary,
    portal_directory, render_passport_fhir_ips_bundle, render_passport_html,
    render_passport_markdown, render_passport_one_page_html, render_roi_cover_text,
    render_roi_followup_letter, render_roi_form_html, revoke_roi_request,
    sandbox_ccda_xml_for_patient, sandbox_fhir_bundle_for_org, sandbox_hl7v2_message_for_patient,
    sandbox_kipu_payload_for_patient, sandbox_smart_health_link_uri, search_portal_directory,
    sign_roi_request, AllergyEntry, ClinicalDocumentFormat, ConfidentialityClass, HealthPassport,
    IntegrationCapability, LinkedPortalConnection, Medication, PassportAudience,
    PassportExportOptions, PatientChart, PatientDemoState, PortalConnectionMode,
    PortalConnectionStatus, PortalOrganization, PortalVendor, Problem, RoiBatchDispatchReport,
    RoiDateRange, RoiDeliveryChannel, RoiElectronicFormat, RoiLegalBasis, RoiRecipient,
    RoiRecordCategory, RoiRequest, RoiStatus, SmartHealthLinkPayload, SmartPkceSession,
};
use leptos::prelude::*;

use crate::i18n::{t, use_locale, Locale, Msg};

const DEMO_TIMESTAMP: &str = "2026-11-26T04:30:00Z";
const DEMO_DUE_30D: &str = "2026-12-26";
const DEMO_PKCE_VERIFIER: &str = "dabney-moe-smart-fhir-pkce-verifier-2026-read-only-v1";

fn portal_status_badge_class(status: &PortalConnectionStatus) -> &'static str {
    match status {
        PortalConnectionStatus::Connected { .. } => {
            "border-emerald-400/40 bg-emerald-500/15 text-emerald-200"
        }
        PortalConnectionStatus::PendingAuth { .. } => {
            "border-cyan-400/40 bg-cyan-500/15 text-cyan-200"
        }
        PortalConnectionStatus::NeedsReauth { .. } => {
            "border-amber-400/40 bg-amber-500/15 text-amber-200"
        }
        PortalConnectionStatus::Disconnected { .. } => {
            "border-slate-500/40 bg-slate-800/80 text-slate-300"
        }
    }
}

fn capability_badge_class(cap: IntegrationCapability) -> &'static str {
    match cap {
        IntegrationCapability::Part2SudProtected => {
            "border-amber-400/45 bg-amber-500/15 text-amber-200"
        }
        IntegrationCapability::KipuApiV3Hmac => {
            "border-violet-400/40 bg-violet-500/15 text-violet-200"
        }
        IntegrationCapability::SmartOnFhirR4V2Pkce | IntegrationCapability::SmartOnFhirR4V1Pkce => {
            "border-emerald-400/35 bg-emerald-500/10 text-emerald-200"
        }
        IntegrationCapability::SmartHealthLink => "border-cyan-400/35 bg-cyan-500/10 text-cyan-200",
        IntegrationCapability::CcdaXmlImport | IntegrationCapability::Hl7V2Import => {
            "border-sky-400/35 bg-sky-500/10 text-sky-200"
        }
        IntegrationCapability::VendorRestApi | IntegrationCapability::AutomatedHipaaRoi => {
            "border-white/15 bg-white/[0.04] text-slate-200"
        }
    }
}

fn roi_status_badge_class(status: &RoiStatus) -> &'static str {
    match status {
        RoiStatus::Draft => "border-slate-500/40 bg-slate-800/80 text-slate-200",
        RoiStatus::SignedReadyToSend { .. } => "border-cyan-400/40 bg-cyan-500/15 text-cyan-200",
        RoiStatus::Dispatched { .. } => "border-emerald-400/40 bg-emerald-500/15 text-emerald-200",
        RoiStatus::Acknowledged { .. } => "border-teal-400/40 bg-teal-500/15 text-teal-200",
        RoiStatus::Fulfilled { .. } => "border-emerald-300/50 bg-emerald-500/25 text-emerald-100",
        RoiStatus::Revoked { .. } => "border-rose-400/40 bg-rose-500/15 text-rose-200",
    }
}

/// Interactive Doctor Portal Sign-In & FHIR R4 Link-Through panel.
///
/// Lets a patient search health systems (Epic/MyChart, Oracle Health,
/// athenahealth, VA, Medicare, or a custom SMART FHIR R4 URL), inspect read-only
/// PKCE `S256` OAuth parameters, complete a zero-PHI sandbox portal sign-in that
/// runs the real FHIR R4 ingestion pipeline, or upload any real FHIR R4 `.json`
/// bundle file directly into their local chart.
#[component]
pub fn PatientEhrPortalsPanel(
    patient: PatientChart,
    linked_portals: RwSignal<HashMap<String, Vec<LinkedPortalConnection>>>,
    charts: RwSignal<HashMap<String, PatientChart>>,
    allergies_map: RwSignal<HashMap<String, Vec<AllergyEntry>>>,
    demo_states: RwSignal<HashMap<String, PatientDemoState>>,
    #[prop(optional)] reset_tick: Option<RwSignal<u32>>,
    on_records_updated: Callback<String>,
) -> impl IntoView {
    let locale = use_locale();
    let patient_id = StoredValue::new(patient.id.clone());
    let patient_snapshot = StoredValue::new(patient);

    let search_query = RwSignal::new(String::new());
    let vendor_filter = RwSignal::new(Option::<PortalVendor>::None);
    let pending_session = RwSignal::new(Option::<SmartPkceSession>::None);
    let inspected_shlink = RwSignal::new(Option::<SmartHealthLinkPayload>::None);
    let custom_name = RwSignal::new(String::new());
    let custom_fhir_url = RwSignal::new(String::new());
    let custom_form_error = RwSignal::new(Option::<String>::None);
    let fhir_paste = RwSignal::new(String::new());
    let status_notice = RwSignal::new(Option::<String>::None);
    let error_notice = RwSignal::new(Option::<String>::None);

    // Universal "Add What You Have" intake state (paper/PDF/photo, pasted email, quick med/allergy/condition)
    let intake_tab = RwSignal::new(0u8); // 0 = Paper/PDF/Photo, 1 = Paste Email/Note, 2 = Quick Med/Allergy/Condition
    let paper_note = RwSignal::new(String::new());
    let email_source = RwSignal::new(String::new());
    let email_text = RwSignal::new(String::new());
    let manual_med_name = RwSignal::new(String::new());
    let manual_med_dose = RwSignal::new(String::new());
    let manual_med_schedule = RwSignal::new(String::new());
    let manual_allergy_sub = RwSignal::new(String::new());
    let manual_allergy_react = RwSignal::new(String::new());
    let manual_cond_label = RwSignal::new(String::new());
    let intake_notice = RwSignal::new(Option::<String>::None);
    let intake_error = RwSignal::new(Option::<String>::None);

    if let Some(tick) = reset_tick {
        Effect::new(move |prev: Option<u32>| {
            let current = tick.get();
            if prev.is_some() && prev != Some(current) {
                pending_session.set(None);
                inspected_shlink.set(None);
                custom_name.set(String::new());
                custom_fhir_url.set(String::new());
                custom_form_error.set(None);
                fhir_paste.set(String::new());
                status_notice.set(None);
                error_notice.set(None);
                paper_note.set(String::new());
                email_source.set(String::new());
                email_text.set(String::new());
                manual_med_name.set(String::new());
                manual_med_dose.set(String::new());
                manual_med_schedule.set(String::new());
                manual_allergy_sub.set(String::new());
                manual_allergy_react.set(String::new());
                manual_cond_label.set(String::new());
                intake_notice.set(None);
                intake_error.set(None);
            }
            current
        });
    }

    let start_smart_launch = move |org: PortalOrganization| {
        error_notice.set(None);
        status_notice.set(None);
        let state_tok = format!("dabney-state-{}", org.id);
        match build_smart_authorize_url(
            &org,
            "dabney-patient-smart-r4",
            "https://dabney.moe/clinical/patient",
            None,
            &state_tok,
            DEMO_PKCE_VERIFIER,
        ) {
            Ok(session) => {
                pending_session.set(Some(session));
            }
            Err(err) => {
                error_notice.set(Some(err));
            }
        }
    };

    let complete_portal_import =
        move |org_id: String,
              org_name: String,
              vendor: PortalVendor,
              fhir_base_url: String,
              raw_document: String,
              mode_override: Option<PortalConnectionMode>| {
            error_notice.set(None);
            let pid = patient_id.get_value();
            let source_label = format!("{org_name} ({})", vendor.short_badge());
            match ingest_clinical_document(&source_label, &org_id, &raw_document) {
                Ok(ingested) => {
                    if let Some(shl) = ingested.smart_health_link {
                        let host = shl.host();
                        let label_txt = shl
                            .label
                            .clone()
                            .unwrap_or_else(|| "SMART Health Link".to_string());
                        inspected_shlink.set(Some(shl));
                        let msg = format!(
                            "Verified SMART Health Link (`{label_txt}`) targeting HTTPS manifest host `{host}` with valid 32-byte AES-256 key."
                        );
                        status_notice.set(Some(msg.clone()));
                        on_records_updated.run(msg);
                        return;
                    }

                    let Some(import) = ingested.import else {
                        error_notice.set(Some("No clinical records decoded from input.".into()));
                        return;
                    };

                    let mode = mode_override.unwrap_or(match ingested.format {
                        ClinicalDocumentFormat::FhirR4Json => {
                            PortalConnectionMode::ImportedFhirBundle
                        }
                        ClinicalDocumentFormat::KipuJson => PortalConnectionMode::KipuApiV3,
                        ClinicalDocumentFormat::CcdaR21Xml => PortalConnectionMode::ImportedCcdaXml,
                        ClinicalDocumentFormat::Hl7V2Pipe => PortalConnectionMode::ImportedHl7V2,
                        ClinicalDocumentFormat::SmartHealthLinkUri => {
                            PortalConnectionMode::SmartHealthLink
                        }
                    });

                    let mut added_standard_ids: Vec<String> = Vec::new();
                    let mut added_part2_count = 0usize;
                    let mut counts = clinical_workspace::MergeCounts::default();
                    let mut current_allergies =
                        allergies_map.with_untracked(|m| m.get(&pid).cloned().unwrap_or_default());

                    charts.update(|map| {
                        if let Some(chart) = map.get_mut(&pid) {
                            let existing_ids: std::collections::HashSet<String> =
                                chart.records.iter().map(|r| r.id.clone()).collect();
                            counts = merge_fhir_import_into_chart(
                                chart,
                                &mut current_allergies,
                                &import,
                            );
                            for rec in &import.records {
                                if !existing_ids.contains(&rec.id) {
                                    if rec.confidentiality.is_part2_sud() {
                                        added_part2_count += 1;
                                    } else {
                                        added_standard_ids.push(rec.id.clone());
                                    }
                                }
                            }
                        }
                    });
                    allergies_map.update(|map| {
                        map.insert(pid.clone(), current_allergies);
                    });
                    // Only auto-select standard HIPAA records; 42 CFR Part 2 SUD records
                    // stay unselected & protected from Passport export unless opted in!
                    if !added_standard_ids.is_empty() {
                        demo_states.update(|map| {
                            if let Some(st) = map.get_mut(&pid) {
                                for id in &added_standard_ids {
                                    if !st.selected_record_ids.contains(id) {
                                        st.selected_record_ids.push(id.clone());
                                    }
                                }
                            }
                        });
                    }

                    let conn = LinkedPortalConnection {
                        organization_id: org_id.clone(),
                        organization_name: org_name.clone(),
                        vendor,
                        fhir_base_url,
                        status: PortalConnectionStatus::Connected {
                            connected_at: DEMO_TIMESTAMP.into(),
                            last_synced_at: DEMO_TIMESTAMP.into(),
                            mode,
                            imported_record_count: import.records.len(),
                            imported_medication_count: import.medications.len(),
                            imported_problem_count: import.problems.len(),
                            imported_allergy_count: import.allergies.len(),
                        },
                    };

                    linked_portals.update(|map| {
                        let list = map.entry(pid.clone()).or_default();
                        if let Some(slot) = list.iter_mut().find(|c| c.organization_id == org_id) {
                            *slot = conn;
                        } else {
                            list.push(conn);
                        }
                    });

                    pending_session.set(None);
                    let warn_note = if import.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" ({} partial warning(s) noted)", import.warnings.len())
                    };
                    let part2_note = if import.confidentiality.is_part2_sud()
                        || added_part2_count > 0
                    {
                        " Tagged 42 CFR Part 2 (SUD): excluded from Portable Health Passport exports unless you explicitly opt in."
                    } else {
                        " Records are now live in Your Health, the Agentic Harness, and your Portable Passport."
                    };
                    let msg = format!(
                        "Imported {} from {org_name}: merged {} new record(s), {} medication(s), {} problem(s), {} allergy(ies) ({} duplicate(s) skipped){warn_note}.{part2_note}",
                        ingested.format.label(),
                        counts.added_records,
                        counts.added_medications,
                        counts.added_problems,
                        counts.added_allergies,
                        counts.skipped_duplicates,
                    );
                    status_notice.set(Some(msg.clone()));
                    on_records_updated.run(msg);
                }
                Err(err) => {
                    error_notice.set(Some(err));
                }
            }
        };

    let on_custom_smart_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        custom_form_error.set(None);
        let name = custom_name.get().trim().to_string();
        let base = custom_fhir_url.get().trim().to_string();
        if name.is_empty() || base.is_empty() {
            let msg = "Enter both a clinic name and an HTTPS FHIR R4 base URL (e.g., https://fhir.yourclinic.org/api/FHIR/R4).".to_string();
            custom_form_error.set(Some(msg.clone()));
            error_notice.set(Some(msg));
            return;
        }
        let clean = base.trim_end_matches('/');
        let org = PortalOrganization {
            id: format!("org-custom-{}", name.len()),
            name,
            city_state: "Custom SMART on FHIR R4".into(),
            vendor: PortalVendor::CustomSmartFhir,
            fhir_base_url: clean.to_string(),
            authorize_url: format!("{clean}/oauth2/authorize"),
            token_url: format!("{clean}/oauth2/token"),
            him_email: None,
            him_fax: None,
            him_address: None,
            is_sandbox: false,
            transport: PortalVendor::CustomSmartFhir.default_transport_profile(),
        };
        if let Err(err) = build_smart_authorize_url(
            &org,
            "dabney-patient-smart-r4",
            "https://dabney.moe/clinical/patient",
            None,
            &format!("dabney-state-{}", org.id),
            DEMO_PKCE_VERIFIER,
        ) {
            custom_form_error.set(Some(err.clone()));
            error_notice.set(Some(err));
            return;
        }
        start_smart_launch(org);
    };

    let merge_patient_added_record = move |rec: clinical_workspace::ChartRecord, msg: String| {
        let pid = patient_id.get_value();
        let rec_id = rec.id.clone();
        charts.update(|map| {
            if let Some(chart) = map.get_mut(&pid) {
                if !chart.records.iter().any(|existing| existing.id == rec.id) {
                    chart.records.insert(0, rec);
                }
            }
        });
        demo_states.update(|map| {
            if let Some(st) = map.get_mut(&pid) {
                if !st.selected_record_ids.contains(&rec_id) {
                    st.selected_record_ids.push(rec_id);
                }
            }
        });
        intake_error.set(None);
        intake_notice.set(Some(msg.clone()));
        on_records_updated.run(msg);
    };

    let on_upload_paper_file = move |ev: leptos::ev::Event| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            use wasm_bindgen::JsCast;
            let Some(input) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                return;
            };
            let fname = file.name();
            let ftype = file.type_();
            let fsize = file.size() as u64;
            let note = paper_note.get_untracked();
            let rec = build_paper_attachment_record(&fname, &ftype, fsize, &note, "2026-11-26");
            paper_note.set(String::new());
            merge_patient_added_record(
                rec,
                format!("Added paper/PDF reference `{fname}` locally to your Health Passport (zero bytes uploaded)."),
            );
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        {
            let _ = (ev, merge_patient_added_record);
        }
    };

    let on_add_pasted_email = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        intake_error.set(None);
        let src = email_source.get_untracked();
        let raw = email_text.get_untracked();
        match parse_pasted_email_or_summary(&src, &raw, "2026-11-26") {
            Ok(rec) => {
                let title = rec.title.clone();
                email_source.set(String::new());
                email_text.set(String::new());
                merge_patient_added_record(
                    rec,
                    format!("Added `{title}` to your timeline and Health Passport (local only)."),
                );
            }
            Err(err) => {
                intake_notice.set(None);
                intake_error.set(Some(err));
            }
        }
    };

    let on_add_manual_med = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        intake_error.set(None);
        let name = manual_med_name.get_untracked().trim().to_string();
        if name.is_empty() {
            intake_error.set(Some(
                "Enter a medication name first (e.g., Lisinopril).".into(),
            ));
            return;
        }
        let dose = manual_med_dose.get_untracked().trim().to_string();
        let schedule = manual_med_schedule.get_untracked().trim().to_string();
        let pid = patient_id.get_value();
        charts.update(|map| {
            if let Some(chart) = map.get_mut(&pid) {
                chart.medications.push(Medication {
                    name: name.clone(),
                    dose: if dose.is_empty() {
                        "Dose as prescribed".into()
                    } else {
                        dose
                    },
                    schedule: if schedule.is_empty() {
                        "Patient-added".into()
                    } else {
                        schedule
                    },
                    indication: "Added by patient".into(),
                    confidentiality: ConfidentialityClass::StandardHipaa,
                });
            }
        });
        manual_med_name.set(String::new());
        manual_med_dose.set(String::new());
        manual_med_schedule.set(String::new());
        let msg = format!("Added medication `{name}` to your Health Passport.");
        intake_notice.set(Some(msg.clone()));
        on_records_updated.run(msg);
    };

    let on_add_manual_allergy = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        intake_error.set(None);
        let substance = manual_allergy_sub.get_untracked().trim().to_string();
        if substance.is_empty() {
            intake_error.set(Some(
                "Enter an allergy or intolerance substance first (e.g., Penicillin).".into(),
            ));
            return;
        }
        let reaction = manual_allergy_react.get_untracked().trim().to_string();
        let pid = patient_id.get_value();
        allergies_map.update(|map| {
            let list = map.entry(pid).or_default();
            let next_idx = list.len() + 1;
            list.push(AllergyEntry {
                id: format!("alg-patient-{next_idx}"),
                substance: substance.clone(),
                reaction: if reaction.is_empty() {
                    "Patient-reported reaction / intolerance".into()
                } else {
                    reaction
                },
                criticality: "High".into(),
                recorded_at: "2026-11-26".into(),
                source_label: "Patient-Added".into(),
                confidentiality: ConfidentialityClass::StandardHipaa,
            });
        });
        manual_allergy_sub.set(String::new());
        manual_allergy_react.set(String::new());
        let msg = format!("Added allergy `{substance}` to your Health Passport.");
        intake_notice.set(Some(msg.clone()));
        on_records_updated.run(msg);
    };

    let on_add_manual_condition = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        intake_error.set(None);
        let label = manual_cond_label.get_untracked().trim().to_string();
        if label.is_empty() {
            intake_error.set(Some(
                "Enter a condition or past surgery first (e.g., Asthma or 2021 Appendectomy)."
                    .into(),
            ));
            return;
        }
        let pid = patient_id.get_value();
        charts.update(|map| {
            if let Some(chart) = map.get_mut(&pid) {
                chart.problems.push(Problem {
                    code: "PATIENT".into(),
                    label: label.clone(),
                    status: "Patient-added".into(),
                    confidentiality: ConfidentialityClass::StandardHipaa,
                });
            }
        });
        manual_cond_label.set(String::new());
        let msg = format!("Added condition/history `{label}` to your Health Passport.");
        intake_notice.set(Some(msg.clone()));
        on_records_updated.run(msg);
    };

    let on_paste_fhir_import = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let raw = fhir_paste.get();
        if raw.trim().is_empty() {
            error_notice.set(Some(
                "Paste a FHIR R4 JSON, KipuEMR JSON, C-CDA R2.1 XML, HL7 v2 message, or shlink:/ URI first."
                    .into(),
            ));
            return;
        }
        complete_portal_import(
            "org-manual-import".into(),
            "Pasted Clinical Export".into(),
            PortalVendor::CustomSmartFhir,
            "local://clinical-document-paste".into(),
            raw,
            None,
        );
        fhir_paste.set(String::new());
    };

    let on_upload_fhir_file = move |ev: leptos::ev::Event| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            use wasm_bindgen::{closure::Closure, JsCast};
            let Some(input) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                return;
            };
            let fname = file.name();
            let Ok(reader) = web_sys::FileReader::new() else {
                error_notice.set(Some("Browser FileReader unavailable.".into()));
                return;
            };
            let reader_cb = reader.clone();
            let onload = Closure::once(move |_: web_sys::ProgressEvent| {
                if let Ok(val) = reader_cb.result() {
                    if let Some(text) = val.as_string() {
                        complete_portal_import(
                            format!("file-{}", fname.len()),
                            format!("Clinical File: {fname}"),
                            PortalVendor::CustomSmartFhir,
                            format!("local://{fname}"),
                            text,
                            None,
                        );
                    }
                }
            });
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            let _ = reader.read_as_text(&file);
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        let _ = ev;
    };

    view! {
        <div class="space-y-6" data-patient-ehr-portals="true">
            // Universal "Add What You Have" Intake (Paper, PDFs, Photos, Emails, Manual Meds/Allergies)
            <section
                data-universal-patient-intake="true"
                class="rounded-2xl border border-emerald-400/35 bg-gradient-to-br from-[#01081b] via-slate-900/90 to-slate-950 p-5 space-y-4"
            >
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "No Portal Required · 100% Local on Your Device"
                        </p>
                        <h2 class="mt-1 text-lg font-bold text-white">
                            {move || t(locale.get(), Msg::PassportAddWhatYouHave)}
                        </h2>
                        <p class="mt-1 text-xs leading-relaxed text-slate-300">
                            "Most health history lives in paper folders, PDF lab reports, portal emails, and memory—not just Epic. Drop in whatever you have right here and it merges straight into your Health Passport."
                        </p>
                    </div>
                </div>

                <div class="flex flex-wrap gap-2" role="tablist" aria-label="Universal record intake modes">
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || (intake_tab.get() == 0).to_string()
                        on:click=move |_| intake_tab.set(0)
                        class=move || if intake_tab.get() == 0 {
                            "min-h-[38px] rounded-full bg-emerald-500 px-3.5 py-1.5 text-xs font-semibold text-slate-950"
                        } else {
                            "min-h-[38px] rounded-full border border-white/15 bg-white/5 px-3.5 py-1.5 text-xs font-medium text-slate-200 hover:bg-white/10"
                        }
                    >
                        "1. Paper / PDF / Photo"
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || (intake_tab.get() == 1).to_string()
                        on:click=move |_| intake_tab.set(1)
                        class=move || if intake_tab.get() == 1 {
                            "min-h-[38px] rounded-full bg-emerald-500 px-3.5 py-1.5 text-xs font-semibold text-slate-950"
                        } else {
                            "min-h-[38px] rounded-full border border-white/15 bg-white/5 px-3.5 py-1.5 text-xs font-medium text-slate-200 hover:bg-white/10"
                        }
                    >
                        "2. Paste Email / Visit Note"
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || (intake_tab.get() == 2).to_string()
                        on:click=move |_| intake_tab.set(2)
                        class=move || if intake_tab.get() == 2 {
                            "min-h-[38px] rounded-full bg-emerald-500 px-3.5 py-1.5 text-xs font-semibold text-slate-950"
                        } else {
                            "min-h-[38px] rounded-full border border-white/15 bg-white/5 px-3.5 py-1.5 text-xs font-medium text-slate-200 hover:bg-white/10"
                        }
                    >
                        "3. Quick Med / Allergy / Condition"
                    </button>
                </div>

                {move || match intake_tab.get() {
                    0 => view! {
                        <div class="rounded-xl border border-white/10 bg-slate-950/65 p-4 space-y-3 text-xs">
                            <p class="text-slate-300">
                                "Attach a PDF lab report, discharge paperwork, or photo of a paper sheet. Only the local reference and your note are added—zero files are ever uploaded."
                            </p>
                            <input
                                type="text"
                                aria-label="Optional note about this paper record or PDF"
                                placeholder="Short note about this paper/PDF (e.g., 2025 Cardiology Echo PDF or ER discharge sheet)..."
                                prop:value=move || paper_note.get()
                                on:input=move |ev| paper_note.set(event_target_value(&ev))
                                class="w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                            />
                            <div class="flex flex-wrap items-center gap-3">
                                <label class="block">
                                    <span class="sr-only">"Choose a paper scan, PDF, or photo"</span>
                                    <input
                                        type="file"
                                        aria-label="Choose a paper scan, PDF, or photo"
                                        accept=".pdf,.png,.jpg,.jpeg,.webp,.txt,application/pdf,image/*"
                                        on:change=on_upload_paper_file
                                        class="block text-xs text-slate-300 file:mr-3 file:rounded-full file:border-0 file:bg-emerald-500 file:px-3.5 file:py-1.5 file:text-xs file:font-semibold file:text-slate-950 hover:file:bg-emerald-400"
                                    />
                                </label>
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        let note = paper_note.get_untracked();
                                        let rec = build_paper_attachment_record(
                                            "prior-clinic-discharge-summary.pdf",
                                            "application/pdf",
                                            184_320,
                                            if note.trim().is_empty() {
                                                "Sample paper discharge summary & med reconciliation sheet from prior clinic."
                                            } else {
                                                &note
                                            },
                                            "2026-11-26",
                                        );
                                        paper_note.set(String::new());
                                        merge_patient_added_record(
                                            rec,
                                            "Added sample paper/PDF discharge record locally to your Health Passport.".into(),
                                        );
                                    }
                                    class="rounded-full border border-cyan-400/40 bg-cyan-500/10 px-3 py-1.5 text-[11px] font-semibold text-cyan-200 hover:bg-cyan-500/20"
                                >
                                    "+ Try Sample Paper PDF"
                                </button>
                            </div>
                        </div>
                    }.into_any(),
                    1 => view! {
                        <form on:submit=on_add_pasted_email class="rounded-xl border border-white/10 bg-slate-950/65 p-4 space-y-2.5 text-xs">
                            <input
                                type="text"
                                aria-label="Where is this email or note from?"
                                placeholder="Where is this from? (e.g., Email from Dr. Lin, Quest Lab PDF, Therapist summary)"
                                prop:value=move || email_source.get()
                                on:input=move |ev| email_source.set(event_target_value(&ev))
                                class="w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                            />
                            <textarea
                                rows="3"
                                aria-label="Paste email, visit summary, or lab note text"
                                placeholder="Paste any doctor email, portal message, or visit summary (e.g., Subject: Lab Results / A1c: 6.7% / Keep current dose)..."
                                prop:value=move || email_text.get()
                                on:input=move |ev| email_text.set(event_target_value(&ev))
                                class="w-full rounded-xl border border-white/15 bg-slate-950 p-2.5 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                            />
                            <div class="flex flex-wrap gap-2">
                                <button
                                    type="submit"
                                    class="rounded-full bg-emerald-500 px-4 py-1.5 text-xs font-semibold text-slate-950 hover:bg-emerald-400"
                                >
                                    "Add to My Health Passport"
                                </button>
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        email_source.set("Prior Primary Care Email (Sample)".into());
                                        email_text.set(
                                            "Subject: Annual Labs & Medication Check\nProvider: Dr. Lin\nA1c: 6.7%\nBP: 124/78\nLabs stable; continue current extended-release dose and recheck in 6 months."
                                                .into(),
                                        );
                                    }
                                    class="rounded-full border border-cyan-400/40 bg-cyan-500/10 px-3 py-1.5 text-[11px] font-semibold text-cyan-200 hover:bg-cyan-500/20"
                                >
                                    "Fill Sample Doctor Email"
                                </button>
                            </div>
                        </form>
                    }.into_any(),
                    _ => view! {
                        <div class="grid gap-3 md:grid-cols-3 text-xs">
                            <form on:submit=on_add_manual_med class="rounded-xl border border-white/10 bg-slate-950/65 p-3.5 space-y-2">
                                <p class="font-semibold text-emerald-300">"Quick Add Medication"</p>
                                <input
                                    type="text"
                                    aria-label="Medication name"
                                    placeholder="Name (e.g., Sertraline)"
                                    prop:value=move || manual_med_name.get()
                                    on:input=move |ev| manual_med_name.set(event_target_value(&ev))
                                    class="w-full rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-400"
                                />
                                <input
                                    type="text"
                                    aria-label="Medication dose"
                                    placeholder="Dose (e.g., 50 mg)"
                                    prop:value=move || manual_med_dose.get()
                                    on:input=move |ev| manual_med_dose.set(event_target_value(&ev))
                                    class="w-full rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-400"
                                />
                                <input
                                    type="text"
                                    aria-label="Medication schedule"
                                    placeholder="Schedule (e.g., Once daily AM)"
                                    prop:value=move || manual_med_schedule.get()
                                    on:input=move |ev| manual_med_schedule.set(event_target_value(&ev))
                                    class="w-full rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-400"
                                />
                                <button type="submit" class="w-full rounded-full border border-emerald-400/40 bg-emerald-500/15 py-1.5 font-semibold text-emerald-200 hover:bg-emerald-500/25">
                                    "+ Add Medication"
                                </button>
                            </form>

                            <form on:submit=on_add_manual_allergy class="rounded-xl border border-white/10 bg-slate-950/65 p-3.5 space-y-2">
                                <p class="font-semibold text-rose-300">"Quick Add Allergy / Intolerance"</p>
                                <input
                                    type="text"
                                    aria-label="Allergy substance"
                                    placeholder="Substance (e.g., Sulfa drugs)"
                                    prop:value=move || manual_allergy_sub.get()
                                    on:input=move |ev| manual_allergy_sub.set(event_target_value(&ev))
                                    class="w-full rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-400"
                                />
                                <input
                                    type="text"
                                    aria-label="Allergy reaction"
                                    placeholder="Reaction (e.g., Rash / hives)"
                                    prop:value=move || manual_allergy_react.get()
                                    on:input=move |ev| manual_allergy_react.set(event_target_value(&ev))
                                    class="w-full rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-400"
                                />
                                <button type="submit" class="w-full rounded-full border border-rose-400/40 bg-rose-500/15 py-1.5 font-semibold text-rose-200 hover:bg-rose-500/25">
                                    "+ Add Allergy"
                                </button>
                            </form>

                            <form on:submit=on_add_manual_condition class="rounded-xl border border-white/10 bg-slate-950/65 p-3.5 space-y-2">
                                <p class="font-semibold text-cyan-300">"Quick Add Condition / Surgery"</p>
                                <input
                                    type="text"
                                    aria-label="Condition or past surgery"
                                    placeholder="e.g., Migraines or 2020 Knee Surgery"
                                    prop:value=move || manual_cond_label.get()
                                    on:input=move |ev| manual_cond_label.set(event_target_value(&ev))
                                    class="w-full rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-400"
                                />
                                <button type="submit" class="w-full rounded-full border border-cyan-400/40 bg-cyan-500/15 py-1.5 font-semibold text-cyan-200 hover:bg-cyan-500/25">
                                    "+ Add Condition"
                                </button>
                            </form>
                        </div>
                    }.into_any(),
                }}

                {move || intake_notice.get().map(|msg| view! {
                    <p role="status" class="rounded-xl border border-emerald-400/35 bg-emerald-500/10 px-3.5 py-2 text-xs text-emerald-100">{msg}</p>
                })}
                {move || intake_error.get().map(|err| view! {
                    <p role="alert" class="rounded-xl border border-rose-400/40 bg-rose-500/10 px-3.5 py-2 text-xs text-rose-200">{err}</p>
                })}
            </section>

            // Top explanation & read-only guarantee
            <section class="rounded-2xl border border-cyan-400/25 bg-slate-900/60 p-5 space-y-3">
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                            "Patient Portal Link-Through · SMART on FHIR R4 + PKCE"
                        </p>
                        <h2 class="mt-1 text-lg font-semibold text-white">
                            {move || t(locale.get(), Msg::PassportPortalsHeading)}
                        </h2>
                        <p class="mt-1 text-xs leading-relaxed text-slate-300">
                            "Connect one or more health systems (Epic / MyChart, Oracle Health, athenahealth, VA, Medicare, or any SMART on FHIR R4 clinic) using read-only OAuth 2.0 PKCE (S256), or import a downloaded FHIR R4 bundle. "
                            "Records merge straight into your chart and portable Health Passport—Dabney.moe never writes back to your doctor's EHR."
                        </p>
                    </div>
                    <span class="shrink-0 rounded-full border border-emerald-400/40 bg-emerald-500/10 px-3 py-1 font-mono text-[11px] font-semibold text-emerald-300">
                        "Read-Only · PKCE S256"
                    </span>
                </div>

                {move || status_notice.get().map(|msg| view! {
                    <p role="status" class="rounded-xl border border-emerald-400/35 bg-emerald-500/10 px-3.5 py-2.5 text-xs text-emerald-100">
                        {msg}
                    </p>
                })}
                {move || error_notice.get().map(|err| view! {
                    <p role="alert" class="rounded-xl border border-rose-400/40 bg-rose-500/10 px-3.5 py-2.5 text-xs text-rose-200">
                        {err}
                    </p>
                })}
            </section>

            // Connected portals status list
            {move || {
                let pid = patient_id.get_value();
                let connected = linked_portals.with(|m| m.get(&pid).cloned().unwrap_or_default());
                (!connected.is_empty()).then(|| view! {
                    <section class="rounded-2xl border border-emerald-400/30 bg-emerald-500/[0.04] p-5 space-y-3">
                        <div class="flex items-center justify-between gap-2">
                            <h3 class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                                {format!("Linked Patient Portals ({})", connected.len())}
                            </h3>
                            <span class="text-[11px] text-slate-300">
                                "Disconnecting stops future refreshes while keeping imported history."
                            </span>
                        </div>
                        <div class="space-y-2.5">
                            {connected
                                .into_iter()
                                .map(|conn| {
                                    let badge_cls = portal_status_badge_class(&conn.status);
                                    let status_label = conn.status.badge_label();
                                    let detail_text = match &conn.status {
                                        PortalConnectionStatus::Connected {
                                            mode,
                                            imported_record_count,
                                            imported_medication_count,
                                            imported_allergy_count,
                                            last_synced_at,
                                            ..
                                        } => format!(
                                            "{} · {} record(s), {} med(s), {} allergy(ies) · synced {}",
                                            mode.label(),
                                            imported_record_count,
                                            imported_medication_count,
                                            imported_allergy_count,
                                            last_synced_at
                                        ),
                                        PortalConnectionStatus::Disconnected {
                                            retained_record_count,
                                            disconnected_at,
                                        } => format!(
                                            "Disconnected {disconnected_at} · {retained_record_count} imported record(s) retained locally."
                                        ),
                                        _ => conn.fhir_base_url.clone(),
                                    };
                                    let org_id_disc = conn.organization_id.clone();
                                    let org_id_sync = conn.organization_id.clone();
                                    let org_name_sync = conn.organization_name.clone();
                                    let vendor_sync = conn.vendor;
                                    let base_sync = conn.fhir_base_url.clone();
                                    let is_conn = conn.status.is_connected();

                                    view! {
                                        <div class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/70 p-3.5">
                                            <div>
                                                <div class="flex flex-wrap items-center gap-2">
                                                    <span class="text-sm font-semibold text-white">
                                                        {conn.organization_name}
                                                    </span>
                                                    <span class="rounded bg-white/10 px-2 py-0.5 font-mono text-[10px] text-cyan-200">
                                                        {conn.vendor.short_badge()}
                                                    </span>
                                                    <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-semibold {badge_cls}")>
                                                        {status_label}
                                                    </span>
                                                </div>
                                                <p class="mt-1 text-xs text-slate-300">{detail_text}</p>
                                            </div>
                                            <div class="flex flex-wrap gap-2">
                                                <button
                                                    type="button"
                                                    on:click=move |_| {
                                                        let payload = patient_snapshot.with_value(|p| {
                                                            if vendor_sync == PortalVendor::KipuEmr {
                                                                sandbox_kipu_payload_for_patient(p)
                                                            } else if !vendor_sync.default_transport_profile().supports_smart_on_fhir() {
                                                                sandbox_ccda_xml_for_patient(p)
                                                            } else {
                                                                sandbox_fhir_bundle_for_org(&org_id_sync, p)
                                                            }
                                                        });
                                                        let mode = if vendor_sync == PortalVendor::KipuEmr {
                                                            Some(PortalConnectionMode::KipuApiV3)
                                                        } else {
                                                            Some(PortalConnectionMode::InteractiveSandboxDemo)
                                                        };
                                                        complete_portal_import(
                                                            org_id_sync.clone(),
                                                            org_name_sync.clone(),
                                                            vendor_sync,
                                                            base_sync.clone(),
                                                            payload,
                                                            mode,
                                                        );
                                                    }
                                                    class="rounded-full border border-cyan-400/35 bg-cyan-500/10 px-3 py-1 text-[11px] font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                                                >
                                                    {if is_conn { "Refresh sync" } else { "Reconnect" }}
                                                </button>
                                                {is_conn.then(|| view! {
                                                    <button
                                                        type="button"
                                                        on:click=move |_| {
                                                            let pid = patient_id.get_value();
                                                            linked_portals.update(|map| {
                                                                if let Some(list) = map.get_mut(&pid) {
                                                                    if let Some(item) = list.iter_mut().find(|c| c.organization_id == org_id_disc) {
                                                                        let retained = match item.status {
                                                                            PortalConnectionStatus::Connected { imported_record_count, .. } => imported_record_count,
                                                                            _ => 0,
                                                                        };
                                                                        item.status = PortalConnectionStatus::Disconnected {
                                                                            disconnected_at: DEMO_TIMESTAMP.into(),
                                                                            retained_record_count: retained,
                                                                        };
                                                                    }
                                                                }
                                                            });
                                                            status_notice.set(Some("Disconnected portal; imported records remain in your local chart and Health Passport.".into()));
                                                        }
                                                        class="rounded-full border border-white/15 px-3 py-1 text-[11px] text-slate-300 transition hover:border-rose-400/40 hover:text-rose-200"
                                                    >
                                                        "Disconnect"
                                                    </button>
                                                })}
                                            </div>
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </section>
                })
            }}

            // Active SMART on FHIR R4 PKCE Consent & Launch Sheet
            {move || {
                pending_session.get().map(|session| {
                    let s_org_id = session.organization_id.clone();
                    let s_org_name = session.organization_name.clone();
                    let s_vendor = session.vendor;
                    let s_base = session.fhir_base_url.clone();
                    view! {
                        <section
                            data-smart-consent-sheet="true"
                            class="rounded-2xl border border-cyan-300/50 bg-[#01081b] p-5 space-y-4 shadow-xl shadow-cyan-500/10"
                        >
                            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-white/10 pb-3">
                                <div>
                                    <p class="text-[11px] font-semibold uppercase tracking-widest text-emerald-300">
                                        "SMART on FHIR R4 OAuth 2.0 + PKCE Consent Ready"
                                    </p>
                                    <h3 class="mt-1 text-base font-bold text-white">
                                        {session.organization_name.clone()}
                                    </h3>
                                    <p class="mt-0.5 font-mono text-[11px] text-slate-300">
                                        {format!("FHIR Base: {}", session.fhir_base_url)}
                                    </p>
                                </div>
                                <button
                                    type="button"
                                    on:click=move |_| pending_session.set(None)
                                    class="rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 hover:bg-white/5"
                                >
                                    "Cancel"
                                </button>
                            </div>

                            <div class="grid gap-3 sm:grid-cols-2 text-xs">
                                <div class="rounded-xl border border-white/10 bg-slate-950/70 p-3 space-y-1">
                                    <p class="font-semibold text-cyan-300">"PKCE Challenge (RFC 7636 S256)"</p>
                                    <p class="break-all font-mono text-[11px] text-slate-200">
                                        {session.code_challenge.clone()}
                                    </p>
                                    <p class="text-[11px] text-slate-400">
                                        {format!("State: {} · Read-only: {}", session.state, session.read_only)}
                                    </p>
                                </div>
                                <div class="rounded-xl border border-white/10 bg-slate-950/70 p-3 space-y-1">
                                    <p class="font-semibold text-emerald-300">"Requested Read-Only Scopes"</p>
                                    <p class="break-words font-mono text-[10px] leading-relaxed text-slate-300">
                                        {session.scopes.clone()}
                                    </p>
                                </div>
                            </div>

                            <div class="flex flex-wrap items-center gap-3">
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        let bundle = patient_snapshot.with_value(|p| {
                                            sandbox_fhir_bundle_for_org(&s_org_id, p)
                                        });
                                        complete_portal_import(
                                            s_org_id.clone(),
                                            s_org_name.clone(),
                                            s_vendor,
                                            s_base.clone(),
                                            bundle,
                                            Some(PortalConnectionMode::InteractiveSandboxDemo),
                                        );
                                    }
                                    class="min-h-[42px] rounded-full bg-emerald-500 px-5 py-2 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                                >
                                    "Complete Sandbox Portal Sign-In & Import FHIR R4 Bundle →"
                                </button>
                                <a
                                    href=session.authorize_url.clone()
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    class="inline-flex min-h-[42px] items-center rounded-full border border-cyan-400/40 bg-cyan-500/10 px-4 py-2 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                                >
                                    "Inspect / Open Real OAuth URL ↗"
                                </a>
                            </div>
                        </section>
                    }
                })
            }}

            // Inspected SMART Health Link (shlink:/) verification card
            {move || {
                inspected_shlink.get().map(|shl| {
                    let host = shl.host();
                    let label = shl
                        .label
                        .clone()
                        .unwrap_or_else(|| "SMART Health Link".to_string());
                    let flags_view = shl.flags.clone();
                    let exp_txt = shl
                        .exp
                        .map(|ts| format!("UNIX {ts}"))
                        .unwrap_or_else(|| "No expiration".into());
                    view! {
                        <section
                            data-shlink-preview="true"
                            class="rounded-2xl border border-cyan-400/45 bg-cyan-500/[0.06] p-4 space-y-2.5 text-xs"
                        >
                            <div class="flex flex-wrap items-center justify-between gap-2">
                                <div>
                                    <p class="text-[11px] font-semibold uppercase tracking-widest text-cyan-300">
                                        "SMART Health Link (shlink:/) Verified · HTTPS SSRF-Checked"
                                    </p>
                                    <h3 class="mt-0.5 text-sm font-bold text-white">{label}</h3>
                                </div>
                                <button
                                    type="button"
                                    on:click=move |_| inspected_shlink.set(None)
                                    class="rounded-full border border-white/15 px-3 py-1 text-[11px] text-slate-300 hover:bg-white/5"
                                >
                                    "Dismiss"
                                </button>
                            </div>
                            <div class="flex flex-wrap items-center gap-2 font-mono text-[11px] text-slate-200">
                                <span>{format!("Manifest host: {host}")}</span>
                                <span>"·"</span>
                                <span class="text-emerald-300">"32-byte AES-256 key validated (not displayed)"</span>
                                <span>"·"</span>
                                <span>{format!("v{} · Expires: {exp_txt}", shl.v)}</span>
                            </div>
                            <div class="flex flex-wrap gap-1.5">
                                {flags_view.into_iter().map(|fl| view! {
                                    <span class="rounded-full border border-cyan-400/35 bg-slate-950/70 px-2.5 py-0.5 text-[10px] font-semibold text-cyan-200">
                                        {fl.label()}
                                    </span>
                                }).collect_view()}
                            </div>
                        </section>
                    }
                })
            }}

            // Searchable Health System Directory
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <div>
                        <h3 class="text-base font-semibold text-white">
                            "Doctors & Hospitals Directory"
                        </h3>
                        <p class="text-xs text-slate-300">
                            "Search SMART on FHIR R4, behavioral health (KipuEMR · 42 CFR Part 2), C-CDA / HL7 v2, and ROI-fallback EHRs."
                        </p>
                    </div>
                </div>

                <div class="space-y-2.5">
                    <input
                        type="search"
                        aria-label="Search doctors and hospital portals"
                        placeholder="Search Epic, Oracle, athenahealth, healow, MEDITECH, NextGen, Veradigm, Elation, VA, Medicare, KipuEMR, Hint, SimplePractice, DrChrono, ModMed..."
                        prop:value=move || search_query.get()
                        on:input=move |ev| search_query.set(event_target_value(&ev))
                        class="min-h-[42px] w-full rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2 text-sm text-white placeholder:text-slate-400 focus:border-cyan-400/60 focus:outline-none"
                    />
                    <div class="flex flex-wrap gap-1.5">
                        <button
                            type="button"
                            on:click=move |_| vendor_filter.set(None)
                            class=move || if vendor_filter.get().is_none() {
                                "rounded-full bg-cyan-500 px-3 py-1 text-[11px] font-semibold text-slate-950"
                            } else {
                                "rounded-full border border-white/15 px-3 py-1 text-[11px] text-slate-300 hover:text-white"
                            }
                        >
                            "All"
                        </button>
                        {PortalVendor::all()
                            .iter()
                            .copied()
                            .filter(|v| *v != PortalVendor::CustomSmartFhir)
                            .map(|v| {
                                view! {
                                    <button
                                        type="button"
                                        on:click=move |_| vendor_filter.set(Some(v))
                                        class=move || if vendor_filter.get() == Some(v) {
                                            "rounded-full bg-cyan-500 px-3 py-1 text-[11px] font-semibold text-slate-950"
                                        } else {
                                            "rounded-full border border-white/15 px-3 py-1 text-[11px] text-slate-300 hover:text-white"
                                        }
                                    >
                                        {v.short_badge()}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>

                <div class="grid gap-3 sm:grid-cols-2">
                    {move || {
                        let results = search_portal_directory(&search_query.get(), vendor_filter.get());
                        if results.is_empty() {
                            return view! {
                                <p class="col-span-full rounded-xl border border-white/10 bg-slate-950/60 p-4 text-xs text-slate-300">
                                    "No portals matched that query. Enter a custom SMART on FHIR R4 URL below or send an automated ROI form."
                                </p>
                            }
                            .into_any();
                        }
                        results
                            .into_iter()
                            .map(|org| {
                                let supports_smart = org.supports_smart_on_fhir();
                                let is_kipu = org.transport.supports_kipu_api();
                                let caps = org.transport.capabilities.clone();
                                let notes = org.transport.notes.clone();
                                let endpoint_preview = if org.fhir_base_url.is_empty() {
                                    "C-CDA / PDF Export & Automated ROI".to_string()
                                } else {
                                    org.fhir_base_url.clone()
                                };
                                let org_for_btn = org.clone();
                                view! {
                                    <article class="flex flex-col justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/65 p-4 transition hover:border-cyan-400/35">
                                        <div class="space-y-1.5">
                                            <div class="flex flex-wrap items-center justify-between gap-2">
                                                <span class="rounded bg-white/10 px-2 py-0.5 font-mono text-[10px] text-cyan-200">
                                                    {org.vendor.short_badge()}
                                                </span>
                                                {org.is_sandbox.then(|| view! {
                                                    <span class="rounded-full border border-emerald-400/35 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-semibold text-emerald-300">
                                                        "Zero-PHI Sandbox"
                                                    </span>
                                                })}
                                            </div>
                                            <h4 class="text-sm font-semibold text-white">{org.name}</h4>
                                            <p class="text-xs text-slate-400">{org.city_state}</p>
                                            <div class="flex flex-wrap gap-1 pt-0.5">
                                                {caps.into_iter().map(|cap| {
                                                    let cls = capability_badge_class(cap);
                                                    view! {
                                                        <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-medium {cls}")>
                                                            {cap.badge_label()}
                                                        </span>
                                                    }
                                                }).collect_view()}
                                            </div>
                                            <p class="text-[11px] leading-snug text-slate-300">{notes}</p>
                                        </div>
                                        <div class="flex items-center justify-between gap-2 border-t border-white/10 pt-2.5">
                                            <span class="truncate font-mono text-[10px] text-slate-400">
                                                {endpoint_preview}
                                            </span>
                                            {if supports_smart {
                                                view! {
                                                    <button
                                                        type="button"
                                                        on:click=move |_| start_smart_launch(org_for_btn.clone())
                                                        class="shrink-0 rounded-full bg-emerald-500 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                                                    >
                                                        "Sign in →"
                                                    </button>
                                                }
                                                .into_any()
                                            } else if is_kipu {
                                                view! {
                                                    <button
                                                        type="button"
                                                        on:click=move |_| {
                                                            let kipu_json = patient_snapshot.with_value(sandbox_kipu_payload_for_patient);
                                                            complete_portal_import(
                                                                org_for_btn.id.clone(),
                                                                org_for_btn.name.clone(),
                                                                org_for_btn.vendor,
                                                                org_for_btn.fhir_base_url.clone(),
                                                                kipu_json,
                                                                Some(PortalConnectionMode::KipuApiV3),
                                                            );
                                                        }
                                                        class="shrink-0 rounded-full bg-amber-400 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-amber-300"
                                                    >
                                                        "Load Kipu (Part 2) →"
                                                    </button>
                                                }
                                                .into_any()
                                            } else {
                                                view! {
                                                    <button
                                                        type="button"
                                                        on:click=move |_| {
                                                            let ccda = patient_snapshot.with_value(sandbox_ccda_xml_for_patient);
                                                            complete_portal_import(
                                                                org_for_btn.id.clone(),
                                                                org_for_btn.name.clone(),
                                                                org_for_btn.vendor,
                                                                "local://ccda-export".into(),
                                                                ccda,
                                                                Some(PortalConnectionMode::ImportedCcdaXml),
                                                            );
                                                        }
                                                        class="shrink-0 rounded-full border border-cyan-400/45 bg-cyan-500/15 px-3 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/25"
                                                    >
                                                        "Import C-CDA / ROI"
                                                    </button>
                                                }
                                                .into_any()
                                            }}
                                        </div>
                                    </article>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </div>
            </section>

            // Custom SMART Endpoint + Multi-Format Clinical Document Import
            <div class="grid gap-4 md:grid-cols-2">
                <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-3">
                    <h3 class="text-sm font-semibold text-white">
                        "Connect Any Custom SMART on FHIR R4 Portal"
                    </h3>
                    <p class="text-xs text-slate-300">
                        "Paste any clinic's HTTPS FHIR R4 base URL to generate a read-only PKCE S256 launch session."
                    </p>
                    <form on:submit=on_custom_smart_submit novalidate=true class="space-y-2.5">
                        <input
                            type="text"
                            aria-label="Custom clinic name"
                            placeholder="Clinic or hospital name (e.g., Harborview Cardiology)"
                            prop:value=move || custom_name.get()
                            on:input=move |ev| {
                                custom_form_error.set(None);
                                custom_name.set(event_target_value(&ev));
                            }
                            class="w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-cyan-400/60 focus:outline-none"
                        />
                        <input
                            type="url"
                            aria-label="Custom clinic FHIR R4 base URL"
                            placeholder="https://fhir.yourclinic.org/api/FHIR/R4"
                            prop:value=move || custom_fhir_url.get()
                            on:input=move |ev| {
                                custom_form_error.set(None);
                                custom_fhir_url.set(event_target_value(&ev));
                            }
                            class="w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 font-mono text-xs text-white placeholder:text-slate-400 focus:border-cyan-400/60 focus:outline-none"
                        />
                        <button
                            type="submit"
                            class="rounded-full border border-cyan-400/40 bg-cyan-500/15 px-4 py-2 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/25"
                        >
                            "Build PKCE S256 Launch URL"
                        </button>
                        {move || custom_form_error.get().map(|err| view! {
                            <p role="alert" class="rounded-xl border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">{err}</p>
                        })}
                    </form>
                </section>

                <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-3">
                    <h3 class="text-sm font-semibold text-white">
                        "Import Downloaded FHIR R4 Bundle, Kipu JSON, C-CDA XML, HL7 v2, or shlink:/"
                    </h3>
                    <p class="text-xs text-slate-300">
                        "Auto-detects FHIR R4 (.json), KipuEMR API JSON (42 CFR Part 2), C-CDA R2.1 (.xml), HL7 v2 (.hl7), or SMART Health Links (`shlink:/`) locally in browser RAM."
                    </p>
                    <label class="block">
                        <span class="sr-only">"Choose a clinical document file (.json, .xml, .hl7)"</span>
                        <input
                            type="file"
                            aria-label="Choose a clinical document file (.json, .xml, .hl7)"
                            accept=".json,.xml,.ccda,.hl7,.txt,application/json,application/fhir+json,application/xml,text/xml,text/plain"
                            on:change=on_upload_fhir_file
                            class="block w-full text-xs text-slate-300 file:mr-3 file:rounded-full file:border-0 file:bg-emerald-500 file:px-3.5 file:py-1.5 file:text-xs file:font-semibold file:text-slate-950 hover:file:bg-emerald-400"
                        />
                    </label>
                    <form on:submit=on_paste_fhir_import class="space-y-2">
                        <textarea
                            rows="2"
                            aria-label="Paste FHIR R4 JSON, Kipu JSON, C-CDA XML, HL7 v2, or shlink:/ URI"
                            placeholder="Paste FHIR R4 JSON, Kipu JSON, C-CDA XML, HL7 v2 (`MSH|...`), or `shlink:/...` URI (zero data leaves your device)..."
                            prop:value=move || fhir_paste.get()
                            on:input=move |ev| fhir_paste.set(event_target_value(&ev))
                            class="w-full rounded-xl border border-white/15 bg-slate-950 p-2.5 font-mono text-[11px] text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                        />
                        <div class="flex flex-wrap items-center gap-2">
                            <button
                                type="submit"
                                class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-4 py-1.5 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/25"
                            >
                                "Auto-Detect & Merge Document"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| {
                                    let sample = patient_snapshot.with_value(sandbox_kipu_payload_for_patient);
                                    complete_portal_import(
                                        "org-kipu-sandbox".into(),
                                        "Harborview Behavioral (KipuEMR Sandbox)".into(),
                                        PortalVendor::KipuEmr,
                                        "https://api.kipuworks.com".into(),
                                        sample,
                                        Some(PortalConnectionMode::KipuApiV3),
                                    );
                                }
                                class="rounded-full border border-amber-400/40 bg-amber-500/10 px-3 py-1 text-[11px] font-medium text-amber-200 hover:bg-amber-500/20"
                            >
                                "+ Kipu Sandbox (Part 2)"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| {
                                    let sample = patient_snapshot.with_value(sandbox_ccda_xml_for_patient);
                                    complete_portal_import(
                                        "org-ccda-sandbox".into(),
                                        "C-CDA R2.1 CCD Sample".into(),
                                        PortalVendor::SimplePractice,
                                        "local://ccda-sample.xml".into(),
                                        sample,
                                        Some(PortalConnectionMode::ImportedCcdaXml),
                                    );
                                }
                                class="rounded-full border border-sky-400/40 bg-sky-500/10 px-3 py-1 text-[11px] font-medium text-sky-200 hover:bg-sky-500/20"
                            >
                                "+ C-CDA XML Sample"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| {
                                    let sample = patient_snapshot.with_value(sandbox_hl7v2_message_for_patient);
                                    complete_portal_import(
                                        "org-hl7v2-sandbox".into(),
                                        "Regional Lab HL7 v2.5.1 ORU".into(),
                                        PortalVendor::Meditech,
                                        "local://oru-r01.hl7".into(),
                                        sample,
                                        Some(PortalConnectionMode::ImportedHl7V2),
                                    );
                                }
                                class="rounded-full border border-sky-400/40 bg-sky-500/10 px-3 py-1 text-[11px] font-medium text-sky-200 hover:bg-sky-500/20"
                            >
                                "+ HL7 v2 Sample"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| {
                                    let uri = patient_snapshot.with_value(sandbox_smart_health_link_uri);
                                    complete_portal_import(
                                        "org-shlink-sandbox".into(),
                                        "SMART Health Link Sandbox".into(),
                                        PortalVendor::CustomSmartFhir,
                                        "https://shl-sandbox.dabney-demo.invalid".into(),
                                        uri,
                                        Some(PortalConnectionMode::SmartHealthLink),
                                    );
                                }
                                class="rounded-full border border-cyan-400/40 bg-cyan-500/10 px-3 py-1 text-[11px] font-medium text-cyan-200 hover:bg-cyan-500/20"
                            >
                                "Inspect shlink:/ Sample"
                            </button>
                        </div>
                    </form>
                </section>
            </div>
        </div>
    }
}

/// Patient-Owned Portable Shareable Health Passport File section.
///
/// Generates and persists a verified, self-contained file the patient can share
/// with any doctor, specialist, ER, family member, or caregiver—without needing
/// a shared EHR or repeating their medical history—and supports full round-trip
/// file reload on any device.
#[component]
pub fn PortablePassportSection(
    patient: PatientChart,
    state: PatientDemoState,
    allergies: Vec<AllergyEntry>,
    linked_portals: Vec<LinkedPortalConnection>,
    audience: RwSignal<PassportAudience>,
    emergency_note: RwSignal<String>,
    #[prop(optional)] reset_tick: Option<RwSignal<u32>>,
    on_restore_passport: Callback<HealthPassport>,
) -> impl IntoView {
    let locale = use_locale();
    let patient_sv = StoredValue::new(patient);
    let state_sv = StoredValue::new(state);
    let allergies_sv = StoredValue::new(allergies);
    let portals_sv = StoredValue::new(linked_portals);

    let copy_status = RwSignal::new(Option::<String>::None);
    let import_status = RwSignal::new(Option::<String>::None);
    let import_error = RwSignal::new(Option::<String>::None);
    let show_waiting_room_card = RwSignal::new(false);
    let clinic_english_handout = RwSignal::new(true);
    // Hard 42 CFR Part 2 invariants: SUD records AND § 2.31(b) counseling notes
    // are strictly excluded by default unless separately opted in.
    let include_part2_sud = RwSignal::new(false);
    let include_part2_counseling = RwSignal::new(false);

    if let Some(tick) = reset_tick {
        Effect::new(move |prev: Option<u32>| {
            let current = tick.get();
            if prev.is_some() && prev != Some(current) {
                include_part2_sud.set(false);
                include_part2_counseling.set(false);
                copy_status.set(None);
                import_status.set(None);
                import_error.set(None);
                show_waiting_room_card.set(false);
                clinic_english_handout.set(true);
            }
            current
        });
    }

    let active_passport = Memo::new(move |_| {
        let aud = audience.get();
        let em = emergency_note.get();
        let inc_part2 = include_part2_sud.get();
        let inc_counseling = inc_part2 && include_part2_counseling.get();
        patient_sv.with_value(|p| {
            state_sv.with_value(|st| {
                allergies_sv.with_value(|algs| {
                    portals_sv.with_value(|lp| {
                        build_health_passport_with_options(
                            p,
                            st,
                            algs,
                            lp,
                            aud,
                            &em,
                            DEMO_TIMESTAMP,
                            PassportExportOptions {
                                include_part2_sud_records: inc_part2,
                                include_part2_sud_counseling_notes: inc_counseling,
                            },
                        )
                    })
                })
            })
        })
    });

    let on_upload_passport = move |ev: leptos::ev::Event| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            use wasm_bindgen::{closure::Closure, JsCast};
            let Some(input) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                return;
            };
            let Ok(reader) = web_sys::FileReader::new() else {
                import_error.set(Some("Browser FileReader unavailable.".into()));
                return;
            };
            let reader_cb = reader.clone();
            let onload = Closure::once(move |_: web_sys::ProgressEvent| {
                if let Ok(val) = reader_cb.result() {
                    if let Some(text) = val.as_string() {
                        match parse_health_passport_json(&text) {
                            Ok(restored) => {
                                import_error.set(None);
                                import_status.set(Some(format!(
                                    "Verified SHA-256 & restored Health Passport for {} ({} record(s), {} med(s)).",
                                    restored.owner_name,
                                    restored.records.len(),
                                    restored.medications.len()
                                )));
                                on_restore_passport.run(restored);
                            }
                            Err(err) => {
                                import_status.set(None);
                                import_error.set(Some(err));
                            }
                        }
                    }
                }
            });
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            let _ = reader.read_as_text(&file);
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        {
            let _ = (ev, on_restore_passport);
        }
    };

    let copy_markdown_summary = move |_| {
        let md = render_passport_markdown(&active_passport.get_untracked());
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            use js_sys::{Function, Reflect};
            use wasm_bindgen::{JsCast, JsValue};
            let copied = web_sys::window()
                .and_then(|w| Reflect::get(w.as_ref(), &JsValue::from_str("navigator")).ok())
                .and_then(|nav| Reflect::get(&nav, &JsValue::from_str("clipboard")).ok())
                .and_then(|clip| {
                    let write = Reflect::get(&clip, &JsValue::from_str("writeText")).ok()?;
                    let func = write.dyn_ref::<Function>()?;
                    func.call1(&clip, &JsValue::from_str(&md)).ok()
                })
                .is_some();
            if copied {
                copy_status.set(Some(
                    "Copied Markdown Health Passport summary to clipboard.".into(),
                ));
            } else {
                copy_status.set(Some(
                    "Clipboard unavailable in this browser context—expand the Markdown preview below to copy.".into(),
                ));
            }
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        {
            let _ = md;
            copy_status.set(Some("Markdown summary ready.".into()));
        }
    };

    view! {
        <section
            data-portable-health-passport="true"
            class="rounded-2xl border border-cyan-400/35 bg-gradient-to-br from-[#01081b] via-slate-900/90 to-slate-950 p-5 space-y-5 shadow-lg shadow-cyan-500/5"
        >
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-white/10 pb-3.5">
                <div class="max-w-2xl">
                    <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                        {move || t(locale.get(), Msg::PassportEyebrow)}
                    </p>
                    <h2 class="mt-1 text-lg font-bold text-white">
                        {move || t(locale.get(), Msg::PassportHeading)}
                    </h2>
                    <p class="mt-1 text-xs leading-relaxed text-slate-300">
                        {move || t(locale.get(), Msg::PassportSubtitle)}
                    </p>
                </div>
                <label class="inline-flex min-h-[40px] shrink-0 cursor-pointer items-center gap-2 rounded-full border border-emerald-400/45 bg-emerald-500/15 px-3.5 py-1.5 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/25">
                    <span>{move || t(locale.get(), Msg::PassportOpenSaved)}</span>
                    <input
                        type="file"
                        aria-label="Open my saved Health Passport JSON file"
                        accept=".json,application/json"
                        on:change=on_upload_passport
                        class="sr-only"
                    />
                </label>
            </div>

            // Step 1: Visit-Tailored Recipient Preset Selector
            <div class="space-y-2">
                <p class="text-[11px] font-semibold uppercase tracking-wider text-cyan-200">
                    {move || t(locale.get(), Msg::PassportWhoSeeing)}
                </p>
                <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                    {PassportAudience::all()
                        .iter()
                        .copied()
                        .map(|preset| {
                            view! {
                                <button
                                    type="button"
                                    aria-pressed=move || (audience.get() == preset).to_string()
                                    on:click=move |_| audience.set(preset)
                                    class=move || if audience.get() == preset {
                                        "rounded-xl border border-cyan-400/60 bg-cyan-500/15 p-3 text-start transition"
                                    } else {
                                        "rounded-xl border border-white/10 bg-slate-950/60 p-3 text-start transition hover:border-white/25"
                                    }
                                >
                                    <p class="text-xs font-semibold text-white">{preset.short_label()}</p>
                                    <p class="mt-1 text-[11px] leading-snug text-slate-300">
                                        {preset.description()}
                                    </p>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                {move || audience.get().suggests_part2_review().then(|| view! {
                    <p class="rounded-xl border border-violet-400/35 bg-violet-500/10 px-3.5 py-2 text-xs text-violet-100">
                        {move || t(locale.get(), Msg::PassportPart2BehavioralHint)}
                    </p>
                })}
            </div>

            // Step 2: Optional Emergency / Quick Context Note & 42 CFR Part 2 Privacy Controls
            <div class="space-y-2.5">
                <label class="block text-xs font-medium text-slate-300">
                    "Step 2 · Emergency / Quick-Glance Instructions (shown at top of handout)"
                    <input
                        type="text"
                        placeholder="e.g., Emergency contact, preferred pharmacy, key allergy or medication note (no real PHI in demo)..."
                        prop:value=move || emergency_note.get()
                        on:input=move |ev| emergency_note.set(event_target_value(&ev))
                        class="mt-1.5 w-full rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2 text-xs text-white placeholder:text-slate-400 focus:border-cyan-400/60 focus:outline-none"
                    />
                </label>

                <label class="flex cursor-pointer items-start gap-2.5 rounded-xl border border-amber-400/30 bg-amber-500/[0.06] p-3 text-xs text-amber-100">
                    <input
                        type="checkbox"
                        prop:checked=move || include_part2_sud.get()
                        on:change=move |ev| {
                            let checked = event_target_checked(&ev);
                            include_part2_sud.set(checked);
                            if !checked {
                                include_part2_counseling.set(false);
                            }
                        }
                        class="mt-0.5 accent-amber-400"
                    />
                    <span class="space-y-0.5">
                        <strong class="block text-amber-200">
                            "Explicit 42 CFR Part 2 Opt-In (Substance Use Disorder Records — Default: Excluded)"
                        </strong>
                        <span class="block text-[11px] leading-relaxed text-amber-100/90">
                            "Any 42 CFR Part 2-tagged records (e.g., KipuEMR behavioral health imports) are withheld from your Portable Health Passport by default. Checking this box includes general SUD records (still excluding separate § 2.31(b) counseling notes) and embeds the statutory 42 CFR § 2.32 Prohibition on Redisclosure notice across JSON, HTML, Markdown, and FHIR R4 IPS exports."
                        </span>
                    </span>
                </label>

                <label class=move || if include_part2_sud.get() {
                    "flex cursor-pointer items-start gap-2.5 rounded-xl border border-rose-400/30 bg-rose-500/[0.06] p-3 text-xs text-rose-100"
                } else {
                    "flex cursor-not-allowed items-start gap-2.5 rounded-xl border border-white/10 bg-slate-950/40 p-3 text-xs text-slate-400 opacity-75"
                }>
                    <input
                        type="checkbox"
                        disabled=move || !include_part2_sud.get()
                        prop:checked=move || include_part2_sud.get() && include_part2_counseling.get()
                        on:change=move |ev| {
                            if include_part2_sud.get_untracked() {
                                include_part2_counseling.set(event_target_checked(&ev));
                            } else {
                                include_part2_counseling.set(false);
                            }
                        }
                        class="mt-0.5 accent-rose-400"
                    />
                    <span class="space-y-0.5">
                        <strong class=move || if include_part2_sud.get() { "block text-rose-200" } else { "block text-slate-300" }>
                            "Separate 42 CFR § 2.31(b) Opt-In (SUD Counseling Notes — Default: Excluded)"
                        </strong>
                        <span class="block text-[11px] leading-relaxed">
                            {move || if include_part2_sud.get() {
                                "Separately maintained SUD counseling notes are never included under a general Part 2 export and require this separate explicit opt-in."
                            } else {
                                "Enable general 42 CFR Part 2 opt-in above first. Separately maintained § 2.31(b) SUD counseling notes remain strictly withheld until both boxes are checked."
                            }}
                        </span>
                    </span>
                </label>
            </div>

            // Step 3: Live Export Payload, 1-Page Waiting-Room Handout & Actions
            {move || {
                let passport = active_passport.get();
                let slug = passport.file_slug();
                let short_sha: String = passport.integrity_sha256.chars().take(16).collect();
                let json_body = serde_json::to_string_pretty(&passport).unwrap_or_default();
                let one_page_body = render_passport_one_page_html(&passport);
                let html_body = render_passport_html(&passport);
                let ips_body = serde_json::to_string_pretty(&render_passport_fhir_ips_bundle(&passport))
                    .unwrap_or_default();
                let md_body = render_passport_markdown(&passport);

                let one_page_href = data_download_uri("text/html", &one_page_body);
                let html_href = data_download_uri("text/html", &html_body);
                let json_href = data_download_uri("application/json", &json_body);
                let ips_href = data_download_uri("application/fhir+json", &ips_body);
                let waiting_card_passport = passport.clone();

                view! {
                    <div class="space-y-4">
                        <div class="grid gap-2 sm:grid-cols-4 text-xs">
                            <div class="rounded-xl border border-white/10 bg-slate-950/65 p-3">
                                <p class="text-slate-400">"Records in file"</p>
                                <p class="mt-0.5 text-base font-bold text-emerald-300">
                                    {passport.records.len()}
                                </p>
                            </div>
                            <div class="rounded-xl border border-white/10 bg-slate-950/65 p-3">
                                <p class="text-slate-400">"Meds & allergies"</p>
                                <p class="mt-0.5 text-base font-bold text-cyan-300">
                                    {format!("{} · {}", passport.medications.len(), passport.allergies.len())}
                                </p>
                            </div>
                            <div class="rounded-xl border border-white/10 bg-slate-950/65 p-3">
                                <p class="text-slate-400">"Private notes excluded"</p>
                                <p class="mt-0.5 text-base font-bold text-white">
                                    {passport.excluded_private_note_count}
                                </p>
                            </div>
                            <div class="rounded-xl border border-white/10 bg-slate-950/65 p-3">
                                <p class="text-slate-400">"Things to know up front"</p>
                                <p class="mt-0.5 text-base font-bold text-cyan-200">
                                    {passport.things_to_know.len()}
                                </p>
                            </div>
                        </div>

                        // Accurate 42 CFR Part 2 status banner: shows BOTH the § 2.32 notice
                        // confirmation AND any still-withheld Part 2 / § 2.31(b) records!
                        {(passport.part2_sud_included || passport.excluded_part2_record_count > 0).then(|| view! {
                            <div class="rounded-xl border border-amber-400/35 bg-amber-500/10 px-3.5 py-2.5 text-xs text-amber-100 space-y-1">
                                {passport.part2_sud_included.then(|| view! {
                                    <p class="font-medium text-amber-200">
                                        "42 CFR § 2.32 Prohibition on Redisclosure notice embedded in JSON, HTML, Markdown, and FHIR R4 IPS exports."
                                    </p>
                                })}
                                {(passport.excluded_part2_record_count > 0).then(|| view! {
                                    <p>
                                        {if passport.excluded_part2_counseling_note_count > 0
                                            && passport.excluded_part2_counseling_note_count == passport.excluded_part2_record_count
                                        {
                                            format!(
                                                "42 CFR § 2.31(b) protection active: {} separate SUD counseling note(s) remain withheld from this export.",
                                                passport.excluded_part2_counseling_note_count
                                            )
                                        } else {
                                            format!(
                                                "42 CFR Part 2 protection active: {} SUD-tagged record(s) withheld from this export.",
                                                passport.excluded_part2_record_count
                                            )
                                        }}
                                    </p>
                                })}
                            </div>
                        })}

                        // Clinic English Handout toggle when patient UI is in a non-English language (Wave 4)
                        {move || (locale.get() != Locale::En).then(|| view! {
                            <label class="flex cursor-pointer items-center gap-2.5 rounded-xl border border-cyan-400/30 bg-cyan-500/[0.06] px-3.5 py-2.5 text-xs text-cyan-100">
                                <input
                                    type="checkbox"
                                    prop:checked=move || clinic_english_handout.get()
                                    on:change=move |ev| clinic_english_handout.set(event_target_checked(&ev))
                                    class="accent-cyan-400"
                                />
                                <span>
                                    {move || t(locale.get(), Msg::PassportClinicEnglishToggle)}
                                    <span class="ms-1.5 text-[11px] text-slate-300">
                                        {move || if clinic_english_handout.get() {
                                            "(English handout active for U.S. clinic intake)"
                                        } else {
                                            "(Using standard portable handout format)"
                                        }}
                                    </span>
                                </span>
                            </label>
                        })}

                        <div class="flex flex-wrap gap-2.5">
                            <a
                                href=one_page_href
                                download=format!("dabney-1page-handout-{slug}.html")
                                class="inline-flex min-h-[42px] items-center gap-1.5 rounded-full bg-emerald-500 px-4 py-2 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                            >
                                {move || t(locale.get(), Msg::PassportOnePageHandout)}
                            </a>
                            <button
                                type="button"
                                aria-pressed=move || show_waiting_room_card.get().to_string()
                                on:click=move |_| show_waiting_room_card.update(|v| *v = !*v)
                                class="min-h-[42px] rounded-full border border-emerald-400/45 bg-emerald-500/15 px-4 py-2 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/25"
                            >
                                {move || if show_waiting_room_card.get() {
                                    t(locale.get(), Msg::PassportWaitingRoomHide)
                                } else {
                                    t(locale.get(), Msg::PassportWaitingRoomShow)
                                }}
                            </button>
                            <a
                                href=html_href
                                download=format!("dabney-health-brief-{slug}.html")
                                class="inline-flex min-h-[42px] items-center gap-1.5 rounded-full border border-emerald-400/35 bg-white/5 px-4 py-2 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/15"
                            >
                                {move || t(locale.get(), Msg::PassportPrintableBrief)}
                            </a>
                            <a
                                href=json_href
                                download=format!("dabney-health-passport-{slug}.json")
                                class="inline-flex min-h-[42px] items-center gap-1.5 rounded-full bg-cyan-500 px-4 py-2 text-xs font-semibold text-slate-950 transition hover:bg-cyan-400"
                            >
                                {move || t(locale.get(), Msg::PassportSaveBackup)}
                            </a>
                            <button
                                type="button"
                                on:click=copy_markdown_summary
                                class="min-h-[42px] rounded-full border border-white/20 bg-white/5 px-4 py-2 text-xs font-semibold text-slate-100 transition hover:bg-white/10"
                            >
                                {move || t(locale.get(), Msg::PassportCopySummary)}
                            </button>
                        </div>

                        {move || copy_status.get().map(|msg| view! {
                            <p role="status" class="text-xs text-emerald-300">{msg}</p>
                        })}

                        // High-Contrast On-Screen 1-Page Waiting-Room Card (hand phone to nurse/doctor)
                        {move || show_waiting_room_card.get().then(|| {
                            let card = waiting_card_passport.clone();
                            view! {
                                <section
                                    data-waiting-room-card="true"
                                    dir="ltr"
                                    class="rounded-2xl border-2 border-emerald-400/60 bg-white p-5 text-slate-900 shadow-xl space-y-3"
                                >
                                    <div class="flex flex-wrap items-start justify-between gap-2 border-b-2 border-emerald-700 pb-2.5">
                                        <div>
                                            <p class="text-[11px] font-bold uppercase tracking-wider text-emerald-800">
                                                {format!("Waiting-Room Health Summary · {}", card.audience.label())}
                                            </p>
                                            <h3 class="text-lg font-bold text-slate-950">{card.owner_name.clone()}</h3>
                                            <p class="text-xs text-slate-600">
                                                {format!("DOB: {} · ID: {} · Focus: {}", card.birth_date, card.identifier, card.primary_focus)}
                                            </p>
                                        </div>
                                        <span class="rounded-full bg-emerald-100 px-2.5 py-1 text-[11px] font-semibold text-emerald-900">
                                            "Synthetic Demo · Zero PHI"
                                        </span>
                                    </div>

                                    {(!card.things_to_know.is_empty()).then(|| view! {
                                        <div class="rounded-xl border border-teal-300 bg-teal-50 p-3 text-xs">
                                            <p class="font-bold uppercase tracking-wider text-teal-900">"Key Context (Things to Know Up Front)"</p>
                                            <ul class="mt-1 list-disc ps-5 space-y-0.5 text-slate-800">
                                                {card.things_to_know.iter().map(|pt| view! { <li>{pt.clone()}</li> }).collect_view()}
                                            </ul>
                                        </div>
                                    })}

                                    {(!card.emergency_note.is_empty()).then(|| view! {
                                        <div class="rounded-xl border border-amber-400 bg-amber-50 p-3 text-xs">
                                            <p class="font-bold uppercase tracking-wider text-amber-900">"Emergency / Quick Instructions"</p>
                                            <p class="mt-1 text-slate-800">{card.emergency_note.clone()}</p>
                                        </div>
                                    })}

                                    <div class="grid gap-3 md:grid-cols-2 text-xs">
                                        <div class="rounded-xl border border-slate-200 p-3">
                                            <p class="font-bold uppercase tracking-wider text-rose-800">"Allergies & Intolerances"</p>
                                            <ul class="mt-1 list-disc ps-5 space-y-0.5 text-slate-800">
                                                {if card.allergies.is_empty() {
                                                    view! { <li>"None documented"</li> }.into_any()
                                                } else {
                                                    card.allergies.iter().map(|a| view! {
                                                        <li><strong>{a.substance.clone()}</strong>{format!(" — {}", a.reaction)}</li>
                                                    }).collect_view().into_any()
                                                }}
                                            </ul>
                                        </div>
                                        <div class="rounded-xl border border-slate-200 p-3">
                                            <p class="font-bold uppercase tracking-wider text-slate-700">"Active Medications"</p>
                                            <ul class="mt-1 list-disc ps-5 space-y-0.5 text-slate-800">
                                                {card.medications.iter().map(|m| view! {
                                                    <li><strong>{format!("{} {}", m.name, m.dose)}</strong>{format!(" ({})", m.schedule)}</li>
                                                }).collect_view()}
                                            </ul>
                                        </div>
                                    </div>

                                    {(!card.visit_questions.is_empty()).then(|| view! {
                                        <div class="rounded-xl border border-slate-200 p-3 text-xs">
                                            <p class="font-bold uppercase tracking-wider text-slate-700">"Top Questions for Today's Visit"</p>
                                            <ol class="mt-1 list-decimal ps-5 space-y-0.5 text-slate-800">
                                                {card.visit_questions.iter().map(|q| view! { <li>{q.clone()}</li> }).collect_view()}
                                            </ol>
                                        </div>
                                    })}
                                </section>
                            }
                        })}

                        // Restore a previously saved portable passport file
                        <div class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-xs">
                            <div>
                                <p class="font-semibold text-white">
                                    "Re-Open a Saved Health Passport File (.json)"
                                </p>
                                <p class="text-[11px] text-slate-300">
                                    "Load a previously persisted Health Passport file with automatic integrity verification (42 CFR Part 2 records stay protected)."
                                </p>
                            </div>
                            <label class="shrink-0">
                                <span class="sr-only">"Load saved Health Passport JSON file"</span>
                                <input
                                    type="file"
                                    aria-label="Load saved Health Passport JSON file"
                                    accept=".json,application/json"
                                    on:change=on_upload_passport
                                    class="block text-xs text-slate-300 file:mr-2 file:rounded-full file:border file:border-white/20 file:bg-white/10 file:px-3 file:py-1.5 file:text-xs file:font-semibold file:text-white hover:file:bg-white/15"
                                />
                            </label>
                        </div>

                        {move || import_status.get().map(|msg| view! {
                            <p role="status" class="rounded-xl border border-emerald-400/35 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-100">{msg}</p>
                        })}
                        {move || import_error.get().map(|err| view! {
                            <p role="alert" class="rounded-xl border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">{err}</p>
                        })}

                        // Progressive Disclosure Drawer for Technical / Clinic Standards (Wave 3)
                        <details class="rounded-xl border border-white/10 bg-slate-950/65 px-3.5 py-2.5 text-xs">
                            <summary class="cursor-pointer select-none font-semibold text-cyan-300 hover:text-cyan-200">
                                {move || t(locale.get(), Msg::PassportTechnicalDrawer)}
                            </summary>
                            <div class="mt-3 space-y-3 border-t border-white/10 pt-3">
                                <div class="flex flex-wrap items-center justify-between gap-2">
                                    <div class="flex flex-wrap items-center gap-2 font-mono text-[11px] text-slate-300">
                                        <span class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-2.5 py-0.5 text-emerald-200">
                                            "dabney.health_passport.v1"
                                        </span>
                                        <span>{format!("SHA-256: {short_sha}…")}</span>
                                    </div>
                                    <a
                                        href=ips_href
                                        download=format!("dabney-ips-{slug}.fhir.json")
                                        class="inline-flex min-h-[36px] items-center gap-1.5 rounded-full border border-cyan-400/40 bg-cyan-500/10 px-3.5 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                                    >
                                        "Download FHIR R4 IPS (.fhir.json)"
                                    </a>
                                </div>
                                <div>
                                    <p class="mb-1.5 text-[11px] font-semibold text-slate-300">
                                        "Exact Shareable Summary Text (Markdown)"
                                    </p>
                                    <pre dir="ltr" class="max-h-72 overflow-auto whitespace-pre-wrap rounded-lg border border-white/10 bg-slate-950 p-3 font-mono text-[11px] leading-relaxed text-slate-200">
                                        {md_body}
                                    </pre>
                                </div>
                            </div>
                        </details>
                    </div>
                }
            }}
        </section>
    }
}

/// Automated HIPAA Release of Information (ROI) & 45 CFR § 164.524 Right of
/// Access panel for `/clinical/patient`.
#[component]
pub fn AutomatedRoiPanel(
    patient: PatientChart,
    roi_map: RwSignal<HashMap<String, Vec<RoiRequest>>>,
    #[prop(optional)] reset_tick: Option<RwSignal<u32>>,
) -> impl IntoView {
    let locale = use_locale();
    let patient_id = StoredValue::new(patient.id.clone());
    let patient_name = StoredValue::new(patient.display_name.clone());
    let patient_dob = StoredValue::new(patient.birth_date.clone());

    // Builder state for queuing a new clinic ROI request
    let clinic_name = RwSignal::new(String::new());
    let him_email = RwSignal::new(String::new());
    let him_fax = RwSignal::new(String::new());
    let him_address = RwSignal::new(String::new());
    let legal_basis = RwSignal::new(RoiLegalBasis::HipaaRightOfAccess164524);
    let delivery_channel = RwSignal::new(RoiDeliveryChannel::DirectEmail);
    let electronic_format = RwSignal::new(RoiElectronicFormat::FhirR4Json);
    let date_range_choice = RwSignal::new("last_3_years".to_string());
    let recipient_is_third_party = RwSignal::new(false);
    let recipient_name = RwSignal::new(String::new());
    let recipient_dest = RwSignal::new("patient-demo@dabney-demo.invalid".to_string());
    let include_sensitive = RwSignal::new(false);
    let include_part2_sud_cat = RwSignal::new(false);
    let include_part2_counseling = RwSignal::new(false);
    let part2_specific_desc = RwSignal::new(
        "Outpatient SUD admission summary, medication orders, vitals, and lab results (excluding separate SUD counseling notes)."
            .to_string(),
    );
    let selected_vendor = RwSignal::new(Option::<PortalVendor>::None);
    let selected_categories = RwSignal::new(RoiRecordCategory::standard_default().to_vec());
    let signer_name = RwSignal::new(patient.display_name);
    let next_seq = RwSignal::new(3usize);

    let batch_report = RwSignal::new(Option::<RoiBatchDispatchReport>::None);
    let roi_notice = RwSignal::new(Option::<String>::None);
    let roi_error = RwSignal::new(Option::<String>::None);
    let card_sign_errors = RwSignal::new(HashMap::<String, String>::new());

    if let Some(tick) = reset_tick {
        Effect::new(move |prev: Option<u32>| {
            let current = tick.get();
            if prev.is_some() && prev != Some(current) {
                clinic_name.set(String::new());
                him_email.set(String::new());
                him_fax.set(String::new());
                him_address.set(String::new());
                legal_basis.set(RoiLegalBasis::HipaaRightOfAccess164524);
                include_sensitive.set(false);
                include_part2_sud_cat.set(false);
                include_part2_counseling.set(false);
                selected_vendor.set(None);
                batch_report.set(None);
                roi_notice.set(None);
                roi_error.set(None);
                card_sign_errors.set(HashMap::new());
            }
            current
        });
    }

    let prefill_from_org = move |org_id: String| {
        if let Some(org) = find_portal_organization(&org_id) {
            let is_part2 = org.vendor.is_part2_sensitive() || org.transport.part2_sensitive;
            clinic_name.set(format!("{} — Health Information Management", org.name));
            him_email.set(org.him_email.unwrap_or_default());
            him_fax.set(org.him_fax.unwrap_or_default());
            him_address.set(org.him_address.unwrap_or_default());
            selected_vendor.set(Some(org.vendor));
            if is_part2 {
                legal_basis.set(RoiLegalBasis::Part2SudAuthorization42Cfr2);
                include_part2_sud_cat.set(true);
            } else if legal_basis.get_untracked().is_part2_sud() {
                legal_basis.set(RoiLegalBasis::HipaaRightOfAccess164524);
                include_part2_sud_cat.set(false);
            }
        }
    };

    let toggle_category = move |cat: RoiRecordCategory| {
        selected_categories.update(|list| {
            if let Some(pos) = list.iter().position(|c| *c == cat) {
                list.remove(pos);
            } else {
                list.push(cat);
            }
        });
    };

    let add_roi_request = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        roi_error.set(None);
        let mut cats = selected_categories.get_untracked();
        if include_sensitive.get_untracked()
            && !cats.contains(&RoiRecordCategory::SensitivePsychotherapyAndBehavioral)
        {
            cats.push(RoiRecordCategory::SensitivePsychotherapyAndBehavioral);
        }
        let basis = legal_basis.get_untracked();
        if (basis.is_part2_sud() || include_part2_sud_cat.get_untracked())
            && !cats.contains(&RoiRecordCategory::Part2SubstanceUseDisorder)
        {
            cats.push(RoiRecordCategory::Part2SubstanceUseDisorder);
        }
        if include_part2_counseling.get_untracked()
            && !cats.contains(&RoiRecordCategory::Part2SudCounselingNotes)
        {
            cats.push(RoiRecordCategory::Part2SudCounselingNotes);
        }
        let range = match date_range_choice.get_untracked().as_str() {
            "last_12_months" => RoiDateRange::Last12Months,
            "all" => RoiDateRange::AllAvailableHistory,
            _ => RoiDateRange::Last3Years,
        };
        let recipient = if recipient_is_third_party.get_untracked() {
            RoiRecipient::ThirdParty {
                recipient_name: recipient_name.get_untracked().trim().to_string(),
                relationship_or_clinic: "Designated Doctor / Recipient".into(),
                destination: recipient_dest.get_untracked().trim().to_string(),
            }
        } else {
            RoiRecipient::PatientSelf {
                delivery_destination: recipient_dest.get_untracked().trim().to_string(),
            }
        };

        let seq = next_seq.get_untracked();
        next_seq.set(seq + 1);
        let pid = patient_id.get_value();
        let clinic_str = clinic_name.get_untracked().trim().to_string();
        let is_part2 = basis.is_part2_sud()
            || include_part2_sud_cat.get_untracked()
            || include_part2_counseling.get_untracked();

        let draft = RoiRequest {
            id: format!("roi-{pid}-{seq}"),
            patient_id: pid.clone(),
            patient_name: patient_name.get_value(),
            patient_birth_date: patient_dob.get_value(),
            patient_contact_email: "patient-demo@dabney-demo.invalid".into(),
            clinic_name: clinic_str.clone(),
            clinic_him_email: him_email.get_untracked().trim().to_string(),
            clinic_him_fax: him_fax.get_untracked().trim().to_string(),
            clinic_address: him_address.get_untracked().trim().to_string(),
            clinic_portal_vendor: selected_vendor.get_untracked(),
            legal_basis: basis,
            delivery_channel: delivery_channel.get_untracked(),
            electronic_format: electronic_format.get_untracked(),
            date_range: range,
            categories: cats,
            recipient,
            expiration_date: "2027-02-24".into(),
            purpose_note: if is_part2 {
                "At the request of the patient for continuity of care (42 CFR Part 2 consent)."
                    .into()
            } else {
                "Patient-requested electronic medical records for continuity of care.".into()
            },
            attestation_accepted: false,
            status: RoiStatus::Draft,
            part2_disclosing_program: if is_part2 { clinic_str } else { String::new() },
            part2_specific_description: if is_part2 {
                part2_specific_desc.get_untracked().trim().to_string()
            } else {
                String::new()
            },
            part2_tpo_redisclosure_acknowledged: false,
            part2_separate_counseling_notes_consent: include_part2_counseling.get_untracked(),
        };

        if let Err(err) = clinical_workspace::validate_roi_request(&draft) {
            roi_error.set(Some(err));
            return;
        }

        roi_map.update(|map| {
            map.entry(pid).or_default().insert(0, draft);
        });
        clinic_name.set(String::new());
        him_email.set(String::new());
        him_fax.set(String::new());
        him_address.set(String::new());
        selected_vendor.set(None);
        include_sensitive.set(false);
        include_part2_sud_cat.set(false);
        include_part2_counseling.set(false);
        roi_notice.set(Some(
            "Added new ROI draft below. Review, check the required attestation(s), sign, and dispatch in batch.".into(),
        ));
    };

    let run_batch_dispatch = move |_| {
        roi_error.set(None);
        let pid = patient_id.get_value();
        let mut outcome: Option<RoiBatchDispatchReport> = None;
        roi_map.update(|map| {
            if let Some(list) = map.get_mut(&pid) {
                outcome = Some(dispatch_roi_batch(
                    list,
                    DEMO_TIMESTAMP,
                    DEMO_DUE_30D,
                    "Automated ROI dispatch (local zero-PHI sandbox; configure ROI_DISPATCH_WEBHOOK_URL on server for live HIM email/e-fax relay)",
                ));
            }
        });
        if let Some(rep) = outcome {
            if rep.total_queued == 0 {
                roi_error.set(Some(
                    "No signed ROI forms are queued yet. Sign at least one draft below before running batch automation.".into(),
                ));
            } else {
                roi_notice.set(Some(format!(
                    "Automated batch dispatch completed: {}/{} signed ROI form(s) dispatched with 30-day statutory deadlines ({DEMO_DUE_30D}).",
                    rep.dispatched_count, rep.total_queued
                )));
                batch_report.set(Some(rep));
            }
        }
    };

    let directory_clinics = StoredValue::new({
        let all = portal_directory();
        let priority_ids = [
            "org-stanford-mychart",
            "org-ucsf-mychart",
            "org-kipu-sandbox",
            "org-simplepractice-clinic",
            "org-hint-dpc",
            "org-mayo-mychart",
        ];
        priority_ids
            .iter()
            .filter_map(|id| all.iter().find(|o| o.id == *id).cloned())
            .collect::<Vec<_>>()
    });

    view! {
        <div class="space-y-6" data-patient-roi-automation="true">
            // Batch Automation Overview & Trigger
            <section class="rounded-2xl border border-emerald-400/35 bg-gradient-to-br from-[#01081b] via-slate-900/85 to-slate-950 p-5 space-y-4">
                <div class="flex flex-wrap items-start justify-between gap-4">
                    <div class="max-w-2xl">
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "Automated Release of Information · 45 CFR § 164.524 & § 164.508"
                        </p>
                        <h2 class="mt-1 text-lg font-bold text-white">
                            {move || t(locale.get(), Msg::PassportRoiHeading)}
                        </h2>
                        <p class="mt-1 text-xs leading-relaxed text-slate-300">
                            "Generate signed, legally structured HIPAA Right of Access & Third-Party ROI packets for prior clinics and hospitals, dispatch every signed request in one batch, track the federal 30-day fulfillment clock, and generate Day-15 / Day-30 follow-up reminders automatically."
                        </p>
                    </div>
                    {move || {
                        let pid = patient_id.get_value();
                        let queued = roi_map.with(|m| {
                            m.get(&pid)
                                .map(|list| {
                                    list.iter()
                                        .filter(|r| matches!(r.status, RoiStatus::SignedReadyToSend { .. }))
                                        .count()
                                })
                                .unwrap_or(0)
                        });
                        view! {
                            <button
                                type="button"
                                on:click=run_batch_dispatch
                                class="min-h-[44px] shrink-0 rounded-full bg-emerald-500 px-5 py-2.5 text-xs font-semibold text-slate-950 shadow-lg shadow-emerald-500/20 transition hover:bg-emerald-400"
                            >
                                {format!("Automate Send All Signed ROI Forms ({queued}) →")}
                            </button>
                        }
                    }}
                </div>

                {move || roi_notice.get().map(|msg| view! {
                    <p role="status" class="rounded-xl border border-emerald-400/35 bg-emerald-500/10 px-3.5 py-2.5 text-xs text-emerald-100">
                        {msg}
                    </p>
                })}
                {move || roi_error.get().map(|err| view! {
                    <p role="alert" class="rounded-xl border border-rose-400/40 bg-rose-500/10 px-3.5 py-2.5 text-xs text-rose-200">
                        {err}
                    </p>
                })}
                {move || batch_report.get().map(|rep| view! {
                    <div class="rounded-xl border border-cyan-400/30 bg-cyan-500/[0.06] p-3.5 space-y-1.5 text-xs">
                        <p class="font-semibold text-cyan-200">
                            {format!("Latest Batch Dispatch Log ({} sent, {} skipped/failed)", rep.dispatched_count, rep.skipped_or_failed_count)}
                        </p>
                        <ul class="space-y-1">
                            {rep.items.into_iter().map(|item| view! {
                                <li class="text-slate-200">
                                    <span class="font-semibold text-white">{item.clinic_name}</span>
                                    " — "
                                    <span>{item.detail}</span>
                                </li>
                            }).collect_view()}
                        </ul>
                    </div>
                })}
            </section>

            // Queue a New Clinic ROI Request
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <div>
                        <h3 class="text-base font-semibold text-white">
                            "Add a Clinic / Hospital ROI Request"
                        </h3>
                        <p class="text-xs text-slate-300">
                            "Prefill HIM email & fax from a known health system or enter any doctor's office."
                        </p>
                    </div>
                </div>

                <div class="flex flex-wrap gap-1.5">
                    {directory_clinics.with_value(|orgs| {
                        orgs.iter()
                            .take(6)
                            .map(|org| {
                                let oid = org.id.clone();
                                let label = org.name.clone();
                                view! {
                                    <button
                                        type="button"
                                        on:click=move |_| prefill_from_org(oid.clone())
                                        class="rounded-full border border-white/15 bg-slate-950/70 px-3 py-1 text-[11px] text-slate-200 transition hover:border-cyan-400/45 hover:text-white"
                                    >
                                        {format!("+ {label}")}
                                    </button>
                                }
                            })
                            .collect_view()
                    })}
                </div>

                <form on:submit=add_roi_request class="space-y-4">
                    <div class="grid gap-3 sm:grid-cols-2">
                        <input
                            type="text"
                            aria-label="Releasing clinic or hospital name"
                            placeholder="Releasing clinic / hospital name *"
                            prop:value=move || clinic_name.get()
                            on:input=move |ev| clinic_name.set(event_target_value(&ev))
                            class="rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                        />
                        <input
                            type="email"
                            aria-label="Clinic HIM medical records email"
                            placeholder="HIM / Medical Records email (e.g., ROI@hospital.org)"
                            prop:value=move || him_email.get()
                            on:input=move |ev| him_email.set(event_target_value(&ev))
                            class="rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                        />
                        <input
                            type="text"
                            aria-label="Clinic HIM fax number"
                            placeholder="HIM Fax number (e.g., +1-650-725-9821)"
                            prop:value=move || him_fax.get()
                            on:input=move |ev| him_fax.set(event_target_value(&ev))
                            class="rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                        />
                        <input
                            type="text"
                            aria-label="Clinic address"
                            placeholder="Clinic mailing address (optional)"
                            prop:value=move || him_address.get()
                            on:input=move |ev| him_address.set(event_target_value(&ev))
                            class="rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none"
                        />
                    </div>

                    <div class="grid gap-3 sm:grid-cols-3 text-xs">
                        <label class="space-y-1 text-slate-300">
                            <span class="text-[11px] font-medium">"Legal Basis"</span>
                            <select
                                aria-label="HIPAA and 42 CFR Part 2 legal basis"
                                on:change=move |ev| {
                                    match event_target_value(&ev).as_str() {
                                        "164508" => {
                                            legal_basis.set(RoiLegalBasis::ThirdPartyAuthorization164508);
                                            recipient_is_third_party.set(true);
                                        }
                                        "part2" => {
                                            legal_basis.set(RoiLegalBasis::Part2SudAuthorization42Cfr2);
                                            include_part2_sud_cat.set(true);
                                        }
                                        _ => {
                                            legal_basis.set(RoiLegalBasis::HipaaRightOfAccess164524);
                                            recipient_is_third_party.set(false);
                                        }
                                    }
                                }
                                class="w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white"
                            >
                                <option
                                    value="164524"
                                    selected=move || legal_basis.get() == RoiLegalBasis::HipaaRightOfAccess164524
                                >
                                    "HIPAA Right of Access (45 CFR § 164.524)"
                                </option>
                                <option
                                    value="164508"
                                    selected=move || legal_basis.get() == RoiLegalBasis::ThirdPartyAuthorization164508
                                >
                                    "Third-Party ROI (45 CFR § 164.508)"
                                </option>
                                <option
                                    value="part2"
                                    selected=move || legal_basis.get() == RoiLegalBasis::Part2SudAuthorization42Cfr2
                                >
                                    "42 CFR Part 2 SUD Authorization (42 CFR § 2.31 & § 2.32)"
                                </option>
                            </select>
                        </label>
                        <label class="space-y-1 text-slate-300">
                            <span class="text-[11px] font-medium">"Dispatch Channel"</span>
                            <select
                                aria-label="ROI dispatch channel"
                                on:change=move |ev| {
                                    let next = match event_target_value(&ev).as_str() {
                                        "efax" => RoiDeliveryChannel::EFaxAutomation,
                                        "portal" => RoiDeliveryChannel::PatientPortalRequest,
                                        "mail" => RoiDeliveryChannel::PrintableMailPacket,
                                        _ => RoiDeliveryChannel::DirectEmail,
                                    };
                                    delivery_channel.set(next);
                                }
                                class="w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white"
                            >
                                <option value="email">"Automated HIM Email"</option>
                                <option value="efax">"Automated e-Fax Gateway"</option>
                                <option value="portal">"Patient Portal ROI Upload"</option>
                                <option value="mail">"Printable Signed PDF / Mail"</option>
                            </select>
                        </label>
                        <label class="space-y-1 text-slate-300">
                            <span class="text-[11px] font-medium">"Electronic Format & Date Range"</span>
                            <div class="flex gap-1.5">
                                <select
                                    aria-label="Electronic format"
                                    on:change=move |ev| {
                                        let next = match event_target_value(&ev).as_str() {
                                            "ccda" => RoiElectronicFormat::CcdaXml,
                                            "pdf" => RoiElectronicFormat::PdfElectronic,
                                            "any" => RoiElectronicFormat::AnyStandardElectronic,
                                            _ => RoiElectronicFormat::FhirR4Json,
                                        };
                                        electronic_format.set(next);
                                    }
                                    class="w-1/2 rounded-xl border border-white/15 bg-slate-950 px-2 py-2 text-xs text-white"
                                >
                                    <option value="fhir">"FHIR R4"</option>
                                    <option value="ccda">"C-CDA XML"</option>
                                    <option value="pdf">"PDF"</option>
                                    <option value="any">"Any e-copy"</option>
                                </select>
                                <select
                                    aria-label="Record date range"
                                    on:change=move |ev| date_range_choice.set(event_target_value(&ev))
                                    class="w-1/2 rounded-xl border border-white/15 bg-slate-950 px-2 py-2 text-xs text-white"
                                >
                                    <option value="last_3_years">"Last 3 yrs"</option>
                                    <option value="last_12_months">"Last 12 mo"</option>
                                    <option value="all">"All history"</option>
                                </select>
                            </div>
                        </label>
                    </div>

                    <div class="grid gap-3 sm:grid-cols-2 text-xs">
                        {move || recipient_is_third_party.get().then(|| view! {
                            <input
                                type="text"
                                aria-label="Third-party recipient name"
                                placeholder="Recipient doctor / specialist / person name *"
                                prop:value=move || recipient_name.get()
                                on:input=move |ev| recipient_name.set(event_target_value(&ev))
                                class="rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400"
                            />
                        })}
                        <input
                            type="text"
                            aria-label="Delivery destination for released records"
                            placeholder="Send records to (email, fax, or Dabney.moe Portable Passport) *"
                            prop:value=move || recipient_dest.get()
                            on:input=move |ev| recipient_dest.set(event_target_value(&ev))
                            class="rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-xs text-white placeholder:text-slate-400"
                        />
                    </div>

                    // Record Category Checkboxes + Separate Sensitive Opt-In
                    <div class="space-y-2 text-xs">
                        <p class="font-medium text-slate-300">"Record Categories Requested"</p>
                        <div class="flex flex-wrap gap-2">
                            {RoiRecordCategory::all()
                                .iter()
                                .copied()
                                .filter(|c| !c.is_sensitive_opt_in())
                                .map(|cat| {
                                    view! {
                                        <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/70 px-2.5 py-1.5 text-[11px] text-slate-200">
                                            <input
                                                type="checkbox"
                                                prop:checked=move || selected_categories.with(|l| l.contains(&cat))
                                                on:change=move |_| toggle_category(cat)
                                                class="accent-emerald-400"
                                            />
                                            <span>{cat.label()}</span>
                                        </label>
                                    }
                                })
                                .collect_view()}
                        </div>
                        <label class="flex cursor-pointer items-start gap-2 rounded-xl border border-amber-400/25 bg-amber-500/[0.05] p-2.5 text-[11px] text-amber-100">
                            <input
                                type="checkbox"
                                prop:checked=move || include_sensitive.get()
                                on:change=move |ev| include_sensitive.set(event_target_checked(&ev))
                                class="mt-0.5 accent-amber-400"
                            />
                            <span>
                                <strong>"Explicit Separate Opt-In (45 CFR § 164.508(a)(2)): "</strong>
                                "Include psychotherapy / behavioral health notes (excluded by default unless checked)."
                            </span>
                        </label>
                        {move || (legal_basis.get().is_part2_sud() || include_part2_sud_cat.get()).then(|| view! {
                            <div class="rounded-xl border border-amber-400/40 bg-amber-500/10 p-3 space-y-2.5 text-[11px] text-amber-100">
                                <p class="font-semibold text-amber-200">
                                    "42 CFR Part 2 Written Consent Elements (§ 2.31 & § 2.32)"
                                </p>
                                <label class="block space-y-1">
                                    <span class="text-amber-100">"Specific SUD information description (§ 2.31(a)(3)) *"</span>
                                    <input
                                        type="text"
                                        aria-label="42 CFR Part 2 specific SUD information description"
                                        prop:value=move || part2_specific_desc.get()
                                        on:input=move |ev| part2_specific_desc.set(event_target_value(&ev))
                                        class="w-full rounded-lg border border-amber-300/35 bg-slate-950 px-2.5 py-1.5 text-xs text-white"
                                    />
                                </label>
                                <label class="flex cursor-pointer items-start gap-2">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || include_part2_counseling.get()
                                        on:change=move |ev| include_part2_counseling.set(event_target_checked(&ev))
                                        class="mt-0.5 accent-amber-400"
                                    />
                                    <span>
                                        <strong>"Separate Written Consent (42 CFR § 2.31(b)): "</strong>
                                        "Include separately maintained SUD counseling notes (excluded by default)."
                                    </span>
                                </label>
                            </div>
                        })}
                    </div>

                    <button
                        type="submit"
                        class="min-h-[40px] rounded-full border border-cyan-400/45 bg-cyan-500/15 px-5 py-2 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/25"
                    >
                        "+ Add ROI Draft to Queue"
                    </button>
                </form>
            </section>

            // Active ROI Queue & Per-Clinic Automation Cards
            <section class="space-y-3">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <h3 class="text-sm font-semibold uppercase tracking-widest text-slate-300">
                        "ROI Automation Queue & 30-Day Tracker"
                    </h3>
                    <label class="inline-flex items-center gap-2 text-xs text-slate-300">
                        <span>"E-signature:"</span>
                        <input
                            type="text"
                            aria-label="Patient electronic signature name"
                            prop:value=move || signer_name.get()
                            on:input=move |ev| signer_name.set(event_target_value(&ev))
                            class="rounded-lg border border-white/15 bg-slate-950 px-2.5 py-1 text-xs text-white focus:border-emerald-400/60 focus:outline-none"
                        />
                    </label>
                </div>

                {move || {
                    let pid = patient_id.get_value();
                    let list = roi_map.with(|m| m.get(&pid).cloned().unwrap_or_default());
                    if list.is_empty() {
                        return view! {
                            <p class="rounded-xl border border-white/10 bg-slate-950/60 p-4 text-xs text-slate-300">
                                "No ROI requests in queue yet. Add a clinic above to automate sending HIPAA records requests."
                            </p>
                        }
                        .into_any();
                    }

                    list.into_iter()
                        .map(|req| {
                            let badge_cls = roi_status_badge_class(&req.status);
                            let status_label = req.status.label();
                            let req_id_att = req.id.clone();
                            let req_id_part2_ack = req.id.clone();
                            let req_id_sign = req.id.clone();
                            let req_id_fulfill = req.id.clone();
                            let req_id_revoke = req.id.clone();
                            let form_html = render_roi_form_html(&req);
                            let form_href = data_download_uri("text/html", &form_html);
                            let mailto_uri = build_roi_mailto_uri(&req);
                            let cover_txt = render_roi_cover_text(&req);
                            let followup_15 = render_roi_followup_letter(&req, 15);
                            let followup_30 = render_roi_followup_letter(&req, 30);
                            let is_draft = matches!(req.status, RoiStatus::Draft);
                            let is_dispatched = matches!(req.status, RoiStatus::Dispatched { .. });
                            let is_revoked = matches!(req.status, RoiStatus::Revoked { .. });
                            let is_part2 = req.requires_part2_consent();
                            let part2_ack = req.part2_tpo_redisclosure_acknowledged;
                            let ref_badge = req.status.reference_id().map(str::to_string);
                            let due_note = match &req.status {
                                RoiStatus::Dispatched { due_by_30d, transport_note, .. } => {
                                    Some(format!("30-day statutory deadline: {due_by_30d} · {transport_note}"))
                                }
                                RoiStatus::SignedReadyToSend { signed_by, signed_at } => {
                                    Some(format!("Signed by {signed_by} at {signed_at} — ready for batch automation."))
                                }
                                _ => None,
                            };

                            view! {
                                <article
                                    data-roi-card=req.id.clone()
                                    class="rounded-2xl border border-white/10 bg-slate-950/75 p-4 space-y-3"
                                >
                                    <div class="flex flex-wrap items-start justify-between gap-2 border-b border-white/10 pb-2.5">
                                        <div>
                                            <div class="flex flex-wrap items-center gap-2">
                                                <h4 class="text-sm font-semibold text-white">
                                                    {req.clinic_name.clone()}
                                                </h4>
                                                {is_part2.then(|| view! {
                                                    <span class="rounded-full border border-amber-400/45 bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-200">
                                                        "42 CFR Part 2"
                                                    </span>
                                                })}
                                                {ref_badge.map(|r| view! {
                                                    <span class="rounded bg-white/10 px-2 py-0.5 font-mono text-[10px] text-cyan-200">
                                                        {r}
                                                    </span>
                                                })}
                                            </div>
                                            <p class="mt-0.5 text-xs text-slate-300">
                                                {format!(
                                                    "{} · {} · {}",
                                                    req.legal_basis.short_label(),
                                                    req.delivery_channel.label(),
                                                    req.electronic_format.label()
                                                )}
                                            </p>
                                            <p class="mt-0.5 font-mono text-[11px] text-slate-400">
                                                {format!(
                                                    "HIM Email: {} · Fax: {} · To: {}",
                                                    if req.clinic_him_email.is_empty() { "—" } else { &req.clinic_him_email },
                                                    if req.clinic_him_fax.is_empty() { "—" } else { &req.clinic_him_fax },
                                                    req.recipient.summary()
                                                )}
                                            </p>
                                        </div>
                                        <span class=format!("shrink-0 rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {badge_cls}")>
                                            {status_label}
                                        </span>
                                    </div>

                                    {due_note.map(|note| view! {
                                        <p class="text-xs text-emerald-200">{note}</p>
                                    })}

                                    <div class="flex flex-wrap gap-1.5">
                                        {req.categories.iter().map(|cat| view! {
                                            <span class="rounded-md border border-white/10 bg-white/[0.03] px-2 py-0.5 text-[10px] text-slate-300">
                                                {cat.label()}
                                            </span>
                                        }).collect_view()}
                                    </div>

                                    {is_draft.then(|| view! {
                                        <div class="rounded-xl border border-cyan-400/25 bg-cyan-500/[0.05] p-3 space-y-2.5 text-xs">
                                            <label class="flex cursor-pointer items-start gap-2 text-slate-200">
                                                <input
                                                    type="checkbox"
                                                    prop:checked=req.attestation_accepted
                                                    on:change=move |ev| {
                                                        let checked = event_target_checked(&ev);
                                                        let pid = patient_id.get_value();
                                                        let rid = req_id_att.clone();
                                                        roi_map.update(|map| {
                                                            if let Some(list) = map.get_mut(&pid) {
                                                                if let Some(item) = list.iter_mut().find(|r| r.id == rid) {
                                                                    item.attestation_accepted = checked;
                                                                }
                                                            }
                                                        });
                                                    }
                                                    class="mt-0.5 accent-emerald-400"
                                                />
                                                <span>
                                                    "I authorize this releasing provider to deliver the selected electronic records above and understand I may revoke this authorization in writing at any time."
                                                </span>
                                            </label>
                                            {is_part2.then(|| view! {
                                                <label class="flex cursor-pointer items-start gap-2 text-amber-100">
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=part2_ack
                                                        on:change=move |ev| {
                                                            let checked = event_target_checked(&ev);
                                                            let pid = patient_id.get_value();
                                                            let rid = req_id_part2_ack.clone();
                                                            roi_map.update(|map| {
                                                                if let Some(list) = map.get_mut(&pid) {
                                                                    if let Some(item) = list.iter_mut().find(|r| r.id == rid) {
                                                                        item.part2_tpo_redisclosure_acknowledged = checked;
                                                                    }
                                                                }
                                                            });
                                                        }
                                                        class="mt-0.5 accent-amber-400"
                                                    />
                                                    <span>
                                                        "42 CFR § 2.31(a)(10) Acknowledgment: I understand these SUD records are protected by 42 CFR Part 2, that redisclosure by HIPAA recipients may occur as permitted under HIPAA (except in proceedings against me), and that refusal to sign will not affect treatment or benefits."
                                                    </span>
                                                </label>
                                            })}
                                            {
                                                let rid_err_lookup = req_id_sign.clone();
                                                let rid_sign_click = req_id_sign.clone();
                                                view! {
                                                    <div class="space-y-2">
                                                        <button
                                                            type="button"
                                                            on:click=move |_| {
                                                                roi_error.set(None);
                                                                let sig = signer_name.get_untracked();
                                                                let pid = patient_id.get_value();
                                                                let rid = rid_sign_click.clone();
                                                                let mut sign_res = Ok(());
                                                                roi_map.update(|map| {
                                                                    if let Some(list) = map.get_mut(&pid) {
                                                                        if let Some(item) = list.iter_mut().find(|r| r.id == rid) {
                                                                            sign_res = sign_roi_request(item, &sig, DEMO_TIMESTAMP);
                                                                        }
                                                                    }
                                                                });
                                                                if let Err(err) = sign_res {
                                                                    card_sign_errors.update(|m| {
                                                                        m.insert(rid.clone(), err.clone());
                                                                    });
                                                                    roi_error.set(Some(err));
                                                                } else {
                                                                    card_sign_errors.update(|m| {
                                                                        m.remove(&rid);
                                                                    });
                                                                    roi_notice.set(Some("ROI request signed and queued for batch automation.".into()));
                                                                }
                                                            }
                                                            class="rounded-full bg-cyan-500 px-4 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-cyan-400"
                                                        >
                                                            "Sign & Queue for Automation"
                                                        </button>
                                                        {move || card_sign_errors.with(|m| m.get(&rid_err_lookup).cloned()).map(|err| view! {
                                                            <p role="alert" class="rounded-xl border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">{err}</p>
                                                        })}
                                                    </div>
                                                }
                                            }
                                        </div>
                                    })}

                                    <div class="flex flex-wrap items-center gap-2 pt-1">
                                        <a
                                            href=form_href
                                            download=format!("{}-hipaa-roi.html", req.id)
                                            class="inline-flex min-h-[36px] items-center rounded-full border border-emerald-400/35 bg-emerald-500/10 px-3.5 py-1 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/20"
                                        >
                                            {if is_draft {
                                                "Download Unsigned Draft (.html)"
                                            } else {
                                                "Download Signed Form (.html / PDF)"
                                            }}
                                        </a>
                                        {if is_draft {
                                            view! {
                                                <span class="inline-flex min-h-[36px] items-center rounded-full border border-white/10 bg-white/[0.03] px-3 py-1 text-[11px] text-slate-400">
                                                    "Sign above to unlock HIM email handoff"
                                                </span>
                                            }
                                            .into_any()
                                        } else {
                                            mailto_uri
                                                .map(|uri| {
                                                    view! {
                                                        <a
                                                            href=uri
                                                            class="inline-flex min-h-[36px] items-center rounded-full border border-cyan-400/35 bg-cyan-500/10 px-3.5 py-1 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                                                        >
                                                            "Send via HIM Email (mailto:) ↗"
                                                        </a>
                                                    }
                                                })
                                                .into_any()
                                        }}
                                        {is_dispatched.then(|| view! {
                                            <button
                                                type="button"
                                                on:click=move |_| {
                                                    let pid = patient_id.get_value();
                                                    let rid = req_id_fulfill.clone();
                                                    roi_map.update(|map| {
                                                        if let Some(list) = map.get_mut(&pid) {
                                                            if let Some(item) = list.iter_mut().find(|r| r.id == rid) {
                                                                let ref_id = item.status.reference_id().unwrap_or("ROI-DEMO").to_string();
                                                                item.status = RoiStatus::Fulfilled {
                                                                    reference_id: ref_id,
                                                                    fulfilled_at: DEMO_TIMESTAMP.into(),
                                                                };
                                                            }
                                                        }
                                                    });
                                                }
                                                class="min-h-[36px] rounded-full border border-emerald-300/40 bg-emerald-500/15 px-3 py-1 text-xs font-semibold text-emerald-100 hover:bg-emerald-500/25"
                                            >
                                                "Mark Records Received"
                                            </button>
                                        })}
                                        {(!is_revoked).then(|| view! {
                                            <button
                                                type="button"
                                                on:click=move |_| {
                                                    let pid = patient_id.get_value();
                                                    let rid = req_id_revoke.clone();
                                                    roi_map.update(|map| {
                                                        if let Some(list) = map.get_mut(&pid) {
                                                            if let Some(item) = list.iter_mut().find(|r| r.id == rid) {
                                                                revoke_roi_request(item, DEMO_TIMESTAMP);
                                                            }
                                                        }
                                                    });
                                                }
                                                class="min-h-[36px] rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:border-rose-400/40 hover:text-rose-200"
                                            >
                                                "Revoke"
                                            </button>
                                        })}
                                    </div>

                                    <details class="rounded-xl border border-white/10 bg-slate-900/60 px-3 py-2 text-xs">
                                        <summary class="cursor-pointer select-none font-medium text-cyan-300 hover:text-cyan-200">
                                            "Inspect Cover Letter & Automated Day-15 / Day-30 Follow-Up Reminders"
                                        </summary>
                                        <div class="mt-2.5 space-y-2.5 border-t border-white/10 pt-2.5">
                                            <div>
                                                <p class="text-[11px] font-semibold text-slate-300">"Cover Letter / Fax Sheet"</p>
                                                <pre class="mt-1 max-h-44 overflow-auto whitespace-pre-wrap rounded-lg border border-white/10 bg-slate-950 p-2.5 font-mono text-[11px] text-slate-200">
                                                    {cover_txt}
                                                </pre>
                                            </div>
                                            <div class="grid gap-2.5 md:grid-cols-2">
                                                <div>
                                                    <p class="text-[11px] font-semibold text-cyan-300">"Automated Day-15 Status Check"</p>
                                                    <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg border border-white/10 bg-slate-950 p-2.5 font-mono text-[10px] text-slate-300">
                                                        {followup_15}
                                                    </pre>
                                                </div>
                                                <div>
                                                    <p class="text-[11px] font-semibold text-amber-300">"Automated Day-30 Statutory Reminder (45 CFR § 164.524)"</p>
                                                    <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg border border-white/10 bg-slate-950 p-2.5 font-mono text-[10px] text-slate-300">
                                                        {followup_30}
                                                    </pre>
                                                </div>
                                            </div>
                                        </div>
                                    </details>
                                </article>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </section>
        </div>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use clinical_workspace::{
        default_allergies_for_patient, default_patient_demo_state,
        default_roi_requests_for_patient, find_patient,
    };
    use leptos::prelude::RenderHtml;

    #[test]
    fn ehr_portals_passport_and_roi_panels_render_cleanly_without_legacy_brand() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let state = default_patient_demo_state(&nadia);
        let allergies = default_allergies_for_patient(&nadia.id);
        let rois = default_roi_requests_for_patient(&nadia);

        let mut charts_init = HashMap::new();
        charts_init.insert(nadia.id.clone(), nadia.clone());
        let mut alg_init = HashMap::new();
        alg_init.insert(nadia.id.clone(), allergies.clone());
        let mut st_init = HashMap::new();
        st_init.insert(nadia.id.clone(), state.clone());
        let mut roi_init = HashMap::new();
        roi_init.insert(nadia.id.clone(), rois);

        let portals_html = view! {
            <PatientEhrPortalsPanel
                patient=nadia.clone()
                linked_portals=RwSignal::new(HashMap::new())
                charts=RwSignal::new(charts_init)
                allergies_map=RwSignal::new(alg_init)
                demo_states=RwSignal::new(st_init)
                on_records_updated=Callback::new(|_| {})
            />
        }
        .to_html();
        assert!(portals_html.contains("Add What You Have"));
        assert!(portals_html
            .contains("aria-label=\"Choose a clinical document file (.json, .xml, .hl7)\""));
        assert!(portals_html.contains("SMART on FHIR R4 + PKCE"));
        assert!(portals_html.contains("Stanford Health Care"));
        assert!(portals_html.contains("KipuEMR"));
        assert!(portals_html.contains("SimplePractice"));
        assert!(portals_html.contains("Hint Health"));
        assert!(portals_html.contains("42 CFR Part 2"));
        assert!(portals_html.contains("Import Downloaded FHIR R4 Bundle"));
        assert!(portals_html.contains("C-CDA XML Sample"));
        assert!(portals_html.contains("HL7 v2 Sample"));
        assert!(portals_html.contains("shlink:/"));

        let passport_html = view! {
            <PortablePassportSection
                patient=nadia.clone()
                state=state
                allergies=allergies
                linked_portals=Vec::new()
                audience=RwSignal::new(PassportAudience::DoctorOrSpecialist)
                emergency_note=RwSignal::new(String::new())
                on_restore_passport=Callback::new(|_| {})
            />
        }
        .to_html();
        assert!(passport_html.contains("Your Portable Health Passport"));
        assert!(passport_html.contains("dabney.health_passport.v1"));
        assert!(passport_html.contains("Explicit 42 CFR Part 2 Opt-In"));
        assert!(passport_html.contains("1-Page Waiting-Room Handout"));
        assert!(passport_html.contains("New Primary Care"));
        assert!(passport_html.contains("Psychiatrist"));
        assert!(passport_html.contains("Therapist"));
        assert!(passport_html.contains("aria-pressed=\"true\""));
        assert!(passport_html.contains("aria-label=\"Load saved Health Passport JSON file\""));
        assert!(passport_html.contains("Enable general 42 CFR Part 2 opt-in above first"));
        assert!(passport_html.contains("Download Printable Brief"));
        assert!(passport_html.contains("Persist Portable File"));
        assert!(passport_html.contains("Download FHIR R4 IPS"));

        let roi_html = view! {
            <AutomatedRoiPanel
                patient=nadia
                roi_map=RwSignal::new(roi_init)
            />
        }
        .to_html();
        assert!(roi_html.contains("Automate Sending Release of Information"));
        assert!(roi_html.contains("45 CFR"));
        assert!(roi_html.contains("42 CFR Part 2 SUD Authorization"));
        assert!(roi_html.contains("Automate Send All Signed ROI Forms (1)"));
        assert!(roi_html.contains("Download Unsigned Draft (.html)"));
        assert!(roi_html.contains("Sign above to unlock HIM email handoff"));
        assert!(roi_html.contains("Stanford Health Care"));

        let legacy_brand = ["co", "da"].concat();
        for html in [&portals_html, &passport_html, &roi_html] {
            assert!(!html.to_ascii_lowercase().contains(&legacy_brand));
            assert!(!html.contains("text-slate-500"));
        }
    }
}
