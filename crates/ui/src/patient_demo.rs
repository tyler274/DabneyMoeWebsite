//! Interactive zero-PHI Patient Demo page (`/clinical/patient`) and shared
//! clinical demo suite switcher.
//!
//! Runs identically in native SSR and browser/Android WebAssembly with zero
//! external servers, zero telemetry, and 100% fictional synthetic records.

use std::collections::{HashMap, HashSet};

#[cfg(any(feature = "csr", feature = "hydrate"))]
use clinical_workspace::build_harness_llm_prompt;
use clinical_workspace::{
    build_doctor_share_preview, default_allergies_for_patient, default_patient_demo_state,
    default_patient_questions, default_roi_requests_for_patient, run_agent_turn, synthetic_panel,
    AgentAudience, AgentRunOptions, AgentThread, ChartRecord, DemoConnectionState, HealthPassport,
    LinkedPortalConnection, PassportAudience, PatientVisitNote, RecordKind, RoiStatus, RunBudget,
    ShareDeliveryStatus, SharePacketPreview,
};
use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::agent_chat::{AgentToolCatalogBar, AgenticThreadView};
use crate::health_bridge::LocalLlmStatusUi;
#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::health_bridge::{
    generate_local_llm, load_personal_vault, prepare_local_llm, probe_local_llm,
    probe_personal_vault, save_personal_vault, PersonalVaultSaveUi,
};
use crate::local_ai_chat::LocalLlmStatusBanner;
use crate::patient_ehr::{AutomatedRoiPanel, PatientEhrPortalsPanel, PortablePassportSection};
#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::sections::open_site_or_external;
use crate::sections::{Footer, NavBar, SITE_ORIGIN};
use crate::AppShellMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatientTab {
    You,
    Portals,
    Shared,
    Roi,
}

const fn record_kind_badge_class(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::LabResult => "border-violet-400/30 bg-violet-500/15 text-violet-300",
        RecordKind::ProgressNote => "border-cyan-400/30 bg-cyan-500/15 text-cyan-300",
        RecordKind::CgmSummary => "border-emerald-400/30 bg-emerald-500/15 text-emerald-300",
        RecordKind::PatientMessage => "border-amber-400/30 bg-amber-500/15 text-amber-300",
        RecordKind::Imaging => "border-sky-400/30 bg-sky-500/15 text-sky-300",
    }
}

/// Compact clinical demo suite switcher shared across `/clinical`,
/// `/clinical/patient`, `/clinical/physician`, and `/mental-health`.
#[component]
pub fn ClinicalSuiteSwitcher() -> impl IntoView {
    let location = use_location();
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);

    let is_active = move |target: &'static str| {
        let path = location.pathname.get();
        let trimmed = path.trim_end_matches('/');
        let normalized = if trimmed.is_empty() { "/" } else { trimmed };
        if target == "/clinical/patient" && app_mode == AppShellMode::ClinicalDemoApp {
            normalized == "/" || normalized == "/clinical/patient"
        } else {
            normalized == target
        }
    };

    let tab_class = move |target: &'static str| {
        if is_active(target) {
            "inline-flex min-h-[38px] items-center rounded-full border border-emerald-400/50 bg-emerald-500/15 px-3.5 py-1.5 text-xs font-semibold text-emerald-200 transition"
        } else {
            "inline-flex min-h-[38px] items-center rounded-full border border-white/10 bg-slate-950/60 px-3.5 py-1.5 text-xs font-medium text-slate-300 transition hover:border-white/25 hover:text-white"
        }
    };

    let patient_href = if app_mode == AppShellMode::ClinicalDemoApp {
        "/"
    } else {
        "/clinical/patient"
    };

    view! {
        <nav
            aria-label="Clinical demo suite"
            class="flex flex-wrap items-center gap-2 rounded-2xl border border-white/10 bg-white/[0.02] p-2"
        >
            <A href=patient_href attr:class=move || tab_class("/clinical/patient")>
                "Patient Demo"
            </A>
            <A href="/clinical/physician" attr:class=move || tab_class("/clinical/physician")>
                "Physician Demo"
            </A>
            <A href="/mental-health" attr:class=move || tab_class("/mental-health")>
                "Mental Health & CGM"
            </A>
            <A href="/clinical" attr:class=move || tab_class("/clinical")>
                "Suite Overview"
            </A>
        </nav>
    }
}

/// Interactive zero-PHI Patient Demo page (`/clinical/patient`, and `/` inside
/// the standalone Android/Tauri demo app).
#[component]
pub fn PatientDemoPage() -> impl IntoView {
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);
    let baseline_panel = synthetic_panel();
    let patient_order = StoredValue::new(
        baseline_panel
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>(),
    );
    let baseline_charts = StoredValue::new(baseline_panel);

    let (
        initial_id,
        initial_charts_map,
        initial_states,
        initial_allergies,
        initial_rois,
        initial_question,
        initial_threads,
    ) = baseline_charts.with_value(|list| {
        let mut charts_map = HashMap::new();
        let mut states = HashMap::new();
        let mut allergies = HashMap::new();
        let mut rois = HashMap::new();
        let mut threads = HashMap::new();
        for chart in list {
            charts_map.insert(chart.id.clone(), chart.clone());
            states.insert(chart.id.clone(), default_patient_demo_state(chart));
            allergies.insert(chart.id.clone(), default_allergies_for_patient(&chart.id));
            rois.insert(chart.id.clone(), default_roi_requests_for_patient(chart));
        }
        if let Some(first) = list.first() {
            let q = default_patient_questions(&first.id)
                .first()
                .copied()
                .unwrap_or("Summarize my recent synthetic records")
                .to_string();
            let mut thread = AgentThread::new(&first.id, AgentAudience::Patient);
            let turn = run_agent_turn(
                first,
                &q,
                AgentAudience::Patient,
                thread.next_turn_index(),
                &thread.working_notes,
                AgentRunOptions::default(),
            );
            thread.record_turn(turn);
            threads.insert(first.id.clone(), thread);
            (
                first.id.clone(),
                charts_map,
                states,
                allergies,
                rois,
                q,
                threads,
            )
        } else {
            (
                String::new(),
                charts_map,
                states,
                allergies,
                rois,
                String::new(),
                threads,
            )
        }
    });

    let selected_id = RwSignal::new(initial_id);
    let active_tab = RwSignal::new(PatientTab::You);
    let charts = RwSignal::new(initial_charts_map);
    let demo_states = RwSignal::new(initial_states);
    let allergies_map = RwSignal::new(initial_allergies);
    let linked_portals = RwSignal::new(HashMap::<String, Vec<LinkedPortalConnection>>::new());
    let roi_map = RwSignal::new(initial_rois);
    let passport_audience = RwSignal::new(PassportAudience::DoctorOrSpecialist);
    let emergency_note = RwSignal::new(String::new());
    let ehr_sync_banner = RwSignal::new(Option::<String>::None);
    let question_input = RwSignal::new(initial_question);
    let threads = RwSignal::new(initial_threads);
    let step_budget = RwSignal::new(RunBudget::default().max_steps);
    let active_record = RwSignal::new(Option::<String>::None);
    let new_private_note = RwSignal::new(String::new());
    let new_visit_question = RwSignal::new(String::new());
    let next_note_seq = RwSignal::new(2usize);

    // Optional on-device Gemini Nano (AICore) & encrypted note vault (`moe.dabney.personal` only).
    let llm_status = RwSignal::new(Option::<LocalLlmStatusUi>::None);
    let use_local_llm = RwSignal::new(true);
    let llm_preparing = RwSignal::new(false);
    let llm_prepare_notice = RwSignal::new(Option::<String>::None);
    let llm_thinking = RwSignal::new(false);
    let llm_req_seq = RwSignal::new(0usize);
    let vault_enabled = RwSignal::new(false);

    Effect::new(move |_| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            probe_local_llm(move |status| {
                llm_status.set(Some(status));
            });
            probe_personal_vault(move |status| {
                if status.enabled {
                    vault_enabled.set(true);
                    if status.note_count > 0 {
                        load_personal_vault(move |res| {
                            if let Ok(snap) = res {
                                if let Some(notes_raw) = snap.notes_json {
                                    if let Ok(restored) =
                                        serde_json::from_str::<Vec<PatientVisitNote>>(&notes_raw)
                                    {
                                        if !restored.is_empty() {
                                            let active = selected_id.get_untracked();
                                            next_note_seq.set(restored.len() + 2);
                                            demo_states.update(|map| {
                                                if let Some(st) = map.get_mut(&active) {
                                                    st.personal_notes = restored;
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            });
        }
    });

    let prepare_llm_now = Callback::new(move |()| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            llm_preparing.set(true);
            llm_prepare_notice.set(Some(
                "Requesting Gemini Nano download via Android AICore…".to_string(),
            ));
            prepare_local_llm(move |res| {
                llm_preparing.set(false);
                match res {
                    Ok(outcome) => {
                        llm_prepare_notice.set(Some(outcome.detail));
                        probe_local_llm(move |status| llm_status.set(Some(status)));
                    }
                    Err(err) => llm_prepare_notice.set(Some(err)),
                }
            });
        }
    });

    let select_profile = move |patient_id: String| {
        llm_req_seq.update(|n| *n += 1);
        llm_thinking.set(false);
        active_record.set(None);
        ehr_sync_banner.set(None);
        selected_id.set(patient_id.clone());
        let default_q = default_patient_questions(&patient_id)
            .first()
            .copied()
            .unwrap_or("Summarize my recent synthetic records")
            .to_string();
        question_input.set(default_q.clone());
        let has_existing = threads.with_untracked(|map| {
            map.get(&patient_id)
                .is_some_and(|thread| !thread.turns.is_empty())
        });
        if !has_existing {
            let max_steps = step_budget.get_untracked();
            if let Some(seeded) = charts.with_untracked(|map| {
                map.get(&patient_id).map(|p| {
                    let mut thread = AgentThread::new(&p.id, AgentAudience::Patient);
                    let turn = run_agent_turn(
                        p,
                        &default_q,
                        AgentAudience::Patient,
                        thread.next_turn_index(),
                        &thread.working_notes,
                        AgentRunOptions {
                            budget: RunBudget {
                                max_steps,
                                max_tool_calls: max_steps,
                                max_citations: 4,
                            },
                            stop_after_step: None,
                        },
                    );
                    thread.record_turn(turn);
                    thread
                })
            }) {
                threads.update(|map| {
                    map.insert(patient_id, seeded);
                });
            }
        }
    };

    let clear_active_thread = move |_| {
        llm_req_seq.update(|n| *n += 1);
        llm_thinking.set(false);
        active_record.set(None);
        let active_id = selected_id.get_untracked();
        threads.update(|map| {
            if let Some(thread) = map.get_mut(&active_id) {
                thread.clear();
            }
        });
    };

    let ask_patient_question = move |raw_q: String| {
        let trimmed = raw_q.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        active_record.set(None);
        let active_id = selected_id.get();
        let max_steps = step_budget.get_untracked();
        let (turn_index, prior_notes) = threads.with_untracked(|map| {
            map.get(&active_id)
                .map(|t| (t.next_turn_index(), t.working_notes.clone()))
                .unwrap_or((1, Vec::new()))
        });
        let next_pair = charts.with_untracked(|map| {
            map.get(&active_id).map(|p| {
                let turn = run_agent_turn(
                    p,
                    &trimmed,
                    AgentAudience::Patient,
                    turn_index,
                    &prior_notes,
                    AgentRunOptions {
                        budget: RunBudget {
                            max_steps,
                            max_tool_calls: max_steps,
                            max_citations: 4,
                        },
                        stop_after_step: None,
                    },
                );
                (p.clone(), turn)
            })
        });
        let Some((patient_chart, turn)) = next_pair else {
            return;
        };
        threads.update(|map| {
            map.entry(active_id.clone())
                .or_insert_with(|| AgentThread::new(&active_id, AgentAudience::Patient))
                .record_turn(turn.clone());
        });

        let req_id = llm_req_seq.get_untracked() + 1;
        llm_req_seq.set(req_id);

        let should_run_llm = use_local_llm.get_untracked()
            && turn.grounded
            && llm_status
                .get_untracked()
                .is_some_and(|s| s.enabled && s.availability == "available");

        #[cfg(any(feature = "csr", feature = "hydrate"))]
        if should_run_llm {
            let prompt = build_harness_llm_prompt(&patient_chart, &turn);
            let target_pid = active_id;
            llm_thinking.set(true);
            generate_local_llm(&prompt, Some(320), move |res| {
                if llm_req_seq.get_untracked() != req_id {
                    return;
                }
                llm_thinking.set(false);
                if let Ok(gen) = res {
                    let mut enhanced = turn.clone();
                    enhanced.answer = gen.text;
                    enhanced.engine_label = format!(
                        "Agentic Harness + {} ({} ms)",
                        gen.model_name, gen.latency_ms
                    );
                    enhanced.basis = format!(
                        "{} (on-device · {} ms) grounded strictly in {} harness tool step(s) and {} verbatim citation(s).",
                        gen.model_name,
                        gen.latency_ms,
                        enhanced.steps.len(),
                        enhanced.citations.len(),
                    );
                    threads.update(|map| {
                        if let Some(thread) = map.get_mut(&target_pid) {
                            thread.replace_turn(enhanced);
                        }
                    });
                }
            });
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        let _ = (patient_chart, should_run_llm);
    };

    let toggle_record_selection = move |record_id: String| {
        let active_id = selected_id.get();
        demo_states.update(|map| {
            if let Some(state) = map.get_mut(&active_id) {
                if let Some(pos) = state
                    .selected_record_ids
                    .iter()
                    .position(|id| id == &record_id)
                {
                    state.selected_record_ids.remove(pos);
                } else {
                    state.selected_record_ids.push(record_id);
                }
                state.delivery_status = ShareDeliveryStatus::Draft;
            }
        });
    };

    let set_connection_mode = move |next_state: DemoConnectionState| {
        let active_id = selected_id.get();
        demo_states.update(|map| {
            if let Some(state) = map.get_mut(&active_id) {
                if !next_state.allows_sharing() {
                    state.delivery_status = ShareDeliveryStatus::Draft;
                }
                state.connection = next_state;
            }
        });
    };

    let reset_active_demo = move |_| {
        let active_id = selected_id.get();
        let baseline =
            baseline_charts.with_value(|list| list.iter().find(|p| p.id == active_id).cloned());
        if let Some(fresh_chart) = baseline {
            let fresh_state = default_patient_demo_state(&fresh_chart);
            let fresh_allergies = default_allergies_for_patient(&active_id);
            let fresh_rois = default_roi_requests_for_patient(&fresh_chart);
            charts.update(|map| {
                map.insert(active_id.clone(), fresh_chart);
            });
            demo_states.update(|map| {
                map.insert(active_id.clone(), fresh_state);
            });
            allergies_map.update(|map| {
                map.insert(active_id.clone(), fresh_allergies);
            });
            linked_portals.update(|map| {
                map.remove(&active_id);
            });
            roi_map.update(|map| {
                map.insert(active_id, fresh_rois);
            });
            ehr_sync_banner.set(None);
        }
    };

    let restore_from_passport = Callback::new(move |passport: HealthPassport| {
        let active_id = selected_id.get_untracked();
        charts.update(|map| {
            if let Some(chart) = map.get_mut(&active_id) {
                for rec in &passport.records {
                    if !chart.records.iter().any(|r| r.id == rec.id) {
                        chart.records.insert(0, rec.clone());
                    }
                }
                for med in &passport.medications {
                    if !chart
                        .medications
                        .iter()
                        .any(|m| m.name.eq_ignore_ascii_case(&med.name))
                    {
                        chart.medications.push(med.clone());
                    }
                }
                for prob in &passport.problems {
                    if !chart
                        .problems
                        .iter()
                        .any(|p| p.label.eq_ignore_ascii_case(&prob.label))
                    {
                        chart.problems.push(prob.clone());
                    }
                }
            }
        });
        allergies_map.update(|map| {
            let list = map.entry(active_id.clone()).or_default();
            for alg in &passport.allergies {
                if !list
                    .iter()
                    .any(|a| a.substance.eq_ignore_ascii_case(&alg.substance))
                {
                    list.push(alg.clone());
                }
            }
        });
        demo_states.update(|map| {
            if let Some(st) = map.get_mut(&active_id) {
                for rec in &passport.records {
                    if !st.selected_record_ids.contains(&rec.id) {
                        st.selected_record_ids.push(rec.id.clone());
                    }
                }
                if !passport.visit_note.is_empty() {
                    st.visit_note = passport.visit_note.clone();
                }
                if !passport.visit_questions.is_empty() {
                    st.visit_questions = passport.visit_questions.clone();
                }
            }
        });
        if !passport.emergency_note.is_empty() {
            emergency_note.set(passport.emergency_note);
        }
        ehr_sync_banner.set(Some(format!(
            "Restored verified Health Passport ({} record(s), {} med(s)) into your local workspace.",
            passport.records.len(),
            passport.medications.len()
        )));
    });

    let on_submit_question = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        ask_patient_question(question_input.get());
    };

    let on_add_private_note = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let body = new_private_note.get().trim().to_string();
        if body.is_empty() {
            return;
        }
        let active_id = selected_id.get();
        let seq = next_note_seq.get_untracked();
        next_note_seq.set(seq + 1);
        demo_states.update(|map| {
            if let Some(state) = map.get_mut(&active_id) {
                state.personal_notes.push(PatientVisitNote {
                    id: format!("{active_id}-priv-{seq}"),
                    body,
                    created_label: if vault_enabled.get_untracked() {
                        "Saved in encrypted vault".into()
                    } else {
                        "Added locally (demo)".into()
                    },
                });
            }
        });
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        if vault_enabled.get_untracked() {
            let notes_snapshot = demo_states
                .get_untracked()
                .get(&active_id)
                .map(|st| st.personal_notes.clone())
                .unwrap_or_default();
            if let Ok(encoded) = serde_json::to_string(&notes_snapshot) {
                save_personal_vault(
                    &PersonalVaultSaveUi {
                        notes_json: Some(encoded),
                        ..Default::default()
                    },
                    |_| {},
                );
            }
        }
        new_private_note.set(String::new());
    };

    let on_add_visit_question = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let body = new_visit_question.get().trim().to_string();
        if body.is_empty() {
            return;
        }
        let active_id = selected_id.get();
        demo_states.update(|map| {
            if let Some(state) = map.get_mut(&active_id) {
                state.visit_questions.push(body);
                state.delivery_status = ShareDeliveryStatus::Draft;
            }
        });
        new_visit_question.set(String::new());
    };

    let open_web_demo = move |ev: leptos::ev::MouseEvent| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            if open_site_or_external("/clinical/patient") {
                ev.prevent_default();
            }
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        {
            let _ = ev;
        }
    };

    view! {
        <Title text="Patient Demo (Zero PHI) — Dabney.moe Clinical Workspace | dabney.moe" />
        <Meta
            name="description"
            content="Interactive zero-PHI patient health companion demo in pure Rust and WebAssembly: explore fictional records, medications, record-cited questions, and explicit For My Doctor sharing controls with zero data leaving your device."
        />
        <Link rel="canonical" href="https://dabney.moe/clinical/patient" />

        <NavBar />

        <div class="mx-auto max-w-5xl px-4 py-6 sm:px-6 sm:py-10 space-y-6">
            // Top Header
            <header class="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-5">
                <div class="flex items-center gap-3.5">
                    <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-[#01081b] p-1 shadow-sm">
                        <img
                            src="/dabney-elephant.svg"
                            alt="Dabney.moe elephant emblem"
                            class="h-full w-full object-contain"
                        />
                    </div>
                    <div>
                        <p class="text-[11px] font-semibold uppercase tracking-widest text-emerald-300">
                            "Dabney.moe · Patient Companion Demo"
                        </p>
                        <h1 class="text-2xl font-bold tracking-tight text-white sm:text-3xl">
                            "Patient Experience Demo"
                        </h1>
                    </div>
                </div>
                {(app_mode == AppShellMode::ClinicalDemoApp).then(|| view! {
                    <a
                        href=format!("{SITE_ORIGIN}/clinical/patient")
                        target="_blank"
                        rel="noopener noreferrer"
                        on:click=open_web_demo
                        class="inline-flex min-h-[40px] items-center rounded-full border border-cyan-400/30 bg-cyan-500/10 px-4 py-2 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                    >
                        "Open on dabney.moe ↗"
                    </a>
                })}
            </header>

            // Prominent Zero-PHI Synthetic Demo Disclaimer
            <section
                class="rounded-2xl border border-emerald-400/35 bg-emerald-500/10 px-5 py-4"
                role="note"
                aria-label="Zero PHI synthetic demo notice"
            >
                <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <div class="space-y-1">
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "100% Synthetic Demo · Zero PHI · Do Not Enter Real Health Information"
                        </p>
                        <p class="text-xs leading-relaxed text-emerald-50/90 sm:text-sm">
                            "Every name, lab value, medication, and message here is fictional sample data running "
                            "locally in Rust/WebAssembly. Nothing is uploaded, stored on servers, or shared with "
                            "third parties. Educational software demonstration only—not medical advice or a diagnostic tool."
                        </p>
                    </div>
                    <span class="shrink-0 self-start rounded-full border border-emerald-400/40 bg-slate-950/70 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-emerald-300 sm:self-center">
                        "On-Device Only"
                    </span>
                </div>
            </section>

            // Suite navigation across all 3 interactive demos + overview
            <ClinicalSuiteSwitcher />

            // Sample Fictional Patient Switcher
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-4 space-y-3">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <h2 class="text-xs font-semibold uppercase tracking-widest text-slate-300">
                        "Choose a Fictional Sample Profile"
                    </h2>
                    <button
                        type="button"
                        on:click=reset_active_demo
                        class="rounded-full border border-white/15 px-3 py-1 text-[11px] font-medium text-slate-300 transition hover:border-white/30 hover:text-white"
                    >
                        "Reset sample state"
                    </button>
                </div>
                <div class="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-4">
                    {patient_order.with_value(|ids| {
                        ids.iter()
                            .filter_map(|pid| {
                                baseline_charts.with_value(|list| {
                                    list.iter().find(|p| p.id == *pid).cloned()
                                })
                            })
                            .map(|chart| {
                                let pid_click = chart.id.clone();
                                let pid_aria = chart.id.clone();
                                let pid_class = chart.id.clone();
                                let name = chart.display_name.clone();
                                let focus = chart.primary_focus.clone();
                                let mrn = chart.mrn.clone();
                                view! {
                                    <button
                                        type="button"
                                        aria-pressed=move || selected_id.get() == pid_aria
                                        on:click=move |_| select_profile(pid_click.clone())
                                        class=move || {
                                            if selected_id.get() == pid_class {
                                                "min-h-[44px] w-full rounded-xl border border-emerald-400/60 bg-emerald-500/15 p-3 text-left transition"
                                            } else {
                                                "min-h-[44px] w-full rounded-xl border border-white/10 bg-slate-950/60 p-3 text-left transition hover:border-white/25 hover:bg-white/[0.04]"
                                            }
                                        }
                                    >
                                        <div class="flex items-center justify-between gap-2">
                                            <span class="text-sm font-semibold text-white">{name}</span>
                                            <span class="font-mono text-[10px] text-emerald-300">{mrn}</span>
                                        </div>
                                        <p class="mt-1 line-clamp-1 text-xs text-slate-300">{focus}</p>
                                    </button>
                                }
                            })
                            .collect_view()
                    })}
                </div>
            </section>

            // Patient Tab Bar ("Your Health", "Doctor Portals", "For My Doctor & Portable File", "Release of Info")
            // Kept outside chart/ROI data closures so mutating linked portals or ROI
            // state never unmounts/remounts interactive sub-panels and clears local logs.
            <div class="space-y-6">
                {move || ehr_sync_banner.get().map(|msg| view! {
                    <div role="status" class="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-cyan-400/35 bg-cyan-500/10 px-4 py-3 text-xs text-cyan-100">
                        <span>{msg}</span>
                        <button
                            type="button"
                            on:click=move |_| ehr_sync_banner.set(None)
                            class="rounded-full border border-white/15 px-2.5 py-0.5 text-[11px] text-slate-200 hover:bg-white/10"
                        >
                            "Dismiss"
                        </button>
                    </div>
                })}

                <div
                    role="tablist"
                    aria-label="Patient companion sections"
                    class="grid grid-cols-2 gap-2 rounded-2xl border border-white/10 bg-slate-900/70 p-1.5 lg:grid-cols-4"
                >
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || active_tab.get() == PatientTab::You
                        on:click=move |_| active_tab.set(PatientTab::You)
                        class=move || {
                            if active_tab.get() == PatientTab::You {
                                "min-h-[44px] rounded-xl bg-emerald-500 px-3.5 py-2.5 text-xs font-semibold text-slate-950 shadow-sm transition sm:text-sm"
                            } else {
                                "min-h-[44px] rounded-xl px-3.5 py-2.5 text-xs font-medium text-slate-300 transition hover:bg-white/5 hover:text-white sm:text-sm"
                            }
                        }
                    >
                        "Your Health (You)"
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || active_tab.get() == PatientTab::Portals
                        on:click=move |_| active_tab.set(PatientTab::Portals)
                        class=move || {
                            if active_tab.get() == PatientTab::Portals {
                                "min-h-[44px] rounded-xl bg-emerald-500 px-3.5 py-2.5 text-xs font-semibold text-slate-950 shadow-sm transition sm:text-sm"
                            } else {
                                "min-h-[44px] rounded-xl px-3.5 py-2.5 text-xs font-medium text-slate-300 transition hover:bg-white/5 hover:text-white sm:text-sm"
                            }
                        }
                    >
                        {move || {
                            let id = selected_id.get();
                            let connected_portal_count = linked_portals.with(|map| {
                                map.get(&id)
                                    .map(|list| list.iter().filter(|c| c.status.is_connected()).count())
                                    .unwrap_or(0)
                            });
                            if connected_portal_count > 0 {
                                format!("Doctor Portals ({connected_portal_count} linked)")
                            } else {
                                "Doctor Portals (EHR)".to_string()
                            }
                        }}
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || active_tab.get() == PatientTab::Shared
                        on:click=move |_| active_tab.set(PatientTab::Shared)
                        class=move || {
                            if active_tab.get() == PatientTab::Shared {
                                "min-h-[44px] rounded-xl bg-emerald-500 px-3.5 py-2.5 text-xs font-semibold text-slate-950 shadow-sm transition sm:text-sm"
                            } else {
                                "min-h-[44px] rounded-xl px-3.5 py-2.5 text-xs font-medium text-slate-300 transition hover:bg-white/5 hover:text-white sm:text-sm"
                            }
                        }
                    >
                        {move || {
                            let id = selected_id.get();
                            let (selected_count, total_records) = charts.with(|cmap| {
                                cmap.get(&id)
                                    .map(|patient| {
                                        let state = demo_states.with(|dmap| {
                                            dmap.get(&patient.id)
                                                .cloned()
                                                .unwrap_or_else(|| default_patient_demo_state(patient))
                                        });
                                        let preview = build_doctor_share_preview(patient, &state);
                                        (preview.shared_records.len(), patient.records.len())
                                    })
                                    .unwrap_or((0, 0))
                            });
                            format!("For My Doctor ({selected_count}/{total_records})")
                        }}
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected=move || active_tab.get() == PatientTab::Roi
                        on:click=move |_| active_tab.set(PatientTab::Roi)
                        class=move || {
                            if active_tab.get() == PatientTab::Roi {
                                "min-h-[44px] rounded-xl bg-emerald-500 px-3.5 py-2.5 text-xs font-semibold text-slate-950 shadow-sm transition sm:text-sm"
                            } else {
                                "min-h-[44px] rounded-xl px-3.5 py-2.5 text-xs font-medium text-slate-300 transition hover:bg-white/5 hover:text-white sm:text-sm"
                            }
                        }
                    >
                        {move || {
                            let id = selected_id.get();
                            let roi_queued_count = roi_map.with(|map| {
                                map.get(&id)
                                    .map(|list| {
                                        list.iter()
                                            .filter(|r| {
                                                matches!(
                                                    r.status,
                                                    RoiStatus::SignedReadyToSend { .. }
                                                        | RoiStatus::Dispatched { .. }
                                                )
                                            })
                                            .count()
                                    })
                                    .unwrap_or(0)
                            });
                            format!("Release of Info (ROI · {roi_queued_count})")
                        }}
                    </button>
                </div>

                {move || {
                    let active_id = selected_id.get();
                    match active_tab.get() {
                        PatientTab::Portals => {
                            let Some(patient) = charts.with_untracked(|map| map.get(&active_id).cloned()) else {
                                return view! {
                                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 text-sm text-slate-400">
                                        "Select a fictional sample profile above."
                                    </div>
                                }
                                .into_any();
                            };
                            view! {
                                <PatientEhrPortalsPanel
                                    patient=patient
                                    linked_portals=linked_portals
                                    charts=charts
                                    allergies_map=allergies_map
                                    demo_states=demo_states
                                    on_records_updated=Callback::new(move |msg| {
                                        ehr_sync_banner.set(Some(msg));
                                    })
                                />
                            }
                            .into_any()
                        }
                        PatientTab::Roi => {
                            let Some(patient) = charts.with_untracked(|map| map.get(&active_id).cloned()) else {
                                return view! {
                                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 text-sm text-slate-400">
                                        "Select a fictional sample profile above."
                                    </div>
                                }
                                .into_any();
                            };
                            view! {
                                <AutomatedRoiPanel
                                    patient=patient
                                    roi_map=roi_map
                                />
                            }
                            .into_any()
                        }
                        PatientTab::You => {
                            let active_id_you = active_id.clone();
                            view! {
                                {move || {
                                    let Some(patient) = charts.with(|map| map.get(&active_id_you).cloned()) else {
                                        return view! {
                                            <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 text-sm text-slate-400">
                                                "Select a fictional sample profile above."
                                            </div>
                                        }
                                        .into_any();
                                    };
                                    let state = demo_states.with(|map| {
                                        map.get(&patient.id)
                                            .cloned()
                                            .unwrap_or_else(|| default_patient_demo_state(&patient))
                                    });
                                    let patient_allergies = allergies_map
                                        .with(|map| map.get(&patient.id).cloned().unwrap_or_default());
                                    let active_thread = threads
                                        .get()
                                        .get(&patient.id)
                                        .filter(|t| !t.turns.is_empty())
                                        .cloned();
                                    let cited_ids: HashSet<String> = active_thread
                                        .as_ref()
                                        .map(AgentThread::latest_cited_ids)
                                        .unwrap_or_default();
                                    let selected_set: HashSet<String> =
                                        state.selected_record_ids.iter().cloned().collect();
                                    let selected_count = patient
                                        .records
                                        .iter()
                                        .filter(|r| selected_set.contains(&r.id))
                                        .count();
                                    let total_records = patient.records.len();
                                    let presets = default_patient_questions(&patient.id);

                                    view! {
                                <div class="space-y-6">
                                    // Summary & Next Visit Card
                                    <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                                        <div class="flex flex-wrap items-start justify-between gap-3">
                                            <div>
                                                <div class="flex flex-wrap items-center gap-2">
                                                    <h2 class="text-xl font-bold text-white">
                                                        {patient.display_name.clone()}
                                                    </h2>
                                                    <span class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2.5 py-0.5 text-[11px] font-semibold text-emerald-300">
                                                        "Fictional Sample"
                                                    </span>
                                                </div>
                                                <p class="mt-1 text-xs text-slate-400">
                                                    {format!("Sample ID {} · {}", patient.mrn, patient.primary_focus)}
                                                </p>
                                            </div>
                                            {patient.visit.clone().map(|slot| view! {
                                                <div class="rounded-xl border border-cyan-400/25 bg-cyan-500/10 px-3.5 py-2 text-left sm:text-right">
                                                    <p class="font-mono text-xs font-semibold text-cyan-300">
                                                        {format!("Next Demo Visit · {} ({})", slot.starts_at, slot.modality)}
                                                    </p>
                                                    <p class="text-xs text-slate-200">{slot.reason}</p>
                                                </div>
                                            })}
                                        </div>
                                        <p class="text-sm leading-relaxed text-slate-200">
                                            {patient.summary.clone()}
                                        </p>
                                        <div class="flex flex-wrap items-center justify-between gap-3 border-t border-white/10 pt-3">
                                            <p class="text-xs text-slate-300">
                                                {format!(
                                                    "{} of {} synthetic records selected for your portable file & doctor packet.",
                                                    selected_count, total_records
                                                )}
                                            </p>
                                            <div class="flex flex-wrap gap-2">
                                                <button
                                                    type="button"
                                                    on:click=move |_| active_tab.set(PatientTab::Portals)
                                                    class="inline-flex min-h-[38px] items-center rounded-full border border-cyan-400/40 bg-cyan-500/10 px-3.5 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                                                >
                                                    "Link Doctor Portals →"
                                                </button>
                                                <button
                                                    type="button"
                                                    on:click=move |_| active_tab.set(PatientTab::Shared)
                                                    class="inline-flex min-h-[38px] items-center rounded-full border border-emerald-400/40 bg-emerald-500/15 px-3.5 py-1.5 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/25"
                                                >
                                                    "Save / Share Portable File →"
                                                </button>
                                                <button
                                                    type="button"
                                                    on:click=move |_| active_tab.set(PatientTab::Roi)
                                                    class="inline-flex min-h-[38px] items-center rounded-full border border-white/15 bg-white/5 px-3.5 py-1.5 text-xs font-semibold text-slate-200 transition hover:border-emerald-400/40 hover:text-white"
                                                >
                                                    "Automate ROI Forms →"
                                                </button>
                                            </div>
                                        </div>
                                        {(!patient_allergies.is_empty()).then(|| view! {
                                            <div class="flex flex-wrap items-center gap-2 border-t border-white/10 pt-3 text-xs">
                                                <span class="font-semibold uppercase tracking-wider text-rose-300 text-[11px]">
                                                    "Documented Allergies:"
                                                </span>
                                                {patient_allergies.iter().map(|alg| view! {
                                                    <span class="rounded-full border border-rose-400/35 bg-rose-500/10 px-2.5 py-0.5 text-[11px] text-rose-200">
                                                        {format!("{} ({})", alg.substance, alg.reaction)}
                                                    </span>
                                                }).collect_view()}
                                            </div>
                                        })}
                                    </section>

                                    // Medications & Conditions
                                    <div class="grid gap-4 md:grid-cols-2">
                                        <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5">
                                            <h3 class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                                                "Sample Medications"
                                            </h3>
                                            <ul class="mt-3 space-y-2.5">
                                                {patient
                                                    .medications
                                                    .iter()
                                                    .map(|med| {
                                                        view! {
                                                            <li class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                                                                <p class="text-sm font-semibold text-white">
                                                                    {format!("{} {}", med.name, med.dose)}
                                                                </p>
                                                                <p class="mt-0.5 text-xs text-slate-300">
                                                                    {med.schedule.clone()}
                                                                </p>
                                                                <p class="mt-1 text-[11px] text-slate-400">
                                                                    {format!("Sample purpose: {}", med.indication)}
                                                                </p>
                                                            </li>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </ul>
                                        </section>

                                        <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5">
                                            <h3 class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                                                "Active Focus Areas"
                                            </h3>
                                            <ul class="mt-3 space-y-2.5">
                                                {patient
                                                    .problems
                                                    .iter()
                                                    .map(|prob| {
                                                        view! {
                                                            <li class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                                                                <div class="flex items-center justify-between gap-2">
                                                                    <span class="font-mono text-xs font-semibold text-cyan-300">
                                                                        {prob.code.clone()}
                                                                    </span>
                                                                    <span class="text-[11px] text-emerald-300">
                                                                        {prob.status.clone()}
                                                                    </span>
                                                                </div>
                                                                <p class="mt-1 text-sm font-medium text-slate-200">
                                                                    {prob.label.clone()}
                                                                </p>
                                                            </li>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </ul>
                                        </section>
                                    </div>

                                    // Ask About Your Synthetic Records (Bounded Agentic Chat Harness)
                                    <section class="rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-5 space-y-4">
                                        <div class="flex flex-wrap items-start justify-between gap-3">
                                            <div>
                                                <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                                                    "Bounded Agentic Chat Harness (Patient Mode)"
                                                </p>
                                                <h3 class="mt-1 text-lg font-semibold text-white">
                                                    "Ask About These Synthetic Records"
                                                </h3>
                                                <p class="mt-1 text-xs text-slate-300">
                                                    "Multi-step tool-calling explanation scoped strictly to this sample profile on-device. "
                                                    "Clinician-only review drafts remain hidden and unmatched questions cite nothing."
                                                </p>
                                            </div>
                                            <div class="flex flex-wrap items-center gap-2 text-xs">
                                                <label class="inline-flex items-center gap-1.5 text-slate-300">
                                                    <span class="text-[11px] text-slate-400">"Step budget"</span>
                                                    <select
                                                        aria-label="Patient agent step budget"
                                                        class="rounded-lg border border-white/15 bg-slate-950 px-2 py-1 font-mono text-[11px] text-slate-200 focus:border-emerald-400/60 focus:outline-none"
                                                        prop:value=move || step_budget.get().to_string()
                                                        on:change=move |ev| {
                                                            if let Ok(parsed) = event_target_value(&ev).parse::<usize>() {
                                                                step_budget.set(parsed.clamp(2, 6));
                                                            }
                                                        }
                                                    >
                                                        <option value="2">"2 steps"</option>
                                                        <option value="4">"4 steps"</option>
                                                        <option value="6">"6 steps"</option>
                                                    </select>
                                                </label>
                                                <button
                                                    type="button"
                                                    on:click=clear_active_thread
                                                    class="rounded-full border border-white/15 px-3 py-1 text-[11px] text-slate-300 transition hover:border-emerald-400/40 hover:text-white"
                                                >
                                                    "Clear thread"
                                                </button>
                                            </div>
                                        </div>

                                        <AgentToolCatalogBar audience=AgentAudience::Patient />

                                        {move || {
                                            llm_status.get().filter(|s| s.enabled).map(|status| {
                                                view! {
                                                    <LocalLlmStatusBanner
                                                        status=status
                                                        use_local_llm=use_local_llm
                                                        preparing=llm_preparing
                                                        prepare_notice=llm_prepare_notice
                                                        on_prepare=prepare_llm_now
                                                    />
                                                }
                                            })
                                        }}

                                        <form on:submit=on_submit_question class="flex flex-col gap-2.5 sm:flex-row">
                                            <input
                                                type="text"
                                                aria-label="Ask a question about these synthetic sample records"
                                                placeholder="Ask about labs, glucose trends, medications, or recap (no real PHI)..."
                                                prop:value=move || question_input.get()
                                                on:input=move |ev| question_input.set(event_target_value(&ev))
                                                class="min-h-[44px] flex-1 rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2.5 text-sm text-white placeholder:text-slate-400 focus:border-emerald-400/60 focus:outline-none focus:ring-1 focus:ring-emerald-400/50"
                                            />
                                            <button
                                                type="submit"
                                                disabled=move || llm_thinking.get()
                                                class="min-h-[44px] rounded-full bg-emerald-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400 disabled:opacity-50"
                                            >
                                                {move || if llm_thinking.get() { "Synthesizing…" } else { "Run harness" }}
                                            </button>
                                        </form>

                                        <div class="flex flex-wrap gap-2">
                                            {presets
                                                .iter()
                                                .copied()
                                                .chain([
                                                    "Explain my active medications and next visit",
                                                    "Recap earlier working notes",
                                                ])
                                                .map(|preset| {
                                                    view! {
                                                        <button
                                                            type="button"
                                                            on:click=move |_| {
                                                                let q = preset.to_string();
                                                                question_input.set(q.clone());
                                                                ask_patient_question(q);
                                                            }
                                                            class="min-h-[36px] rounded-full border border-white/15 bg-slate-950/70 px-3 py-1.5 text-xs text-slate-200 transition hover:border-emerald-400/45 hover:text-white"
                                                        >
                                                            {preset}
                                                        </button>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>

                                        <div aria-live="polite" aria-label="Patient record answer">
                                            {active_thread.map(|thread| view! {
                                                <AgenticThreadView
                                                    thread=thread
                                                    active_record=active_record
                                                />
                                            })}
                                        </div>
                                    </section>

                                    // Records Timeline with Explicit "Include for My Doctor" Toggle
                                    <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                                        <div class="flex flex-wrap items-center justify-between gap-2">
                                            <div>
                                                <h3 class="text-lg font-semibold text-white">
                                                    "Your Synthetic Health Records"
                                                </h3>
                                                <p class="text-xs text-slate-300">
                                                    "Toggle any record to include or exclude it from your For My Doctor summary packet."
                                                </p>
                                            </div>
                                            <span class="rounded-full border border-white/10 bg-slate-950 px-3 py-1 font-mono text-xs text-slate-300">
                                                {format!("{selected_count}/{total_records} selected")}
                                            </span>
                                        </div>

                                        <div class="space-y-3">
                                            {patient
                                                .records
                                                .clone()
                                                .into_iter()
                                                .map(|rec| {
                                                    let is_selected = selected_set.contains(&rec.id);
                                                    let is_cited = cited_ids.contains(&rec.id);
                                                    let rec_id_toggle = rec.id.clone();
                                                    view! {
                                                        <PatientRecordCard
                                                            record=rec
                                                            is_selected=is_selected
                                                            is_cited=is_cited
                                                            active_record=active_record
                                                            on_toggle=Callback::new(move |_| toggle_record_selection(rec_id_toggle.clone()))
                                                        />
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </section>

                                    // Device-Only Private Notes (Always Excluded From Doctor Packet)
                                    <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                                        <div class="flex flex-wrap items-center justify-between gap-2">
                                            <div>
                                                <h3 class="text-base font-semibold text-white">
                                                    "Private Notes (Never Shared)"
                                                </h3>
                                                <p class="text-xs text-slate-300">
                                                    "Private notes stay strictly on this screen in demo memory and are automatically excluded from doctor packets."
                                                </p>
                                            </div>
                                            <span class="rounded-full border border-slate-600/50 bg-slate-900 px-2.5 py-1 text-[11px] font-medium text-slate-300">
                                                "Always excluded"
                                            </span>
                                        </div>

                                        <ul class="space-y-2">
                                            {state
                                                .personal_notes
                                                .iter()
                                                .map(|note| {
                                                    let note_id = note.id.clone();
                                                    let active_pid = patient.id.clone();
                                                    view! {
                                                        <li class="flex items-start justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                                                            <div>
                                                                <p class="text-[11px] font-medium text-slate-400">
                                                                    {note.created_label.clone()}
                                                                </p>
                                                                <p class="mt-1 text-xs leading-relaxed text-slate-200">
                                                                    {note.body.clone()}
                                                                </p>
                                                            </div>
                                                            <button
                                                                type="button"
                                                                on:click=move |_| {
                                                                    demo_states.update(|map| {
                                                                        if let Some(st) = map.get_mut(&active_pid) {
                                                                            st.personal_notes.retain(|n| n.id != note_id);
                                                                        }
                                                                    });
                                                                }
                                                                class="shrink-0 rounded-lg border border-white/15 px-2.5 py-1 text-[11px] text-slate-300 transition hover:border-rose-400/40 hover:text-rose-200"
                                                            >
                                                                "Remove"
                                                            </button>
                                                        </li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>

                                        <form on:submit=on_add_private_note class="flex flex-col gap-2 sm:flex-row">
                                            <input
                                                type="text"
                                                aria-label="Add a fictional private demo note"
                                                placeholder="Add a fictional private demo note (do not enter real PHI)..."
                                                prop:value=move || new_private_note.get()
                                                on:input=move |ev| new_private_note.set(event_target_value(&ev))
                                                class="min-h-[42px] flex-1 rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2 text-xs text-white placeholder:text-slate-500 focus:border-emerald-400/50 focus:outline-none"
                                            />
                                            <button
                                                type="submit"
                                                class="min-h-[42px] rounded-full border border-white/20 bg-white/5 px-4 py-2 text-xs font-semibold text-slate-100 transition hover:bg-white/10"
                                            >
                                                "Add private note"
                                            </button>
                                        </form>
                                    </section>
                                </div>
                            }
                            .into_any()
                                }}
                            }
                            .into_any()
                        }
                        PatientTab::Shared => {
                            let active_id_shared = active_id.clone();
                            view! {
                                {move || {
                                    let Some(patient) = charts.with(|map| map.get(&active_id_shared).cloned()) else {
                                        return view! {
                                            <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 text-sm text-slate-400">
                                                "Select a fictional sample profile above."
                                            </div>
                                        }
                                        .into_any();
                                    };
                                    let state = demo_states.with(|map| {
                                        map.get(&patient.id)
                                            .cloned()
                                            .unwrap_or_else(|| default_patient_demo_state(&patient))
                                    });
                                    let patient_allergies = allergies_map
                                        .with(|map| map.get(&patient.id).cloned().unwrap_or_default());
                                    let patient_portals = linked_portals
                                        .with(|map| map.get(&patient.id).cloned().unwrap_or_default());
                                    let preview = build_doctor_share_preview(&patient, &state);
                                    let active_pid_note = patient.id.clone();
                                    let active_pid_send = patient.id.clone();
                                    let active_pid_withdraw = patient.id.clone();
                                    let active_pid_q = patient.id.clone();
                                    let default_conn = default_patient_demo_state(&patient).connection;
                                    let paused_conn = DemoConnectionState::Paused {
                                        practice_name: "Dabney.moe Synthetic Clinic".into(),
                                    };
                                    let delivery = state.delivery_status;
                                    let can_send = preview.sharing_enabled
                                        && (!preview.shared_records.is_empty()
                                            || !preview.visit_note.is_empty()
                                            || !preview.visit_questions.is_empty());
                                    let sent_records_count = preview.shared_records.len();
                                    let sent_questions_count = preview.visit_questions.len();

                                    view! {
                                <div class="space-y-6">
                                    // Portable Shareable Health Passport File (persist & share with ANY doctor or person)
                                    <PortablePassportSection
                                        patient=patient.clone()
                                        state=state.clone()
                                        allergies=patient_allergies
                                        linked_portals=patient_portals
                                        audience=passport_audience
                                        emergency_note=emergency_note
                                        on_restore_passport=restore_from_passport
                                    />

                                    // Practice Connection Control
                                    <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                                        <div class="flex flex-wrap items-start justify-between gap-3">
                                            <div>
                                                <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                                                    "Patient-Controlled Practice Connection (Simulated)"
                                                </p>
                                                <h2 class="mt-1 text-lg font-semibold text-white">
                                                    {state.connection.practice_label()}
                                                </h2>
                                                <p class="mt-1 text-xs text-slate-300">
                                                    "Nothing is ever shared automatically. Pause or disconnect at any time to see how sharing controls behave."
                                                </p>
                                            </div>
                                            <span class=if state.connection.allows_sharing() {
                                                "rounded-full border border-emerald-400/40 bg-emerald-500/15 px-3 py-1 text-xs font-semibold text-emerald-300"
                                            } else {
                                                "rounded-full border border-amber-400/40 bg-amber-500/15 px-3 py-1 text-xs font-semibold text-amber-200"
                                            }>
                                                {state.connection.label()}
                                            </span>
                                        </div>
                                        <div class="flex flex-wrap gap-2">
                                            <button
                                                type="button"
                                                on:click=move |_| set_connection_mode(default_conn.clone())
                                                class=if matches!(state.connection, DemoConnectionState::Connected { .. }) {
                                                    "min-h-[40px] rounded-full bg-emerald-500 px-4 py-2 text-xs font-semibold text-slate-950"
                                                } else {
                                                    "min-h-[40px] rounded-full border border-white/15 px-4 py-2 text-xs font-medium text-slate-200 hover:bg-white/5"
                                                }
                                            >
                                                "Connected (demo)"
                                            </button>
                                            <button
                                                type="button"
                                                on:click=move |_| set_connection_mode(paused_conn.clone())
                                                class=if matches!(state.connection, DemoConnectionState::Paused { .. }) {
                                                    "min-h-[40px] rounded-full bg-amber-400 px-4 py-2 text-xs font-semibold text-slate-950"
                                                } else {
                                                    "min-h-[40px] rounded-full border border-white/15 px-4 py-2 text-xs font-medium text-slate-200 hover:bg-white/5"
                                                }
                                            >
                                                "Pause sharing"
                                            </button>
                                            <button
                                                type="button"
                                                on:click=move |_| set_connection_mode(DemoConnectionState::Standalone)
                                                class=if matches!(state.connection, DemoConnectionState::Standalone) {
                                                    "min-h-[40px] rounded-full bg-cyan-400 px-4 py-2 text-xs font-semibold text-slate-950"
                                                } else {
                                                    "min-h-[40px] rounded-full border border-white/15 px-4 py-2 text-xs font-medium text-slate-200 hover:bg-white/5"
                                                }
                                            >
                                                "Standalone (no clinic)"
                                            </button>
                                        </div>
                                    </section>

                                    // Visit Note & Questions Editor
                                    <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 space-y-4">
                                        <div>
                                            <h3 class="text-base font-semibold text-white">
                                                "Your Note & Questions for the Demo Visit"
                                            </h3>
                                            <p class="mt-0.5 text-xs text-slate-300">
                                                "Edit the synthetic visit note or add sample questions below. Demo only—do not enter real PHI."
                                            </p>
                                        </div>
                                        <textarea
                                            rows="3"
                                            aria-label="Synthetic visit note for demo clinician"
                                            placeholder="Fictional visit note (do not enter real personal health information)..."
                                            prop:value=state.visit_note.clone()
                                            on:input=move |ev| {
                                                let val = event_target_value(&ev);
                                                let pid = active_pid_note.clone();
                                                demo_states.update(|map| {
                                                    if let Some(st) = map.get_mut(&pid) {
                                                        st.visit_note = val;
                                                        st.delivery_status = ShareDeliveryStatus::Draft;
                                                    }
                                                });
                                            }
                                            class="w-full rounded-xl border border-white/15 bg-slate-950 p-3 text-sm text-white placeholder:text-slate-500 focus:border-emerald-400/60 focus:outline-none"
                                        />

                                        <div class="space-y-2">
                                            <p class="text-xs font-semibold uppercase tracking-wider text-slate-300">
                                                "Visit Questions"
                                            </p>
                                            <ul class="space-y-2">
                                                {state
                                                    .visit_questions
                                                    .iter()
                                                    .enumerate()
                                                    .map(|(idx, q)| {
                                                        let pid = active_pid_q.clone();
                                                        view! {
                                                            <li class="flex items-center justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/60 px-3.5 py-2.5 text-xs text-slate-100">
                                                                <span>{q.clone()}</span>
                                                                <button
                                                                    type="button"
                                                                    on:click=move |_| {
                                                                        demo_states.update(|map| {
                                                                            if let Some(st) = map.get_mut(&pid) {
                                                                                if idx < st.visit_questions.len() {
                                                                                    st.visit_questions.remove(idx);
                                                                                    st.delivery_status = ShareDeliveryStatus::Draft;
                                                                                }
                                                                            }
                                                                        });
                                                                    }
                                                                    class="shrink-0 rounded-lg border border-white/15 px-2.5 py-1 text-[11px] text-slate-300 hover:border-rose-400/40 hover:text-rose-200"
                                                                >
                                                                    "Remove"
                                                                </button>
                                                            </li>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </ul>

                                            <form on:submit=on_add_visit_question class="flex flex-col gap-2 sm:flex-row">
                                                <input
                                                    type="text"
                                                    aria-label="Add a fictional question for the demo visit"
                                                    placeholder="Add a fictional demo question (no real PHI)..."
                                                    prop:value=move || new_visit_question.get()
                                                    on:input=move |ev| new_visit_question.set(event_target_value(&ev))
                                                    class="min-h-[42px] flex-1 rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2 text-xs text-white placeholder:text-slate-500 focus:border-emerald-400/50 focus:outline-none"
                                                />
                                                <button
                                                    type="submit"
                                                    class="min-h-[42px] rounded-full border border-emerald-400/40 bg-emerald-500/15 px-4 py-2 text-xs font-semibold text-emerald-200 hover:bg-emerald-500/25"
                                                >
                                                    "Add question"
                                                </button>
                                            </form>
                                        </div>
                                    </section>

                                    // Exact Clinician-Visible Visit Packet Preview
                                    <SharePacketPreviewCard
                                        preview=preview
                                        delivery=delivery
                                        can_send=can_send
                                        on_toggle_record=Callback::new(toggle_record_selection)
                                        on_simulate_send=Callback::new(move |_| {
                                            let pid = active_pid_send.clone();
                                            demo_states.update(|map| {
                                                if let Some(st) = map.get_mut(&pid) {
                                                    st.delivery_status =
                                                        ShareDeliveryStatus::SimulatedLocalSend {
                                                            sent_record_count: sent_records_count,
                                                            question_count: sent_questions_count,
                                                        };
                                                }
                                            });
                                        })
                                        on_withdraw=Callback::new(move |_| {
                                            let pid = active_pid_withdraw.clone();
                                            demo_states.update(|map| {
                                                if let Some(st) = map.get_mut(&pid) {
                                                    st.delivery_status =
                                                        ShareDeliveryStatus::Withdrawn;
                                                }
                                            });
                                        })
                                        on_back_to_records=Callback::new(move |_| {
                                            active_tab.set(PatientTab::You);
                                        })
                                    />
                                </div>
                            }
                            .into_any()
                                }}
                            }
                            .into_any()
                        }
                    }
                }}
            </div>
        </div>

        <Footer />
    }
}

#[component]
fn PatientRecordCard(
    record: ChartRecord,
    is_selected: bool,
    is_cited: bool,
    active_record: RwSignal<Option<String>>,
    on_toggle: Callback<()>,
) -> impl IntoView {
    let kind_cls = record_kind_badge_class(record.kind);
    let anchor_id = format!("record-{}", record.id);
    let active_key = record.id.clone();
    let container_cls = move || {
        let is_active = active_record.with(|active| active.as_deref() == Some(active_key.as_str()));
        if is_active {
            "scroll-mt-24 rounded-xl border border-cyan-300 bg-cyan-500/[0.14] p-4 ring-2 ring-cyan-300/60 transition"
        } else if is_selected {
            "scroll-mt-24 rounded-xl border border-emerald-400/50 bg-emerald-500/[0.07] p-4 transition"
        } else if is_cited {
            "scroll-mt-24 rounded-xl border border-cyan-400/50 bg-cyan-500/[0.06] p-4 transition"
        } else {
            "scroll-mt-24 rounded-xl border border-white/10 bg-slate-950/60 p-4 transition"
        }
    };

    view! {
        <article id=anchor_id class=container_cls>
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="flex flex-wrap items-center gap-2">
                    <span class="rounded bg-white/10 px-2 py-0.5 font-mono text-xs font-medium text-slate-200">
                        {record.id}
                    </span>
                    <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {kind_cls}")>
                        {record.kind.label()}
                    </span>
                    {is_cited.then(|| view! {
                        <span class="rounded-full border border-cyan-400/40 bg-cyan-500/15 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-cyan-200">
                            "Cited"
                        </span>
                    })}
                </div>
                <span class="font-mono text-xs text-slate-400">
                    {format!("{} · {}", record.recorded_at, record.source_label)}
                </span>
            </div>

            <h4 class="mt-2.5 text-sm font-semibold text-white">{record.title}</h4>
            <p class="mt-1.5 text-xs leading-relaxed text-slate-300">{record.body}</p>

            {(!record.key_values.is_empty()).then(|| view! {
                <dl class="mt-3 flex flex-wrap gap-2">
                    {record
                        .key_values
                        .into_iter()
                        .map(|(k, v)| {
                            view! {
                                <div class="rounded-lg border border-white/10 bg-slate-900/80 px-2.5 py-1 text-xs">
                                    <dt class="inline text-slate-400">{format!("{k}: ")}</dt>
                                    <dd class="inline font-semibold text-slate-100">{v}</dd>
                                </div>
                            }
                        })
                        .collect_view()}
                </dl>
            })}

            <div class="mt-3.5 flex flex-wrap items-center justify-between gap-2 border-t border-white/10 pt-3">
                <span class="text-[11px] text-slate-300">
                    {if is_selected {
                        "Marked for your For My Doctor summary packet"
                    } else {
                        "Kept private on your health timeline"
                    }}
                </span>
                <button
                    type="button"
                    aria-pressed=is_selected
                    on:click=move |_| on_toggle.run(())
                    class=if is_selected {
                        "min-h-[38px] rounded-full bg-emerald-500 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                    } else {
                        "min-h-[38px] rounded-full border border-white/20 bg-white/5 px-3.5 py-1.5 text-xs font-semibold text-slate-100 transition hover:border-emerald-400/50 hover:bg-emerald-500/15"
                    }
                >
                    {if is_selected {
                        "Included for doctor ✓"
                    } else {
                        "+ Include for my doctor"
                    }}
                </button>
            </div>
        </article>
    }
}

#[component]
fn SharePacketPreviewCard(
    preview: SharePacketPreview,
    delivery: ShareDeliveryStatus,
    can_send: bool,
    on_toggle_record: Callback<String>,
    on_simulate_send: Callback<()>,
    on_withdraw: Callback<()>,
    on_back_to_records: Callback<()>,
) -> impl IntoView {
    let status_cls = match delivery {
        ShareDeliveryStatus::Draft => "border-cyan-400/35 bg-cyan-500/10 text-cyan-200",
        ShareDeliveryStatus::SimulatedLocalSend { .. } => {
            "border-emerald-400/40 bg-emerald-500/15 text-emerald-200"
        }
        ShareDeliveryStatus::Withdrawn => "border-amber-400/40 bg-amber-500/15 text-amber-200",
    };

    view! {
        <section class="rounded-2xl border border-emerald-400/30 bg-slate-900/70 p-5 space-y-4">
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-white/10 pb-3">
                <div>
                    <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                        "Exact Visit Packet Preview"
                    </p>
                    <h3 class="mt-1 text-lg font-semibold text-white">
                        {format!("For {} · {}", preview.patient_display_name, preview.patient_mrn)}
                    </h3>
                    <p class="mt-0.5 text-xs text-slate-300">
                        {format!("Destination (simulated): {}", preview.practice_label)}
                    </p>
                </div>
                <span
                    aria-live="polite"
                    class=format!("rounded-full border px-3 py-1 text-xs font-semibold {status_cls}")
                >
                    {delivery.badge_label()}
                </span>
            </div>

            <div class="rounded-xl border border-emerald-400/25 bg-emerald-500/[0.06] px-3.5 py-2.5 text-xs text-emerald-100">
                {preview.synthetic_disclaimer}
            </div>

            // Privacy Boundary Summary
            <div class="grid gap-2.5 sm:grid-cols-3 text-xs">
                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-slate-400">"Included records"</p>
                    <p class="mt-1 text-lg font-bold text-emerald-300">
                        {preview.shared_records.len()}
                    </p>
                </div>
                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-slate-400">"Excluded records"</p>
                    <p class="mt-1 text-lg font-bold text-slate-200">
                        {preview.excluded_record_count}
                    </p>
                </div>
                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-slate-400">"Private notes excluded"</p>
                    <p class="mt-1 text-lg font-bold text-cyan-300">
                        {preview.private_note_count}
                    </p>
                </div>
            </div>

            // Selected Records List
            <div class="space-y-2">
                <div class="flex items-center justify-between gap-2">
                    <h4 class="text-xs font-semibold uppercase tracking-wider text-slate-300">
                        "Records Included in This Packet"
                    </h4>
                    <button
                        type="button"
                        on:click=move |_| on_back_to_records.run(())
                        class="text-xs font-semibold text-cyan-300 hover:text-cyan-200"
                    >
                        "Edit selections in Your Health →"
                    </button>
                </div>
                {if preview.shared_records.is_empty() {
                    view! {
                        <p class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-xs text-slate-400">
                            "No records are currently selected. Go to Your Health to choose synthetic records to include."
                        </p>
                    }
                    .into_any()
                } else {
                    view! {
                        <ul class="space-y-2">
                            {preview
                                .shared_records
                                .into_iter()
                                .map(|rec| {
                                    let rec_id = rec.id.clone();
                                    view! {
                                        <li class="flex items-start justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/60 p-3">
                                            <div>
                                                <p class="text-xs font-semibold text-white">
                                                    {format!("{} · {}", rec.title, rec.recorded_at)}
                                                </p>
                                                <p class="mt-1 text-xs text-slate-300">{rec.body}</p>
                                            </div>
                                            <button
                                                type="button"
                                                on:click=move |_| on_toggle_record.run(rec_id.clone())
                                                class="shrink-0 rounded-lg border border-white/15 px-2.5 py-1 text-[11px] text-slate-300 hover:border-rose-400/40 hover:text-rose-200"
                                            >
                                                "Exclude"
                                            </button>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    }
                    .into_any()
                }}
            </div>

            // Packet Actions
            <div class="space-y-2.5 border-t border-white/10 pt-3">
                {match delivery {
                    ShareDeliveryStatus::SimulatedLocalSend {
                        sent_record_count,
                        question_count,
                    } => view! {
                        <p class="rounded-xl border border-emerald-400/35 bg-emerald-500/10 p-3 text-xs text-emerald-100">
                            {format!(
                                "Simulated local send complete ({sent_record_count} synthetic record(s), {question_count} question(s)). \
                                 Zero network requests were made and zero data left your device."
                            )}
                        </p>
                    }
                    .into_any(),
                    ShareDeliveryStatus::Withdrawn => view! {
                        <p class="rounded-xl border border-amber-400/35 bg-amber-500/10 p-3 text-xs text-amber-100">
                            "Packet withdrawn in this local demo. No clinician view holds a pending copy."
                        </p>
                    }
                    .into_any(),
                    ShareDeliveryStatus::Draft => {
                        if !preview.sharing_enabled {
                            view! {
                                <p class="text-xs text-amber-200">
                                    "Sharing is currently paused or standalone. Switch back to Connected (demo) above to test simulated local sending."
                                </p>
                            }
                            .into_any()
                        } else {
                            view! {
                                <p class="text-xs text-slate-400">
                                    "Ready to test simulated local delivery. Clicking below updates local demo state only."
                                </p>
                            }
                            .into_any()
                        }
                    }
                }}

                <div class="flex flex-wrap gap-2">
                    <button
                        type="button"
                        disabled=!can_send
                        on:click=move |_| on_simulate_send.run(())
                        class=if can_send {
                            "min-h-[42px] rounded-full bg-emerald-500 px-5 py-2 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                        } else {
                            "min-h-[42px] cursor-not-allowed rounded-full bg-slate-800 px-5 py-2 text-xs font-semibold text-slate-500"
                        }
                    >
                        "Simulate local send (no network)"
                    </button>
                    {matches!(delivery, ShareDeliveryStatus::SimulatedLocalSend { .. }).then(|| view! {
                        <button
                            type="button"
                            on:click=move |_| on_withdraw.run(())
                            class="min-h-[42px] rounded-full border border-amber-400/40 bg-amber-500/10 px-4 py-2 text-xs font-semibold text-amber-200 transition hover:bg-amber-500/20"
                        >
                            "Withdraw packet"
                        </button>
                    })}
                </div>
            </div>
        </section>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use leptos_meta::provide_meta_context;
    use leptos_router::components::Router;
    use leptos_router::location::RequestUrl;

    use crate::consent::provide_consent_context;

    fn render_with_mode(path: &str, mode: AppShellMode) -> String {
        let owner = Owner::new();
        let html = owner.with(|| {
            provide_meta_context();
            provide_consent_context();
            provide_context(mode);
            provide_context(RequestUrl::new(path));
            view! {
                <Router>
                    <PatientDemoPage />
                </Router>
            }
            .to_html()
        });
        drop(owner);
        html.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    #[test]
    fn patient_demo_page_renders_zero_phi_notice_and_synthetic_profiles() {
        let html = render_with_mode("/clinical/patient", AppShellMode::Website);
        assert!(html.contains("Patient Experience Demo"));
        assert!(html.contains("100% Synthetic Demo"));
        assert!(html.contains("Zero PHI"));
        assert!(html.contains("/dabney-elephant.svg"));
        assert!(html.contains("Nadia Kulkarni"));
        assert!(html.contains("Carlos Mendez"));
        assert!(html.contains("Priya Nair"));
        assert!(html.contains("David Okafor"));
        assert!(html.contains("Your Health (You)"));
        assert!(html.contains("Doctor Portals (EHR)"));
        assert!(html.contains("For My Doctor"));
        assert!(html.contains("Release of Info (ROI"));
        assert!(html.contains("Private Notes (Never Shared)"));
        assert!(html.contains("Included for doctor"));
        assert!(html.contains("Bounded Agentic Chat Harness (Patient Mode)"));
        assert!(html.contains("chart_search"));
        assert!(html.contains("working_note_save"));
        assert!(html.contains(r#"id="record-nk-cgm-01""#));
        let legacy_brand = ["co", "da"].concat();
        assert!(
            !html.to_ascii_lowercase().contains(&legacy_brand),
            "patient demo HTML must not contain legacy brand strings"
        );
    }

    #[test]
    fn clinical_demo_app_mode_hides_website_hire_nav_and_shows_web_link() {
        let html = render_with_mode("/", AppShellMode::ClinicalDemoApp);
        assert!(html.contains("Patient Experience Demo"));
        assert!(html.contains("Open on dabney.moe"));
        assert!(!html.contains("Hire me"));
        assert!(!html.contains("Commissions"));
    }
}
