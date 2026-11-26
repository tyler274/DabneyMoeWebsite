//! Interactive Dabney.moe Mental Health & Comprehensive Commercial CGM tracking workspace (`/mental-health`).

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;

#[cfg(any(feature = "hydrate", feature = "csr"))]
use crate::health_bridge::{
    build_personal_cgm_chat_prompt_with_analytics, generate_local_llm, invoke_tauri_command,
    load_personal_vault, prepare_local_llm, probe_local_llm, probe_personal_health_bridge,
    probe_personal_vault, save_personal_vault, wipe_personal_vault, HealthBridgePermissionUi,
    HealthBridgeReadUi, PersonalVaultSaveUi,
};
use crate::health_bridge::{
    collect_cgm_grounding_facts_with_analytics, synthesize_grounded_cgm_answer_with_analytics,
    HealthBridgeStatusUi, LocalChatTurn, LocalLlmStatusUi, PersonalVaultStatusUi,
};
use crate::local_ai_chat::{PersonalCgmAiChatSection, PersonalVaultBar};
use crate::mental_health_3d::LingoMood3DChart;
#[cfg(any(feature = "hydrate", feature = "csr"))]
use crate::mental_health_cgm::{export_health_bridge_json_compact, monitor_hint_from_filename};
use crate::mental_health_cgm::{
    nominal_window_hours_for_trace, parse_commercial_glucose_input, parse_timestamp,
    sample_csv_for_monitor, select_notable_spike_peaks, slice_readings_window,
    summarize_commercial_glucose_with_window, AdaTimeInRangeBar, AgpPercentileChart,
    CommercialGlucoseImport, GlucoseImportDiagnostics, GlucoseMonitor, GlucoseUnit, GlucoseWindow,
    MeasurementMethod, ValidationLevel, SAMPLE_APPLE_HEALTH_XML, SAMPLE_HEALTH_BRIDGE_JSON,
};
use crate::mental_health_cgm_analytics::{
    analyze_glucose_trace, downsample_segments_for_chart, DEFAULT_CHART_GAP_HOURS,
};
use crate::mental_health_cgm_views::{
    CircadianBreakdownSection, DailyGlucoseAndSessionsSection, GlycemicEpisodesAndRocSection,
    PeriodComparisonCard, SensorSaturationBanner,
};
use crate::mental_health_episodes::{
    summarize_episode_cadence, EpisodeActivationSection, TopCoolDownBanner,
};
use crate::mental_health_exports::{
    load_synthetic_4platform_synthesis, synthesize_opted_in_events, PlatformConsentState,
};
use crate::mental_health_lingo::{
    build_assessment_series, classify_gad7, classify_phq9, correlate_glucose_and_mood,
    csv_data_uri, export_correlations_csv, sample_assessment_history, sample_checkins,
    AssessmentPoint, LingoReading, MentalHealthCheckIn, SAMPLE_LINGO_CSV,
};
use crate::mental_health_support::SupportAndClinicianBriefSection;
use crate::patient_demo::ClinicalSuiteSwitcher;
use crate::sections::{Footer, NavBar};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChartViewMode {
    WebGl3D,
    Svg2D,
}

const CYAN: &str = "#22d3ee";
const EMERALD: &str = "#34d399";
const AMBER: &str = "#fbbf24";
const ROSE: &str = "#fb7185";
const VIOLET: &str = "#a78bfa";
const SLATE_LIGHT: &str = "#94a3b8";

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn compact_textarea_preview(raw: &str) -> String {
    if raw.len() <= 24_576 {
        return raw.to_string();
    }
    let head: String = raw.lines().take(20).collect::<Vec<_>>().join("\n");
    format!(
        "{head}\n# … preview truncated ({:.1} KB total; full file parsed locally in memory)",
        raw.len() as f64 / 1024.0
    )
}

#[component]
pub fn MentalHealthPage() -> impl IntoView {
    let initial_import = parse_commercial_glucose_input(SAMPLE_LINGO_CSV, None).ok();
    let csv_input = RwSignal::new(SAMPLE_LINGO_CSV.to_string());
    let monitor_hint = RwSignal::new(Option::<GlucoseMonitor>::None);
    let detected_monitor = RwSignal::new(
        initial_import
            .as_ref()
            .map(|i| i.monitor)
            .unwrap_or(GlucoseMonitor::AbbottLingo),
    );
    let source_unit = RwSignal::new(
        initial_import
            .as_ref()
            .map(|i| i.source_unit)
            .unwrap_or(GlucoseUnit::MgDl),
    );
    let measurement_method = RwSignal::new(
        initial_import
            .as_ref()
            .map(|i| i.method)
            .unwrap_or(MeasurementMethod::InterstitialCgm),
    );
    let import_diagnostics = RwSignal::new(initial_import.as_ref().map(|i| i.diagnostics.clone()));
    let readings = RwSignal::new(initial_import.map(|i| i.readings).unwrap_or_default());
    let window = RwSignal::new(GlucoseWindow::All);
    let loaded_file_label = RwSignal::new(Option::<String>::None);
    let csv_error = RwSignal::new(Option::<String>::None);

    // Native Android Health Connect / iOS Apple HealthKit state (active ONLY inside
    // `moe.dabney.personal` built with `--features health-connect`; stays `None` on
    // web browsers and `moe.dabney.demo`).
    let hc_status = RwSignal::new(Option::<HealthBridgeStatusUi>::None);
    let hc_busy = RwSignal::new(false);
    let hc_message = RwSignal::new(Option::<String>::None);
    let hc_error = RwSignal::new(Option::<String>::None);

    // Hardware-backed encrypted local personal vault (`moe.dabney.personal` only).
    let vault_status = RwSignal::new(Option::<PersonalVaultStatusUi>::None);
    let vault_busy = RwSignal::new(false);
    let vault_notice = RwSignal::new(Option::<String>::None);

    // On-device LLM (Android AICore / Gemini Nano) + local CGM/mood chat state.
    let llm_status = RwSignal::new(Option::<LocalLlmStatusUi>::None);
    let use_local_llm = RwSignal::new(true);
    let llm_preparing = RwSignal::new(false);
    let llm_prepare_notice = RwSignal::new(Option::<String>::None);
    let chat_input = RwSignal::new(String::new());
    let chat_turns = RwSignal::new(Vec::<LocalChatTurn>::new());
    let chat_busy = RwSignal::new(false);
    let chat_error = RwSignal::new(Option::<String>::None);
    let next_chat_seq = RwSignal::new(1usize);

    let windowed_readings =
        Memo::new(move |_| readings.with(|r| slice_readings_window(r, window.get())));
    let glucose_summary = Memo::new(move |_| {
        let nominal_h = readings.with(|full| nominal_window_hours_for_trace(full, window.get()));
        windowed_readings.with(|sliced| {
            summarize_commercial_glucose_with_window(
                sliced,
                detected_monitor.get(),
                source_unit.get(),
                measurement_method.get(),
                nominal_h,
            )
        })
    });
    let glucose_analytics = Memo::new(move |_| {
        windowed_readings.with(|sliced| {
            readings.with(|full| {
                analyze_glucose_trace(
                    sliced,
                    detected_monitor.get(),
                    measurement_method.get(),
                    Some(full),
                )
            })
        })
    });

    let checkins = RwSignal::new(sample_checkins());
    let user_added_checkins = RwSignal::new(false);
    let correlations = Memo::new(move |_| {
        windowed_readings.with(|r| checkins.with(|c| correlate_glucose_and_mood(r, c)))
    });
    let max_checkin_hour = Memo::new(move |_| {
        windowed_readings
            .with(|r| r.last().map(|x| x.hour_offset).unwrap_or(24.0))
            .max(24.0)
    });
    let chart_mode = RwSignal::new(ChartViewMode::WebGl3D);
    let asrm_score = RwSignal::new(Some(7u8));
    let platform_consent = RwSignal::new(PlatformConsentState::default());
    let activity_events = RwSignal::new(load_synthetic_4platform_synthesis());
    let synthesized_events = Memo::new(move |_| {
        activity_events.with(|ev| synthesize_opted_in_events(ev, &platform_consent.get()))
    });

    // Form state for adding a new mental health check-in.
    let mood_input = RwSignal::new(6u8);
    let anxiety_input = RwSignal::new(5u8);
    let energy_input = RwSignal::new(6u8);
    let sleep_input = RwSignal::new("7.0".to_string());
    let parsed_sleep_hours = Memo::new(move |_| {
        sleep_input
            .get()
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=24.0).contains(v))
            .unwrap_or(7.0)
    });
    let phq9_input = RwSignal::new(String::new());
    let gad7_input = RwSignal::new(String::new());
    let note_input = RwSignal::new(String::new());
    let hour_input = RwSignal::new("18.0".to_string());
    let form_status = RwSignal::new(Option::<String>::None);

    let apply_parsed_glucose = move |parsed: CommercialGlucoseImport,
                                     source_label: Option<String>| {
        let span_h = parsed.diagnostics.span_hours;
        let is_personal_import = source_label.is_some();
        detected_monitor.set(parsed.monitor);
        source_unit.set(parsed.source_unit);
        measurement_method.set(parsed.method);
        import_diagnostics.set(Some(parsed.diagnostics));
        readings.set(parsed.readings);
        loaded_file_label.set(source_label);
        // Clear synthetic sample check-ins when loading a personal trace unless the user has
        // already logged/restored personal check-ins, avoiding synthetic notes squashed onto
        // the left edge of a multi-month real export.
        if is_personal_import && !user_added_checkins.get_untracked() {
            checkins.set(Vec::new());
        }
        // Default multi-week personal exports to the ATTD 14-day view so charts & AGP stay crisp.
        window.set(if span_h > 360.0 {
            GlucoseWindow::Days14
        } else {
            GlucoseWindow::All
        });
        csv_error.set(None);
    };

    let apply_glucose_text =
        move |raw: &str, hint: Option<GlucoseMonitor>, source_label: Option<String>| {
            match parse_commercial_glucose_input(raw, hint) {
                Ok(parsed) => apply_parsed_glucose(parsed, source_label),
                Err(err) => {
                    csv_error.set(Some(err));
                }
            }
        };

    let apply_csv = move |_| {
        let raw = csv_input.get();
        apply_glucose_text(&raw, monitor_hint.get(), None);
    };

    let on_cgm_file_change = move |ev: leptos::ev::Event| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            use wasm_bindgen::closure::Closure;
            use wasm_bindgen::JsCast;

            let Some(target) = ev.target() else { return };
            let Ok(input_el) = target.dyn_into::<web_sys::HtmlInputElement>() else {
                return;
            };
            let Some(files) = input_el.files() else {
                return;
            };
            let Some(file) = files.get(0) else { return };
            let file_name = file.name();
            let file_kb = file.size() / 1024.0;
            let inferred_hint = monitor_hint
                .get()
                .or_else(|| monitor_hint_from_filename(&file_name));
            let Ok(reader) = web_sys::FileReader::new() else {
                csv_error.set(Some("Could not initialize browser FileReader.".into()));
                return;
            };
            let onload = Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(
                move |pe: web_sys::ProgressEvent| {
                    let Some(t) = pe.target() else { return };
                    let Ok(r) = t.dyn_into::<web_sys::FileReader>() else {
                        return;
                    };
                    if let Ok(val) = r.result() {
                        if let Some(text) = val.as_string() {
                            csv_input.set(compact_textarea_preview(&text));
                            let label = format!("{file_name} ({file_kb:.1} KB · on-device)");
                            match parse_commercial_glucose_input(&text, inferred_hint) {
                                Ok(parsed) => {
                                    if vault_status.get_untracked().is_some_and(|v| v.enabled) {
                                        let count = parsed.readings.len();
                                        let bridge_json = export_health_bridge_json_compact(
                                            &parsed,
                                            "local_file",
                                        );
                                        let save_label = label.clone();
                                        save_personal_vault(
                                            &PersonalVaultSaveUi {
                                                cgm_bridge_json: Some(bridge_json),
                                                cgm_source_label: Some(save_label),
                                                cgm_reading_count: Some(count),
                                                ..Default::default()
                                            },
                                            move |res| {
                                                if let Ok(updated) = res {
                                                    vault_status.set(Some(updated));
                                                    vault_notice.set(Some(
                                                        "Encrypted & cached imported CGM file in local vault."
                                                            .to_string(),
                                                    ));
                                                }
                                            },
                                        );
                                    }
                                    apply_parsed_glucose(parsed, Some(label));
                                }
                                Err(err) => {
                                    csv_error.set(Some(err));
                                }
                            }
                        }
                    }
                },
            );
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            if reader.read_as_text(&file).is_err() {
                csv_error.set(Some("Could not read selected glucose file.".into()));
            }
        }
        #[cfg(not(any(feature = "hydrate", feature = "csr")))]
        let _ = ev;
    };

    let load_monitor_preset = move |monitor: GlucoseMonitor| {
        let sample = sample_csv_for_monitor(monitor);
        monitor_hint.set(None);
        csv_input.set(sample.to_string());
        apply_glucose_text(sample, None, None);
    };

    let load_raw_preset = move |sample: &'static str| {
        monitor_hint.set(None);
        csv_input.set(sample.to_string());
        apply_glucose_text(sample, None, None);
    };

    let reset_sample = move |_| {
        load_monitor_preset(GlucoseMonitor::AbbottLingo);
        user_added_checkins.set(false);
        checkins.set(sample_checkins());
        form_status.set(None);
        hc_message.set(None);
        hc_error.set(None);
    };

    let refresh_health_connect_status = move || {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            probe_personal_health_bridge(move |status| {
                hc_status.set(Some(status));
            });
        }
    };

    let refresh_local_llm_status = move || {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            probe_local_llm(move |status| {
                llm_status.set(Some(status));
            });
        }
    };

    #[cfg(any(feature = "hydrate", feature = "csr"))]
    let persist_vault_update = move |payload: PersonalVaultSaveUi, saved_note: &'static str| {
        if vault_status.get_untracked().is_some_and(|v| v.enabled) {
            save_personal_vault(&payload, move |res| match res {
                Ok(updated) => {
                    vault_status.set(Some(updated));
                    vault_notice.set(Some(saved_note.to_string()));
                }
                Err(err) => {
                    vault_notice.set(Some(format!("Local vault save warning: {err}")));
                }
            });
        }
    };

    Effect::new(move |_| {
        refresh_health_connect_status();
        refresh_local_llm_status();
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            probe_personal_vault(move |status| {
                let has_saved_data = status.has_cgm_snapshot
                    || status.checkin_count > 0
                    || status.chat_turn_count > 0;
                vault_status.set(Some(status));
                if has_saved_data {
                    load_personal_vault(move |res| {
                        if let Ok(snap) = res {
                            if !snap.found {
                                return;
                            }
                            if let Some(cgm_json) = snap.cgm_bridge_json {
                                let label = snap
                                    .cgm_source_label
                                    .unwrap_or_else(|| "Encrypted Local Vault".to_string());
                                csv_input.set(compact_textarea_preview(&cgm_json));
                                apply_glucose_text(
                                    &cgm_json,
                                    None,
                                    Some(format!("{label} · restored from AES-256-GCM vault")),
                                );
                            }
                            if let Some(checkins_raw) = snap.checkins_json {
                                if let Ok(restored) =
                                    serde_json::from_str::<Vec<MentalHealthCheckIn>>(&checkins_raw)
                                {
                                    if !restored.is_empty() {
                                        user_added_checkins.set(true);
                                        checkins.set(restored);
                                    }
                                }
                            }
                            if let Some(chat_raw) = snap.chat_history_json {
                                if let Ok(restored) =
                                    serde_json::from_str::<Vec<LocalChatTurn>>(&chat_raw)
                                {
                                    next_chat_seq.set(restored.len() + 1);
                                    chat_turns.set(restored);
                                }
                            }
                            vault_notice.set(Some(
                                "Restored encrypted personal snapshot from Android Keystore vault."
                                    .to_string(),
                            ));
                        }
                    });
                }
            });
        }
    });

    let wipe_vault_now = Callback::new(move |()| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            vault_busy.set(true);
            wipe_personal_vault(move |res| {
                vault_busy.set(false);
                match res {
                    Ok(empty_status) => {
                        vault_status.set(Some(empty_status));
                        load_monitor_preset(GlucoseMonitor::AbbottLingo);
                        user_added_checkins.set(false);
                        checkins.set(sample_checkins());
                        chat_turns.set(Vec::new());
                        form_status.set(None);
                        hc_message.set(None);
                        hc_error.set(None);
                        vault_notice.set(Some(
                            "Encrypted vault wiped and Android Keystore master key destroyed."
                                .to_string(),
                        ));
                    }
                    Err(err) => {
                        vault_notice.set(Some(format!("Vault wipe failed: {err}")));
                    }
                }
            });
        }
    });

    let prepare_local_llm_now = Callback::new(move |()| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
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
                        refresh_local_llm_status();
                    }
                    Err(err) => {
                        llm_prepare_notice.set(Some(err));
                    }
                }
            });
        }
    });

    let send_cgm_chat = Callback::new(move |question: String| {
        let trimmed = question.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        chat_error.set(None);

        let current_monitor = detected_monitor.get_untracked();
        let current_diag = import_diagnostics.get_untracked();
        let current_checkins = checkins.get_untracked();
        let summary = glucose_summary.get_untracked();
        let analytics = glucose_analytics.get_untracked();
        let facts = collect_cgm_grounding_facts_with_analytics(
            current_monitor,
            summary.as_ref(),
            current_diag.as_ref(),
            Some(&analytics),
            &current_checkins,
        );
        let fallback_answer = synthesize_grounded_cgm_answer_with_analytics(
            &trimmed,
            current_monitor,
            summary.as_ref(),
            current_diag.as_ref(),
            Some(&analytics),
            &current_checkins,
        );

        let turn_id = format!("cgm-chat-{}", next_chat_seq.get_untracked());
        next_chat_seq.update(|n| *n += 1);

        let try_aicore = use_local_llm.get_untracked()
            && llm_status
                .get_untracked()
                .is_some_and(|s| s.enabled && s.availability == "available");

        #[cfg(any(feature = "hydrate", feature = "csr"))]
        if try_aicore {
            let recent = chat_turns.get_untracked();
            let prompt = build_personal_cgm_chat_prompt_with_analytics(
                &trimmed,
                current_monitor,
                summary.as_ref(),
                current_diag.as_ref(),
                Some(&analytics),
                &current_checkins,
                &recent,
            );
            chat_busy.set(true);
            generate_local_llm(&prompt, Some(420), move |res| {
                chat_busy.set(false);
                let turn = match res {
                    Ok(gen) => LocalChatTurn {
                        id: turn_id,
                        question: trimmed,
                        answer: gen.text,
                        engine_label: format!("{} · On-Device", gen.model_name),
                        latency_ms: Some(gen.latency_ms),
                        grounding_facts: facts,
                    },
                    Err(err) => {
                        chat_error.set(Some(format!(
                            "Gemini Nano unavailable for this turn ({err}); answered via grounded local Rust."
                        )));
                        LocalChatTurn {
                            id: turn_id,
                            question: trimmed,
                            answer: fallback_answer,
                            engine_label: "Grounded Rust (AICore fallback)".to_string(),
                            latency_ms: None,
                            grounding_facts: facts,
                        }
                    }
                };
                chat_turns.update(|list| {
                    list.push(turn);
                    if list.len() > 24 {
                        let excess = list.len() - 24;
                        list.drain(0..excess);
                    }
                });
                if let Ok(encoded) = serde_json::to_string(&chat_turns.get_untracked()) {
                    persist_vault_update(
                        PersonalVaultSaveUi {
                            chat_history_json: Some(encoded),
                            ..Default::default()
                        },
                        "Saved encrypted chat turn to local vault.",
                    );
                }
            });
            return;
        }

        let _ = try_aicore;
        let turn = LocalChatTurn {
            id: turn_id,
            question: trimmed,
            answer: fallback_answer,
            engine_label: "Grounded Rust · On-Device".to_string(),
            latency_ms: None,
            grounding_facts: facts,
        };
        chat_turns.update(|list| {
            list.push(turn);
            if list.len() > 24 {
                let excess = list.len() - 24;
                list.drain(0..excess);
            }
        });
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        if let Ok(encoded) = serde_json::to_string(&chat_turns.get_untracked()) {
            persist_vault_update(
                PersonalVaultSaveUi {
                    chat_history_json: Some(encoded),
                    ..Default::default()
                },
                "Saved encrypted chat turn to local vault.",
            );
        }
    });

    let clear_cgm_chat = Callback::new(move |()| {
        chat_turns.set(Vec::new());
        chat_error.set(None);
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        persist_vault_update(
            PersonalVaultSaveUi {
                chat_history_json: Some("[]".to_string()),
                ..Default::default()
            },
            "Cleared chat thread in encrypted local vault.",
        );
    });

    let request_health_connect_permission = Callback::new(move |()| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            let platform_name = hc_status
                .get()
                .map(|s| s.platform_kind().display_name())
                .unwrap_or("Health Connect");
            hc_busy.set(true);
            hc_error.set(None);
            hc_message.set(None);
            invoke_tauri_command::<HealthBridgePermissionUi, _>(
                "health_connect_request_permission",
                None,
                move |res| {
                    hc_busy.set(false);
                    match res {
                        Ok(outcome) => {
                            if outcome.granted {
                                hc_message.set(Some(format!(
                                    "{platform_name} blood glucose access granted ({} permission{} active).",
                                    outcome.granted_permissions.len(),
                                    if outcome.granted_permissions.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    }
                                )));
                            } else {
                                hc_error.set(Some(format!(
                                    "Blood glucose read access was not granted in {platform_name}."
                                )));
                            }
                            refresh_health_connect_status();
                        }
                        Err(err) => {
                            hc_error.set(Some(err));
                        }
                    }
                },
            );
        }
    });

    let sync_health_connect_days = Callback::new(move |days: u32| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            use js_sys::{Object, Reflect};
            use wasm_bindgen::JsValue;

            let platform_kind = hc_status
                .get()
                .map(|s| s.platform_kind())
                .unwrap_or(crate::health_bridge::HealthBridgePlatform::AndroidHealthConnect);
            let platform_name = platform_kind.display_name();

            hc_busy.set(true);
            hc_error.set(None);
            hc_message.set(None);

            let args = Object::new();
            let _ = Reflect::set(
                &args,
                &JsValue::from_str("days"),
                &JsValue::from_f64(f64::from(days)),
            );

            invoke_tauri_command::<HealthBridgeReadUi, _>(
                "health_connect_read_glucose",
                Some(args),
                move |res| {
                    hc_busy.set(false);
                    match res {
                        Ok(resp) => {
                            if resp.record_count == 0 {
                                hc_message.set(Some(platform_kind.empty_sync_hint(days)));
                                return;
                            }
                            let sources_label = if resp.sources.is_empty() {
                                platform_name.to_string()
                            } else {
                                resp.sources.join(", ")
                            };
                            csv_input.set(compact_textarea_preview(&resp.bridge_json));
                            let banner_label = format!(
                                "{platform_name} ({} readings · {} · last {days}d · on-device)",
                                resp.record_count, sources_label
                            );
                            apply_glucose_text(&resp.bridge_json, None, Some(banner_label.clone()));
                            persist_vault_update(
                                PersonalVaultSaveUi {
                                    cgm_bridge_json: Some(resp.bridge_json.clone()),
                                    cgm_source_label: Some(banner_label),
                                    cgm_reading_count: Some(resp.record_count),
                                    ..Default::default()
                                },
                                "Encrypted & cached synced CGM snapshot in local Android Keystore vault.",
                            );
                            hc_message.set(Some(format!(
                                "Synced {} blood glucose readings locally from {sources_label} (last {days} days).",
                                resp.record_count
                            )));
                        }
                        Err(err) => {
                            hc_error.set(Some(err));
                        }
                    }
                },
            );
        }
        #[cfg(not(any(feature = "hydrate", feature = "csr")))]
        let _ = days;
    });

    let submit_checkin = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let max_hour_bound = max_checkin_hour.get_untracked();
        let sleep_hours = parsed_sleep_hours.get_untracked();
        let hour_offset = hour_input
            .get()
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=max_hour_bound).contains(v))
            .unwrap_or_else(|| 12.0_f64.min(max_hour_bound));
        let phq9 = phq9_input
            .get()
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|v| *v <= 27);
        let gad7 = gad7_input
            .get()
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|v| *v <= 21);
        let note_text = note_input.get().trim().to_string();
        let note = if note_text.is_empty() {
            "Logged check-in".to_string()
        } else {
            note_text
        };

        let hour_whole = hour_offset.floor() as u32;
        let minute = ((hour_offset - hour_offset.floor()) * 60.0).round() as u32;
        let recorded_at = format!("+{hour_whole:02}h {minute:02}m");

        let entry = MentalHealthCheckIn {
            recorded_at,
            hour_offset,
            mood: mood_input.get().clamp(1, 10),
            anxiety: anxiety_input.get().clamp(1, 10),
            energy: energy_input.get().clamp(1, 10),
            sleep_hours,
            phq9,
            gad7,
            note,
        };
        user_added_checkins.set(true);
        checkins.update(|list| {
            list.push(entry);
            list.sort_by(|a, b| a.hour_offset.total_cmp(&b.hour_offset));
        });
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        if let Ok(encoded) = serde_json::to_string(&checkins.get_untracked()) {
            persist_vault_update(
                PersonalVaultSaveUi {
                    checkins_json: Some(encoded),
                    ..Default::default()
                },
                "Saved encrypted mood check-in to local vault.",
            );
        }
        note_input.set(String::new());
        form_status.set(Some(
            "Check-in added and correlated against the Lingo CGM timeline.".to_string(),
        ));
    };

    view! {
        <Title text="Dabney.moe — Mental Health & Comprehensive Commercial CGM Tracking | dabney.moe" />
        <Meta
            name="description"
            content="Local-first mental health and comprehensive glucose tracking for all major commercial CGMs & BGMs (Abbott Lingo, FreeStyle Libre, Dexcom Clarity/Stelo, Medtronic CareLink, Eversense, Tandem, Omnipod/Glooko/Tidepool) with ADA 5-band TIR, AGP, GMI, GRI, and WebGL2 3D charts."
        />
        <Link rel="canonical" href="https://dabney.moe/mental-health" />

        <NavBar />

        <div class="mx-auto max-w-5xl px-6 py-12">
            // Hero
            <div class="mb-10 flex flex-col gap-6 border-b border-white/10 pb-8 md:flex-row md:items-end md:justify-between">
                <div class="max-w-2xl">
                    <p class="mb-2 text-xs font-semibold uppercase tracking-widest text-cyan-400">
                        "Dabney.moe · Metabolic Psychiatry & All Commercial CGMs"
                    </p>
                    <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                        "Mental Health & Comprehensive Glucose Tracking"
                    </h1>
                    <p class="mt-3 text-base leading-relaxed text-slate-300">
                        "Auto-detect and analyze exports from Abbott Lingo, FreeStyle Libre (LibreView), Dexcom G6/G7/Stelo (Clarity), Medtronic CareLink, Senseonics Eversense, Tandem, Omnipod/Glooko/Tidepool, and fingerstick BGMs in mg/dL or mmol/L—paired with mood, anxiety, PHQ-9, GAD-7, and ASRM locally in pure Rust."
                    </p>
                </div>
                <div class="flex flex-wrap gap-3">
                    <A
                        href="/clinical/patient"
                        attr:class="inline-flex items-center gap-2 rounded-full bg-emerald-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                    >
                        "Open Patient Demo →"
                    </A>
                    <A
                        href="/clinical/physician"
                        attr:class="inline-flex items-center gap-2 rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                    >
                        "Open Physician Demo →"
                    </A>
                    <button
                        type="button"
                        on:click=reset_sample
                        class="rounded-full border border-white/15 px-4 py-2.5 text-sm font-medium text-slate-200 transition hover:bg-white/5"
                    >
                        "Reset sample"
                    </button>
                </div>
            </div>

            <div class="mb-6">
                <ClinicalSuiteSwitcher />
            </div>

            // Opt-in personal mobile health bridge card (Android Health Connect / iOS Apple HealthKit).
            // Shown above the fold ONLY when running inside `moe.dabney.personal` (`enabled == true`);
            // renders nothing on web browsers and the public zero-PHI `moe.dabney.demo` app.
            {move || {
                hc_status.get().filter(|s| s.enabled).map(|status| {
                    view! {
                        <HealthConnectSyncCard
                            status=status
                            busy=hc_busy
                            message=hc_message
                            error=hc_error
                            on_request_permission=request_health_connect_permission
                            on_sync_days=sync_health_connect_days
                        />
                    }
                })
            }}

            // Hardware-backed encrypted local personal vault status & cryptographic wipe
            // (`moe.dabney.personal` only; absent on browsers and `moe.dabney.demo`).
            {move || {
                vault_status.get().filter(|v| v.enabled).map(|status| {
                    view! {
                        <PersonalVaultBar
                            status=status
                            busy=vault_busy
                            notice=vault_notice
                            on_wipe=wipe_vault_now
                        />
                    }
                })
            }}

            // Top-of-page calm Cool-Down banner when activation signals stack up
            <TopCoolDownBanner
                events=synthesized_events.into()
                asrm_score=asrm_score.into()
                sleep_hours=parsed_sleep_hours.into()
            />

            // Local personal file indicator banner (shown only when the user imports a local file)
            {move || {
                loaded_file_label.get().map(|label| {
                    view! {
                        <div class="mb-6 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-emerald-400/30 bg-emerald-500/10 px-4 py-3 text-xs text-emerald-100">
                            <div class="flex items-center gap-2">
                                <span class="inline-block h-2 w-2 rounded-full bg-emerald-400"></span>
                                <span>
                                    <strong>"Local personal trace loaded: "</strong>
                                    {label}
                                    " — never uploaded or stored on any server."
                                </span>
                            </div>
                            <button
                                type="button"
                                on:click=reset_sample
                                class="rounded-full border border-emerald-300/40 bg-slate-950/60 px-3 py-1 font-semibold text-emerald-200 transition hover:bg-emerald-500/20"
                            >
                                "Wipe & restore synthetic sample"
                            </button>
                        </div>
                    }
                })
            }}

            // KPI summary strip (ADA/ATTD clinical metrics across all commercial monitors)
            {move || {
                let summary = glucose_summary.get();
                let analytics = glucose_analytics.get();
                let current_checkins = checkins.get();
                let latest_phq9 = current_checkins.iter().rev().find_map(|c| c.phq9);
                let latest_gad7 = current_checkins.iter().rev().find_map(|c| c.gad7);
                let ep = &analytics.episode_counts;

                let mean_value = summary
                    .as_ref()
                    .map(|s| {
                        let prefix = if s.ceiling_capped_readings > 0 { "≥" } else { "" };
                        format!(
                            "{prefix}{:.0} mg/dL · {prefix}{:.1}%",
                            s.mean_mg_dl, s.gmi_percent
                        )
                    })
                    .unwrap_or_else(|| "—".to_string());
                let mean_detail = summary
                    .as_ref()
                    .map(|s| {
                        if s.ceiling_capped_readings > 0 {
                            format!(
                                "Median {:.0} · (≥ clipped · {:.0}% at cap)",
                                s.median_mg_dl, s.ceiling_capped_pct
                            )
                        } else {
                            format!(
                                "Median {:.0} · Range {:.0}–{:.0}",
                                s.median_mg_dl, s.min_mg_dl, s.max_mg_dl
                            )
                        }
                    })
                    .unwrap_or_default();

                let cv_detail = summary
                    .as_ref()
                    .map(|s| {
                        if s.ceiling_capped_readings > 0 {
                            format!("SD {:.1} · clipped by cap (<36% target)", s.std_dev_mg_dl)
                        } else {
                            format!("SD {:.1} mg/dL (target <36%)", s.std_dev_mg_dl)
                        }
                    })
                    .unwrap_or_default();

                let gri_value = summary
                    .as_ref()
                    .map(|s| {
                        let prefix = if s.ceiling_capped_readings > 0 { "≥" } else { "" };
                        format!("{prefix}{:.0} / 100", s.gri_score)
                    })
                    .unwrap_or_else(|| "—".to_string());
                let gri_detail = summary
                    .as_ref()
                    .map(|s| {
                        format!(
                            "{} · {} hypo · {} >140 exc.",
                            s.gri_zone.label(),
                            ep.level1_hypo_count,
                            ep.lingo_excursion_gt_140_count
                        )
                    })
                    .unwrap_or_default();

                view! {
                    <div class="mb-8 grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-5">
                        <MetricCard
                            label="Mean / GMI (est. A1C)"
                            value=mean_value
                            detail=mean_detail
                            accent="text-cyan-300"
                        />
                        <MetricCard
                            label="TIR 70–180 / Tight"
                            value=summary.as_ref().map(|s| format!("{:.0}% / {:.0}%", s.tir_70_180_pct, s.titr_70_140_pct)).unwrap_or_else(|| "—".to_string())
                            detail=summary.as_ref().map(|s| format!("Wear {:.0}% · {} readings", s.estimated_active_wear_pct, s.reading_count)).unwrap_or_default()
                            accent="text-emerald-300"
                        />
                        <MetricCard
                            label="CV% / MAGE"
                            value=summary.as_ref().map(|s| format!("{:.1}% · {:.0}", s.cv_percent, s.mage_mg_dl)).unwrap_or_else(|| "—".to_string())
                            detail=cv_detail
                            accent="text-amber-300"
                        />
                        <MetricCard
                            label="GRI (Risk Index)"
                            value=gri_value
                            detail=gri_detail
                            accent="text-rose-300"
                        />
                        <MetricCard
                            label="PHQ-9 / GAD-7"
                            value=format!(
                                "{} / {}",
                                latest_phq9.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string()),
                                latest_gad7.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string()),
                            )
                            detail=format!(
                                "{} · {}",
                                latest_phq9.map(|v| classify_phq9(v).label()).unwrap_or("Unscored"),
                                latest_gad7.map(|v| classify_gad7(v).label()).unwrap_or("Unscored"),
                            )
                            accent="text-violet-300"
                        />
                    </div>
                }
            }}

            // Window-specific sensor floor/ceiling saturation & Out-of-Range caveat banner
            {move || {
                glucose_summary.get().map(|summary| view! {
                    <SensorSaturationBanner summary=summary diagnostics=import_diagnostics.get() />
                })
            }}

            // On-device AI chat grounded in loaded CGM metrics & mood check-ins
            // (Android AICore / Gemini Nano on supported personal devices, grounded Rust fallback elsewhere)
            <PersonalCgmAiChatSection
                llm_status=llm_status
                use_local_llm=use_local_llm
                llm_preparing=llm_preparing
                llm_prepare_notice=llm_prepare_notice
                chat_input=chat_input
                chat_turns=chat_turns
                chat_busy=chat_busy
                chat_error=chat_error
                on_prepare_llm=prepare_local_llm_now
                on_send_question=send_cgm_chat
                on_clear_chat=clear_cgm_chat
            />

            // Interactive 3D WebGL2 / 2D SVG dual-axis chart
            <section class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold text-white">
                            {move || format!("{} — Glucose & Mental State Timeline", detected_monitor.get().display_name())}
                        </h2>
                        <p class="text-sm text-slate-400">
                            "Explore the 70–140 mg/dL tight zone with mood (emerald) and anxiety (rose) in pure-Rust WebGL2 3D or classic 2D SVG."
                        </p>
                    </div>
                    <div class="flex flex-wrap items-center gap-3 text-xs text-slate-300">
                        <div class="inline-flex rounded-full border border-white/15 bg-slate-900/80 p-0.5" role="group" aria-label="Time window">
                            {GlucoseWindow::ALL.into_iter().map(|w| {
                                view! {
                                    <button
                                        type="button"
                                        on:click=move |_| window.set(w)
                                        class=move || {
                                            if window.get() == w {
                                                "rounded-full bg-emerald-400 px-2.5 py-1 font-semibold text-slate-950 transition"
                                            } else {
                                                "rounded-full px-2.5 py-1 font-medium text-slate-300 transition hover:text-white"
                                            }
                                        }
                                    >
                                        {w.label()}
                                    </button>
                                }
                            }).collect_view()}
                        </div>
                        <div class="inline-flex rounded-full border border-white/15 bg-slate-900/80 p-0.5">
                            <button
                                type="button"
                                on:click=move |_| chart_mode.set(ChartViewMode::WebGl3D)
                                class=move || {
                                    if chart_mode.get() == ChartViewMode::WebGl3D {
                                        "rounded-full bg-cyan-500 px-3 py-1 font-semibold text-slate-950 transition"
                                    } else {
                                        "rounded-full px-3 py-1 font-medium text-slate-300 transition hover:text-white"
                                    }
                                }
                            >
                                "3D WebGL2"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| chart_mode.set(ChartViewMode::Svg2D)
                                class=move || {
                                    if chart_mode.get() == ChartViewMode::Svg2D {
                                        "rounded-full bg-cyan-500 px-3 py-1 font-semibold text-slate-950 transition"
                                    } else {
                                        "rounded-full px-3 py-1 font-medium text-slate-300 transition hover:text-white"
                                    }
                                }
                            >
                                "2D SVG"
                            </button>
                        </div>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-cyan-400"></span>
                            "Lingo glucose (mg/dL)"
                        </span>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-emerald-400"></span>
                            "Mood (1–10)"
                        </span>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-rose-400"></span>
                            "Anxiety (1–10)"
                        </span>
                    </div>
                </div>
                {move || match chart_mode.get() {
                    ChartViewMode::WebGl3D => view! {
                        <LingoMood3DChart readings=windowed_readings.into() checkins=checkins.into() />
                    }.into_any(),
                    ChartViewMode::Svg2D => view! {
                        <LingoMoodTimelineChart readings=windowed_readings.get() checkins=checkins.get() />
                    }.into_any(),
                }}
            </section>

            // Clinical ADA 5-Band Time-in-Range + 24-Hour Ambulatory Glucose Profile (AGP)
            {move || {
                glucose_summary.get().map(|summary| {
                    let bins = summary.agp_hourly.clone();
                    let device_badge = format!(
                        "{} · {} · {} · window {}",
                        summary.monitor.display_name(),
                        summary.method.label(),
                        summary.source_unit.label(),
                        window.get().label(),
                    );
                    view! {
                        <section class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-5">
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <div>
                                    <h2 class="text-lg font-semibold text-white">
                                        "ADA 5-Band Time-in-Range & 24-Hour Ambulatory Glucose Profile (AGP)"
                                    </h2>
                                    <p class="text-sm text-slate-400">
                                        "International consensus glucose distribution (TBR <54 / 54–69, TIR 70–180, TITR 70–140, TAR 181–250 / >250) and 5th–95th / 25th–75th / median hourly AGP curve."
                                    </p>
                                </div>
                                <span class="inline-flex rounded-full border border-cyan-400/30 bg-cyan-500/10 px-3 py-1 text-xs font-medium text-cyan-300">
                                    {device_badge}
                                </span>
                            </div>
                            <AdaTimeInRangeBar summary=summary />
                            <div class="rounded-xl border border-white/10 bg-slate-950/60 p-4">
                                <div class="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs text-slate-400">
                                    <span class="font-medium text-slate-200">"24-Hour AGP Percentile Ribbon (p5–p95 outer · p25–p75 IQR · p50 median)"</span>
                                    <span>"Local clock hour (00:00–23:00)"</span>
                                </div>
                                <AgpPercentileChart bins=bins />
                            </div>
                        </section>
                    }
                })
            }}

            // Longitudinal CGM Analytics: 14d Period Comparison, Circadian Breakdown,
            // ATTD Sustained Episodes & Velocity, and Daily / Wear Sessions Breakdown.
            {move || {
                glucose_analytics.get().period_comparison.map(|comparison| view! {
                    <PeriodComparisonCard comparison=comparison />
                })
            }}
            {move || {
                let analytics = glucose_analytics.get();
                view! {
                    <CircadianBreakdownSection periods=analytics.circadian />
                    <GlycemicEpisodesAndRocSection
                        episodes=analytics.episodes
                        counts=analytics.episode_counts
                        roc=analytics.rate_of_change
                    />
                    <DailyGlucoseAndSessionsSection
                        days=analytics.daily
                        sessions=analytics.sessions
                        gaps=analytics.gaps
                    />
                }
            }}

            // Longitudinal PHQ-9 & GAD-7 Trend Chart
            <section class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold text-white">
                            "PHQ-9 & GAD-7 Longitudinal Trajectory"
                        </h2>
                        <p class="text-sm text-slate-400">
                            "Multi-week depression (PHQ-9, 0–27) and anxiety (GAD-7, 0–21) scores with Mild (5) and Moderate (10) clinical thresholds."
                        </p>
                    </div>
                    <div class="flex flex-wrap gap-4 text-xs text-slate-300">
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-violet-400"></span>
                            "PHQ-9 (0–27)"
                        </span>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-rose-400"></span>
                            "GAD-7 (0–21)"
                        </span>
                    </div>
                </div>
                {move || {
                    let series = build_assessment_series(&sample_assessment_history(), &checkins.get());
                    view! { <Phq9Gad7TrendChart series=series /> }
                }}
            </section>

            // Two-column workspace: Check-in Logger + Abbott Lingo CSV Import
            <div class="mb-8 grid gap-6 lg:grid-cols-2">
                <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                    <h2 class="mb-1 text-lg font-semibold text-white">"Log Mental Health Check-In"</h2>
                    <p class="mb-4 text-sm text-slate-400">
                        "Correlates immediately with your nearest Lingo glucose reading and prior 2-hour excursion peak."
                    </p>
                    <form on:submit=submit_checkin class="space-y-4">
                        <div class="grid grid-cols-3 gap-3">
                            <label class="block text-xs text-slate-300">
                                {move || format!("Mood: {}/10", mood_input.get())}
                                <input
                                    type="range"
                                    min="1"
                                    max="10"
                                    prop:value=move || mood_input.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                            mood_input.set(v);
                                        }
                                    }
                                    class="mt-1 w-full accent-emerald-400"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                {move || format!("Anxiety: {}/10", anxiety_input.get())}
                                <input
                                    type="range"
                                    min="1"
                                    max="10"
                                    prop:value=move || anxiety_input.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                            anxiety_input.set(v);
                                        }
                                    }
                                    class="mt-1 w-full accent-rose-400"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                {move || format!("Energy: {}/10", energy_input.get())}
                                <input
                                    type="range"
                                    min="1"
                                    max="10"
                                    prop:value=move || energy_input.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                            energy_input.set(v);
                                        }
                                    }
                                    class="mt-1 w-full accent-cyan-400"
                                />
                            </label>
                        </div>
                        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
                            <label class="block text-xs text-slate-300">
                                "Hour offset"
                                <input
                                    type="number"
                                    step="0.5"
                                    min="0"
                                    max=move || format!("{:.1}", max_checkin_hour.get())
                                    prop:value=move || hour_input.get()
                                    on:input=move |ev| hour_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "Sleep (hrs)"
                                <input
                                    type="number"
                                    step="0.5"
                                    min="0"
                                    max="24"
                                    prop:value=move || sleep_input.get()
                                    on:input=move |ev| sleep_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "PHQ-9 (0–27)"
                                <input
                                    type="number"
                                    min="0"
                                    max="27"
                                    placeholder="opt"
                                    prop:value=move || phq9_input.get()
                                    on:input=move |ev| phq9_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "GAD-7 (0–21)"
                                <input
                                    type="number"
                                    min="0"
                                    max="21"
                                    placeholder="opt"
                                    prop:value=move || gad7_input.get()
                                    on:input=move |ev| gad7_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                        </div>
                        <label class="block text-xs text-slate-300">
                            "Context note (meal, stressor, walk, medication)"
                            <input
                                type="text"
                                placeholder="e.g. Afternoon focus dip 60m after sweet snack"
                                prop:value=move || note_input.get()
                                on:input=move |ev| note_input.set(event_target_value(&ev))
                                class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 text-sm text-white placeholder:text-slate-500"
                            />
                        </label>
                        <div class="flex items-center justify-between gap-3">
                            <button
                                type="submit"
                                class="rounded-full bg-emerald-500 px-4 py-2 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                            >
                                "Add check-in"
                            </button>
                            {move || form_status.get().map(|msg| view! {
                                <span class="text-xs text-emerald-300">{msg}</span>
                            })}
                        </div>
                    </form>
                </section>

                <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                    <div class="mb-1 flex flex-wrap items-center justify-between gap-2">
                        <h2 class="text-lg font-semibold text-white">"Abbott Lingo & Commercial CGM Import"</h2>
                        <span class="rounded-full border border-white/10 bg-slate-900 px-2.5 py-0.5 text-[11px] text-cyan-300">
                            {move || format!("{} ({})", detected_monitor.get().display_name(), source_unit.get().label())}
                        </span>
                    </div>
                    <p class="mb-3 text-sm text-slate-400">
                        "Load personal exports locally (never uploaded): Abbott Lingo native CSV, Apple Health export.xml, Health Connect JSON (`dabney.health_bridge.v1`), FreeStyle LibreView, Dexcom Clarity/Stelo, Medtronic CareLink, Eversense, Tandem, Glooko/Tidepool, or BGM CSV."
                    </p>
                    <details class="mb-3 rounded-xl border border-cyan-400/20 bg-cyan-500/5 px-3.5 py-2.5 text-xs text-slate-300">
                        <summary class="cursor-pointer font-semibold text-cyan-200">
                            "How to export from Abbott Lingo today (+ Android Health Connect / Apple HealthKit)"
                        </summary>
                        <ol class="mt-2 list-decimal space-y-1 pl-4 text-[11px] leading-relaxed text-slate-300">
                            <li>"Open the Abbott Lingo app → You → Profile → Actions → Export your glucose data → Download file (5-min ISO-8601 CSV)."</li>
                            <li>"Select that `.csv` below (or an iOS Apple Health `export.xml` or Health Connect JSON) — everything parses locally in Rust/WASM and stays in device RAM."</li>
                            <li>"Direct Google Health Connect live sync lives strictly inside the separate opt-in personal Android build (`moe.dabney.personal`) and feeds this exact `dabney.health_bridge.v1` JSON schema, keeping the public Play Store demo (`moe.dabney.demo`) strictly zero-PHI."</li>
                        </ol>
                    </details>
                    <div class="mb-3 flex flex-wrap gap-1.5">
                        <button
                            type="button"
                            on:click=move |_| load_monitor_preset(GlucoseMonitor::AbbottLingo)
                            class="rounded-full border border-white/15 px-2.5 py-1 text-[11px] text-slate-200 transition hover:bg-white/10"
                        >
                            "Abbott Lingo"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| load_raw_preset(SAMPLE_HEALTH_BRIDGE_JSON)
                            class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2.5 py-1 text-[11px] text-emerald-200 transition hover:bg-emerald-500/20"
                        >
                            "Health Connect JSON"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| load_raw_preset(SAMPLE_APPLE_HEALTH_XML)
                            class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-2.5 py-1 text-[11px] text-cyan-200 transition hover:bg-cyan-500/20"
                        >
                            "Apple Health XML"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| load_monitor_preset(GlucoseMonitor::DexcomClarity)
                            class="rounded-full border border-white/15 px-2.5 py-1 text-[11px] text-slate-200 transition hover:bg-white/10"
                        >
                            "Dexcom G7 Clarity"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| load_monitor_preset(GlucoseMonitor::AbbottFreeStyleLibre)
                            class="rounded-full border border-white/15 px-2.5 py-1 text-[11px] text-slate-200 transition hover:bg-white/10"
                        >
                            "Libre 3 (mmol/L)"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| load_monitor_preset(GlucoseMonitor::MedtronicCareLink)
                            class="rounded-full border border-white/15 px-2.5 py-1 text-[11px] text-slate-200 transition hover:bg-white/10"
                        >
                            "Medtronic CareLink"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| load_monitor_preset(GlucoseMonitor::FingerstickBgm)
                            class="rounded-full border border-white/15 px-2.5 py-1 text-[11px] text-slate-200 transition hover:bg-white/10"
                        >
                            "Fingerstick BGM"
                        </button>
                    </div>
                    <div class="mb-3 grid gap-2 sm:grid-cols-2">
                        <label class="block text-[11px] text-slate-300">
                            "Monitor format"
                            <select
                                on:change=move |ev| {
                                    let val = event_target_value(&ev);
                                    monitor_hint.set(GlucoseMonitor::from_slug(&val));
                                }
                                class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white"
                            >
                                <option value="auto">"Auto-detect (all 10 monitors)"</option>
                                {GlucoseMonitor::ALL.into_iter().map(|m| view! {
                                    <option value=m.slug()>{m.display_name()}</option>
                                }).collect_view()}
                            </select>
                        </label>
                        <label class="block text-[11px] text-slate-300">
                            "Select file (.csv, .xml, .json — stays on device)"
                            <input
                                type="file"
                                accept=".csv,.txt,.json,.xml,text/csv,application/json,application/xml,text/xml"
                                on:change=on_cgm_file_change
                                class="mt-1 block w-full text-xs text-slate-300 file:mr-2 file:rounded-full file:border-0 file:bg-cyan-500 file:px-3 file:py-1 file:text-xs file:font-semibold file:text-slate-950 hover:file:bg-cyan-400"
                            />
                        </label>
                    </div>
                    <textarea
                        rows="5"
                        prop:value=move || csv_input.get()
                        on:input=move |ev| csv_input.set(event_target_value(&ev))
                        class="w-full rounded-xl border border-white/10 bg-slate-950 p-3 font-mono text-xs text-slate-200"
                    />
                    <div class="mt-3 flex flex-wrap items-center justify-between gap-3">
                        <button
                            type="button"
                            on:click=apply_csv
                            class="rounded-full bg-cyan-500 px-4 py-2 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                        >
                            "Parse & Validate Export"
                        </button>
                        {move || csv_error.get().map(|err| view! {
                            <span class="text-xs text-rose-400">{err}</span>
                        })}
                    </div>
                    {move || import_diagnostics.get().map(|diag| view! {
                        <ImportValidationPanel diagnostics=diag />
                    })}
                </section>
            </div>

            // Correlated check-ins table
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="mb-1 text-lg font-semibold text-white">"Glucose–Mood Correlation Log"</h2>
                        <p class="text-sm text-slate-400">
                            "Each mental health check-in paired with nearest Abbott Lingo glucose and peak reading in the preceding 2 hours."
                        </p>
                    </div>
                    {move || {
                        let corr = correlations.get();
                        let href = csv_data_uri(&export_correlations_csv(&corr));
                        view! {
                            <a
                                href=href
                                download="dabney-mental-health-cgm.csv"
                                class="inline-flex items-center gap-1.5 rounded-full border border-white/15 px-4 py-2 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                            >
                                "Download CSV"
                            </a>
                        }
                    }}
                </div>
                <div class="overflow-x-auto">
                    <table class="w-full min-w-[600px] border-collapse text-left text-sm">
                        <thead>
                            <tr class="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                                <th class="py-2 pr-4">"Time"</th>
                                <th class="py-2 pr-4">"Glucose / 2h Peak"</th>
                                <th class="py-2 pr-4">"Mood · Anx · Eng"</th>
                                <th class="py-2 pr-4">"Pattern"</th>
                                <th class="py-2">"Context"</th>
                            </tr>
                        </thead>
                        <tbody class="divide-y divide-white/5 text-slate-200">
                            {move || {
                                let corr = correlations.get();
                                if corr.is_empty() {
                                    return view! {
                                        <tr>
                                            <td colspan="5" class="py-4 text-xs text-slate-400">
                                                "No mental health check-ins logged for this trace yet. Use the form above to correlate mood, anxiety, or sleep against this CGM window."
                                            </td>
                                        </tr>
                                    }.into_any();
                                }
                                corr.into_iter().map(|row| {
                                    let badge = if row.spike_adjacent {
                                        view! {
                                            <span class="inline-flex rounded-full bg-amber-500/15 px-2.5 py-0.5 text-xs font-medium text-amber-300">
                                                "Post-spike (>140)"
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <span class="inline-flex rounded-full bg-emerald-500/15 px-2.5 py-0.5 text-xs font-medium text-emerald-300">
                                                "In Lingo zone"
                                            </span>
                                        }.into_any()
                                    };
                                    view! {
                                        <tr>
                                            <td class="py-3 pr-4 whitespace-nowrap font-mono text-xs text-slate-300">{row.recorded_at}</td>
                                            <td class="py-3 pr-4 whitespace-nowrap font-mono text-xs text-cyan-300">
                                                {format!("{:.0} / {:.0} mg/dL", row.nearest_glucose_mg_dl, row.peak_prior_2h_mg_dl)}
                                            </td>
                                            <td class="py-3 pr-4 whitespace-nowrap text-xs">
                                                {format!("{}/10 · {}/10 · {}/10", row.mood, row.anxiety, row.energy)}
                                            </td>
                                            <td class="py-3 pr-4 whitespace-nowrap">{badge}</td>
                                            <td class="py-3 text-xs text-slate-300">{row.note}</td>
                                        </tr>
                                    }
                                }).collect_view().into_any()
                            }}
                        </tbody>
                    </table>
                </div>
            </section>

            // Local-first Opt-In Multi-Platform Synthesizer, Actogram & Cool-Down Plan
            <EpisodeActivationSection
                events=activity_events
                consent=platform_consent
                asrm_score=asrm_score
                sleep_hours=parsed_sleep_hours.into()
            />

            // On-Device Personal Crisis & Support Card + 1-Page Printable Clinician Visit Brief
            <SupportAndClinicianBriefSection
                glucose_summary=glucose_summary.into()
                analytics=Signal::derive(move || Some(glucose_analytics.get()))
                checkins=checkins.into()
                cadence_summary=Signal::derive(move || {
                    summarize_episode_cadence(
                        &synthesized_events.get(),
                        asrm_score.get(),
                        parsed_sleep_hours.get(),
                    )
                })
                asrm_score=asrm_score.into()
                sleep_hours=parsed_sleep_hours.into()
            />
        </div>

        <Footer />
    }
}

#[component]
fn MetricCard(
    label: &'static str,
    value: String,
    detail: String,
    accent: &'static str,
) -> impl IntoView {
    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-4">
            <p class="text-xs font-medium text-slate-400">{label}</p>
            <p class=format!("mt-1 text-2xl font-bold tracking-tight {accent}")>{value}</p>
            <p class="mt-1 text-xs text-slate-400">{detail}</p>
        </div>
    }
}

#[component]
fn ImportValidationPanel(diagnostics: GlucoseImportDiagnostics) -> impl IntoView {
    let format_label = diagnostics.format.label();
    let span_text = if diagnostics.span_hours >= 48.0 {
        format!("{:.1}d", diagnostics.span_hours / 24.0)
    } else {
        format!("{:.1}h", diagnostics.span_hours)
    };
    let cadence_text = diagnostics
        .median_interval_minutes
        .map(|m| format!("{m:.1} min"))
        .unwrap_or_else(|| "—".to_string());
    let range_text = match (&diagnostics.first_timestamp, &diagnostics.last_timestamp) {
        (Some(first), Some(last)) => format!("{first} → {last}"),
        _ => "—".to_string(),
    };
    let sources_text = if diagnostics.detected_sources.is_empty() {
        None
    } else {
        Some(diagnostics.detected_sources.join(", "))
    };
    let warnings = diagnostics.sample_warnings;
    let checks = diagnostics.checks;

    view! {
        <div class="mt-4 rounded-xl border border-white/10 bg-slate-950/70 p-3.5 space-y-2.5 text-xs">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <span class="font-semibold text-white">"Import Validation Report"</span>
                <span class="rounded-full border border-cyan-400/25 bg-cyan-500/10 px-2 py-0.5 text-[10px] font-medium text-cyan-200">
                    {format_label}
                </span>
            </div>
            <div class="grid grid-cols-2 gap-2 text-[11px] sm:grid-cols-4">
                <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2.5 py-1.5">
                    <p class="text-slate-400">"Parsed"</p>
                    <p class="font-mono font-semibold text-white">{diagnostics.parsed_readings}</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2.5 py-1.5">
                    <p class="text-slate-400">"Span"</p>
                    <p class="font-mono font-semibold text-cyan-200">{span_text}</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2.5 py-1.5">
                    <p class="text-slate-400">"Cadence"</p>
                    <p class="font-mono font-semibold text-emerald-200">{cadence_text}</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2.5 py-1.5">
                    <p class="text-slate-400">"Cap / OOR / Invalid"</p>
                    <p class="font-mono font-semibold text-amber-200">
                        {format!(
                            "{} / {} / {}",
                            diagnostics.bounded_sentinels,
                            diagnostics.out_of_range_rows,
                            diagnostics.skipped_invalid_rows
                        )}
                    </p>
                </div>
            </div>
            <p class="font-mono text-[10px] text-slate-400 break-all">{range_text}</p>
            {sources_text.map(|s| view! {
                <p class="text-[11px] text-slate-300">
                    <span class="text-slate-400">"Detected source(s): "</span>
                    <span class="font-mono text-cyan-200">{s}</span>
                </p>
            })}
            <ul class="space-y-1.5">
                {checks.into_iter().map(|chk| {
                    let (badge_cls, badge_txt) = match chk.level {
                        ValidationLevel::Pass => (
                            "bg-emerald-500/15 text-emerald-300 border-emerald-400/25",
                            "PASS",
                        ),
                        ValidationLevel::Warn => (
                            "bg-amber-500/15 text-amber-300 border-amber-400/25",
                            "WARN",
                        ),
                        ValidationLevel::Info => (
                            "bg-cyan-500/15 text-cyan-200 border-cyan-400/25",
                            "INFO",
                        ),
                    };
                    view! {
                        <li class="flex items-start gap-2 text-[11px] text-slate-300">
                            <span class=format!("mt-0.5 shrink-0 rounded border px-1.5 py-0.5 font-mono text-[9px] font-semibold {badge_cls}")>
                                {badge_txt}
                            </span>
                            <span>
                                <strong class="text-slate-100">{chk.name}": "</strong>
                                {chk.detail}
                            </span>
                        </li>
                    }
                }).collect_view()}
            </ul>
            {(!warnings.is_empty()).then(|| view! {
                <div class="rounded-lg border border-amber-400/25 bg-amber-500/10 p-2 text-[11px] text-amber-200 space-y-0.5">
                    <p class="font-semibold">"Sample row notices:"</p>
                    {warnings.into_iter().map(|w| view! { <p class="font-mono text-[10px]">{w}</p> }).collect_view()}
                </div>
            })}
        </div>
    }
}

fn select_svg_floor_cap_markers(
    readings: &[LingoReading],
    max_markers: usize,
) -> Vec<LingoReading> {
    if readings.is_empty() || max_markers == 0 {
        return Vec::new();
    }
    const GAP_BREAK_HOURS: f64 = 1.5;
    let mut reps: Vec<usize> = Vec::new();
    let mut active_start: Option<usize> = None;
    let mut prev_idx: usize = 0;

    for (idx, r) in readings.iter().enumerate() {
        if r.bound.is_floor_capped() {
            match active_start {
                None => {
                    active_start = Some(idx);
                    prev_idx = idx;
                }
                Some(start) => {
                    if (r.hour_offset - readings[prev_idx].hour_offset).abs() > GAP_BREAK_HOURS {
                        reps.push(start + (prev_idx - start) / 2);
                        active_start = Some(idx);
                    }
                    prev_idx = idx;
                }
            }
        } else if let Some(start) = active_start.take() {
            reps.push(start + (prev_idx - start) / 2);
        }
    }
    if let Some(start) = active_start {
        reps.push(start + (prev_idx - start) / 2);
    }

    if reps.len() > max_markers {
        let step = reps.len() as f64 / max_markers as f64;
        reps = (0..max_markers)
            .map(|i| reps[((i as f64 * step).floor() as usize).min(reps.len() - 1)])
            .collect();
    }

    reps.into_iter().map(|i| readings[i].clone()).collect()
}

fn timeline_x_axis_labels(readings: &[LingoReading], max_hour: f64) -> (String, String, String) {
    if max_hour >= 24.0 {
        let first_date = readings
            .first()
            .and_then(|r| parse_timestamp(&r.timestamp, None))
            .map(|p| p.local_date);
        let last_date = readings
            .last()
            .and_then(|r| parse_timestamp(&r.timestamp, None))
            .map(|p| p.local_date);
        if let (Some(d0), Some(d1)) = (first_date, last_date) {
            let span_days = (d1 - d0).num_days();
            if span_days >= 2 {
                let d_mid = d0 + chrono::TimeDelta::days(span_days / 2);
                return (d0.to_string(), d_mid.to_string(), d1.to_string());
            }
        }
    }
    let mid = if max_hour >= 72.0 {
        format!("{:.1}d", (max_hour / 2.0) / 24.0)
    } else {
        format!("{:.0}h", max_hour / 2.0)
    };
    let end = if max_hour >= 72.0 {
        format!("{:.1}d", max_hour / 24.0)
    } else {
        format!("{:.0}h", max_hour)
    };
    ("0h".to_string(), mid, end)
}

#[component]
fn LingoMoodTimelineChart(
    readings: Vec<LingoReading>,
    checkins: Vec<MentalHealthCheckIn>,
) -> impl IntoView {
    let width = 760.0_f64;
    let height = 240.0_f64;
    let pad_l = 48.0_f64;
    let pad_r = 24.0_f64;
    let pad_t = 18.0_f64;
    let pad_b = 30.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;

    let max_hour = readings
        .iter()
        .map(|r| r.hour_offset)
        .chain(checkins.iter().map(|c| c.hour_offset))
        .fold(24.0_f64, f64::max)
        .max(1.0);
    // Lower floor to 45 mg/dL so Lingo floor-capped 54 mg/dL (<55) readings sit visibly above the bottom edge.
    let min_g = 45.0_f64;
    let max_g = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(190.0_f64, f64::max)
        .max(190.0);

    let x_for = |h: f64| pad_l + (h / max_hour) * plot_w;
    let y_for_g = |g: f64| {
        let clamped = g.clamp(min_g, max_g);
        pad_t + plot_h - ((clamped - min_g) / (max_g - min_g)) * plot_h
    };
    let y_for_score = |score: u8| {
        let norm = (f64::from(score.clamp(1, 10)) - 1.0) / 9.0;
        pad_t + plot_h - norm * plot_h
    };

    let band_top = y_for_g(140.0);
    let band_bottom = y_for_g(70.0);
    let band_h = (band_bottom - band_top).max(1.0);
    let y_180 = y_for_g(180.0);
    let show_max_g_label = (y_180 - pad_t).abs() > 14.0;

    let segments = downsample_segments_for_chart(&readings, 360, DEFAULT_CHART_GAP_HOURS);
    let spike_peaks = select_notable_spike_peaks(&readings, 36);
    let floor_caps = select_svg_floor_cap_markers(&readings, 24);
    let (x_left, x_mid, x_right) = timeline_x_axis_labels(&readings, max_hour);

    let segment_polylines: Vec<String> = segments
        .iter()
        .filter(|seg| seg.len() >= 2)
        .map(|seg| {
            seg.iter()
                .map(|r| {
                    format!(
                        "{:.1},{:.1}",
                        x_for(r.hour_offset),
                        y_for_g(r.glucose_mg_dl)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let isolated_points: Vec<(f64, f64)> = segments
        .iter()
        .filter(|seg| seg.len() == 1)
        .map(|seg| (x_for(seg[0].hour_offset), y_for_g(seg[0].glucose_mg_dl)))
        .collect();

    view! {
        <svg
            viewBox="0 0 760 240"
            class="w-full"
            role="img"
            aria-label="Abbott Lingo continuous glucose monitoring and mental health check-in timeline"
        >
            // Target 70-140 mg/dL shaded metabolic zone
            <rect
                x=pad_l
                y=band_top
                width=plot_w
                height=band_h
                fill="rgba(52, 211, 153, 0.10)"
            />
            // Clinical 180 mg/dL hyperglycemia reference line
            <line x1=pad_l y1=y_180 x2=width - pad_r y2=y_180 stroke="rgba(251, 191, 36, 0.45)" stroke-dasharray="4 4" />
            <line x1=pad_l y1=band_top x2=width - pad_r y2=band_top stroke="rgba(52, 211, 153, 0.35)" stroke-dasharray="4 4" />
            <line x1=pad_l y1=band_bottom x2=width - pad_r y2=band_bottom stroke="rgba(52, 211, 153, 0.35)" stroke-dasharray="4 4" />

            // Y-axis labels
            <text x="8" y=y_180 + 4.0 fill=AMBER font-size="10">"180"</text>
            <text x="8" y=band_top + 4.0 fill=SLATE_LIGHT font-size="10">"140"</text>
            <text x="8" y=band_bottom + 4.0 fill=SLATE_LIGHT font-size="10">"70"</text>
            {show_max_g_label.then(|| view! {
                <text x="8" y=pad_t + 6.0 fill=SLATE_LIGHT font-size="10">{format!("{:.0}", max_g)}</text>
            })}

            // Gap-aware glucose segments (never bridged across >2h sensor gaps)
            {segment_polylines.into_iter().map(|pts| view! {
                <polyline
                    fill="none"
                    stroke=CYAN
                    stroke-width="2.5"
                    stroke-linejoin="round"
                    stroke-linecap="round"
                    points=pts
                />
            }).collect_view()}
            {isolated_points.into_iter().map(|(cx, cy)| view! {
                <circle cx=cx cy=cy r="2.5" fill=CYAN />
            }).collect_view()}

            // Spike markers (> 140 mg/dL local peaks; ceiling-capped readings styled distinctly in orange-red)
            {spike_peaks.into_iter().map(|r| {
                let cx = x_for(r.hour_offset);
                let cy = y_for_g(r.glucose_mg_dl);
                if r.bound.is_ceiling_capped() {
                    view! {
                        <circle cx=cx cy=cy r="5.0" fill="#f97316" stroke="#fff7ed" stroke-width="1.5" />
                    }.into_any()
                } else {
                    view! {
                        <circle cx=cx cy=cy r="4.5" fill=AMBER stroke="#0f172a" stroke-width="1.5" />
                    }.into_any()
                }
            }).collect_view()}

            // Floor-capped markers (<55 / ≤54 mg/dL dips in crimson)
            {floor_caps.into_iter().map(|r| {
                let cx = x_for(r.hour_offset);
                let cy = y_for_g(r.glucose_mg_dl);
                view! {
                    <circle cx=cx cy=cy r="4.5" fill="#f43f5e" stroke="#fff1f2" stroke-width="1.5" />
                }
            }).collect_view()}

            // Mood & Anxiety markers
            {checkins.into_iter().map(|c| {
                let cx = x_for(c.hour_offset);
                let my = y_for_score(c.mood);
                let ay = y_for_score(c.anxiety);
                view! {
                    <g>
                        <line x1=cx y1=pad_t x2=cx y2=height - pad_b stroke="rgba(148,163,184,0.18)" stroke-dasharray="2 3" />
                        <circle cx=cx cy=my r="4.5" fill=EMERALD stroke="#0f172a" stroke-width="1.5" />
                        <circle cx=cx cy=ay r="4.5" fill=ROSE stroke="#0f172a" stroke-width="1.5" />
                    </g>
                }
            }).collect_view()}

            // X-axis labels (real YYYY-MM-DD calendar dates when span >= 24h)
            <text x=pad_l y=height - 8.0 fill=SLATE_LIGHT font-size="10">{x_left}</text>
            <text x=pad_l + plot_w / 2.0 y=height - 8.0 text-anchor="middle" fill=SLATE_LIGHT font-size="10">
                {x_mid}
            </text>
            <text x=width - pad_r y=height - 8.0 text-anchor="end" fill=SLATE_LIGHT font-size="10">
                {x_right}
            </text>
        </svg>
    }
}

#[component]
fn Phq9Gad7TrendChart(series: Vec<AssessmentPoint>) -> impl IntoView {
    let width = 760.0_f64;
    let height = 190.0_f64;
    let pad_l = 44.0_f64;
    let pad_r = 28.0_f64;
    let pad_t = 16.0_f64;
    let pad_b = 28.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;
    let max_score = 27.0_f64;

    let count = series.len().max(2);
    let x_for = |idx: usize| pad_l + (idx as f64 / (count - 1) as f64) * plot_w;
    let y_for = |score: u8| pad_t + plot_h - (f64::from(score).min(max_score) / max_score) * plot_h;

    let phq9_points: String = series
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.phq9.map(|v| format!("{:.1},{:.1}", x_for(i), y_for(v))))
        .collect::<Vec<_>>()
        .join(" ");
    let gad7_points: String = series
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.gad7.map(|v| format!("{:.1},{:.1}", x_for(i), y_for(v))))
        .collect::<Vec<_>>()
        .join(" ");

    let y_mild = y_for(5);
    let y_mod = y_for(10);

    view! {
        <svg
            viewBox="0 0 760 190"
            class="w-full"
            role="img"
            aria-label="PHQ-9 and GAD-7 longitudinal score trajectory"
        >
            <line x1=pad_l y1=y_mod x2=width - pad_r y2=y_mod stroke="rgba(251, 191, 36, 0.35)" stroke-dasharray="4 4" />
            <line x1=pad_l y1=y_mild x2=width - pad_r y2=y_mild stroke="rgba(52, 211, 153, 0.30)" stroke-dasharray="4 4" />

            <text x="6" y=y_mod + 4.0 fill=AMBER font-size="10">"10"</text>
            <text x="6" y=y_mild + 4.0 fill=EMERALD font-size="10">"5"</text>
            <text x="6" y=pad_t + 6.0 fill=SLATE_LIGHT font-size="10">"27"</text>

            <polyline fill="none" stroke=VIOLET stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" points=phq9_points />
            <polyline fill="none" stroke=ROSE stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" points=gad7_points />

            {series.into_iter().enumerate().map(|(i, p)| {
                let cx = x_for(i);
                let phq_circle = p.phq9.map(|v| view! {
                    <circle cx=cx cy=y_for(v) r="4" fill=VIOLET stroke="#0f172a" stroke-width="1.5" />
                });
                let gad_circle = p.gad7.map(|v| view! {
                    <circle cx=cx cy=y_for(v) r="4" fill=ROSE stroke="#0f172a" stroke-width="1.5" />
                });
                view! {
                    <g>
                        {phq_circle}
                        {gad_circle}
                        <text x=cx y=height - 8.0 text-anchor="middle" fill=SLATE_LIGHT font-size="10">{p.date}</text>
                    </g>
                }
            }).collect_view()}
        </svg>
    }
}

#[component]
fn HealthConnectSyncCard(
    status: HealthBridgeStatusUi,
    busy: RwSignal<bool>,
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    on_request_permission: Callback<()>,
    on_sync_days: Callback<u32>,
) -> impl IntoView {
    let platform_kind = status.platform_kind();
    let platform_name = platform_kind.display_name();
    let permission_summary = platform_kind.permission_summary();
    let sdk_available = status.sdk_status == "available";
    let (badge_label, badge_class) = if !sdk_available {
        if status.sdk_status == "update_required" {
            (
                "Update Health Connect",
                "border-amber-400/35 bg-amber-500/15 text-amber-200",
            )
        } else {
            (
                "SDK Unavailable",
                "border-rose-400/35 bg-rose-500/15 text-rose-200",
            )
        }
    } else if status.has_glucose_permission {
        (
            "Connected · Read-only",
            "border-emerald-400/35 bg-emerald-500/15 text-emerald-200",
        )
    } else {
        (
            "Permission required",
            "border-amber-400/35 bg-amber-500/15 text-amber-200",
        )
    };

    let package_id = status.package_id.clone();
    let has_permission = status.has_glucose_permission;

    view! {
        <div
            data-health-connect-card="true"
            class="mb-6 rounded-2xl border border-emerald-400/30 bg-emerald-500/10 p-4 text-xs text-slate-200"
        >
            <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
                <div>
                    <p class="text-[11px] font-semibold uppercase tracking-wider text-emerald-300">
                        {format!("{platform_name} · Live On-Device Sync")}
                    </p>
                    <p class="mt-0.5 text-[11px] text-slate-300">
                        {format!("Personal package {package_id} · {permission_summary}")}
                    </p>
                </div>
                <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {badge_class}")>
                    {badge_label}
                </span>
            </div>

            {if !sdk_available {
                view! {
                    <p class="text-[11px] leading-relaxed text-amber-200">
                        {if status.sdk_status == "update_required" {
                            "Health Connect requires an update on this Android device. Update Health Connect in Google Play or Android System Settings, then reopen Dabney.moe Personal."
                        } else {
                            platform_kind.unavailable_hint()
                        }}
                    </p>
                }
                .into_any()
            } else {
                view! {
                    <div class="flex flex-wrap items-center gap-2">
                        {if !has_permission {
                            view! {
                                <button
                                    type="button"
                                    disabled=move || busy.get()
                                    on:click=move |_| on_request_permission.run(())
                                    class="rounded-full bg-emerald-400 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-300 disabled:opacity-50"
                                >
                                    {move || if busy.get() {
                                        format!("Opening {platform_name}…")
                                    } else {
                                        "Grant Blood Glucose Access".to_string()
                                    }}
                                </button>
                            }
                            .into_any()
                        } else {
                            view! {
                                <button
                                    type="button"
                                    disabled=move || busy.get()
                                    on:click=move |_| on_sync_days.run(14)
                                    class="rounded-full bg-emerald-400 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-300 disabled:opacity-50"
                                >
                                    {move || if busy.get() { "Syncing…" } else { "Sync Last 14 Days" }}
                                </button>
                                <button
                                    type="button"
                                    disabled=move || busy.get()
                                    on:click=move |_| on_sync_days.run(90)
                                    class="rounded-full border border-emerald-300/40 bg-slate-950/60 px-3.5 py-1.5 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/20 disabled:opacity-50"
                                >
                                    "Sync Last 90 Days"
                                </button>
                            }
                            .into_any()
                        }}
                    </div>
                }
                .into_any()
            }}

            {move || message.get().map(|msg| view! {
                <p class="mt-2 text-[11px] text-emerald-200">{msg}</p>
            })}
            {move || error.get().map(|err| view! {
                <p class="mt-2 text-[11px] text-rose-300">{err}</p>
            })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health_bridge::{HealthBridgePermissionUi, HealthBridgeReadUi};
    use leptos::prelude::RenderHtml;

    #[test]
    fn health_connect_bridge_payloads_and_card_render_cleanly() {
        let status: HealthBridgeStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "platform": "android_health_connect",
                "sdk_status": "available",
                "has_glucose_permission": false,
                "package_id": "moe.dabney.personal"
            }"#,
        )
        .unwrap();
        assert!(status.enabled);
        assert!(!status.has_glucose_permission);

        let perm: HealthBridgePermissionUi = serde_json::from_str(
            r#"{
                "granted": true,
                "granted_permissions": ["android.permission.health.READ_BLOOD_GLUCOSE"]
            }"#,
        )
        .unwrap();
        assert!(perm.granted);
        assert_eq!(perm.granted_permissions.len(), 1);

        let read: HealthBridgeReadUi = serde_json::from_str(
            r#"{
                "bridge_json": "{\"schema\":\"dabney.health_bridge.v1\",\"records\":[]}",
                "record_count": 2,
                "sources": ["com.abbott.lingo"]
            }"#,
        )
        .unwrap();
        assert_eq!(read.record_count, 2);
        assert_eq!(read.sources, vec!["com.abbott.lingo"]);

        let busy = RwSignal::new(false);
        let msg = RwSignal::new(None);
        let err = RwSignal::new(None);
        let req = Callback::new(|()| {});
        let sync = Callback::new(|_: u32| {});

        let needs_perm_html = view! {
            <HealthConnectSyncCard
                status=status.clone()
                busy=busy
                message=msg
                error=err
                on_request_permission=req
                on_sync_days=sync
            />
        }
        .to_html();
        assert!(needs_perm_html.contains("Android Health Connect"));
        assert!(needs_perm_html.contains("Grant Blood Glucose Access"));
        assert!(needs_perm_html.contains("moe.dabney.personal"));

        let connected_status = HealthBridgeStatusUi {
            has_glucose_permission: true,
            ..status
        };
        let connected_html = view! {
            <HealthConnectSyncCard
                status=connected_status
                busy=busy
                message=msg
                error=err
                on_request_permission=req
                on_sync_days=sync
            />
        }
        .to_html();
        assert!(connected_html.contains("Sync Last 14 Days"));
        assert!(connected_html.contains("Sync Last 90 Days"));

        let ios_status = HealthBridgeStatusUi {
            enabled: true,
            platform: "ios_healthkit".to_string(),
            sdk_status: "available".to_string(),
            has_glucose_permission: true,
            package_id: "moe.dabney.personal".to_string(),
        };
        let ios_html = view! {
            <HealthConnectSyncCard
                status=ios_status
                busy=busy
                message=msg
                error=err
                on_request_permission=req
                on_sync_days=sync
            />
        }
        .to_html();
        assert!(ios_html.contains("Apple HealthKit"));
        assert!(ios_html.contains("HKQuantityTypeIdentifierBloodGlucose"));
        assert!(ios_html.contains("Sync Last 14 Days"));
    }

    #[test]
    fn mental_health_chart_and_metrics_render_cleanly() {
        let import = parse_commercial_glucose_input(SAMPLE_LINGO_CSV, None).unwrap();
        let readings = import.readings;
        let checkins = sample_checkins();
        let html = view! {
            <LingoMoodTimelineChart readings=readings checkins=checkins />
        }
        .to_html();
        assert!(html.contains("Abbott Lingo continuous glucose monitoring"));
        assert!(html.contains("<polyline"));
        assert!(html.contains("140"));
        assert!(html.contains("70"));

        let diag_html = view! {
            <ImportValidationPanel diagnostics=import.diagnostics />
        }
        .to_html();
        assert!(diag_html.contains("Import Validation Report"));
        assert!(diag_html.contains("Timestamps &amp; Chronology"));

        let trend_html = view! {
            <Phq9Gad7TrendChart series=sample_assessment_history() />
        }
        .to_html();
        assert!(trend_html.contains("PHQ-9 and GAD-7 longitudinal score trajectory"));
        assert!(trend_html.contains("2026-10-31"));
    }

    #[test]
    fn timeline_svg_breaks_sensor_gaps_renders_dates_and_styles_saturation() {
        use crate::mental_health_lingo::GlucoseReadingBound;

        let readings = vec![
            LingoReading {
                timestamp: "2026-07-01T08:00-07:00".to_string(),
                hour_offset: 0.0,
                glucose_mg_dl: 54.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::BelowSensorFloor,
            },
            LingoReading {
                timestamp: "2026-07-01T09:00-07:00".to_string(),
                hour_offset: 1.0,
                glucose_mg_dl: 108.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            },
            // 48-hour sensor-off gap -> must render as a separate <polyline>
            LingoReading {
                timestamp: "2026-07-03T09:00-07:00".to_string(),
                hour_offset: 49.0,
                glucose_mg_dl: 201.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::AboveSensorCeiling,
            },
            LingoReading {
                timestamp: "2026-07-03T10:00-07:00".to_string(),
                hour_offset: 50.0,
                glucose_mg_dl: 155.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            },
        ];

        let html = view! {
            <LingoMoodTimelineChart readings=readings checkins=Vec::new() />
        }
        .to_html();

        assert_eq!(html.matches("<polyline").count(), 2);
        assert!(html.contains(">180</text>"));
        assert!(html.contains("2026-07-01"));
        assert!(html.contains("2026-07-02"));
        assert!(html.contains("2026-07-03"));
        assert!(html.contains("#f97316"), "ceiling-capped spike marker");
        assert!(html.contains("#f43f5e"), "floor-capped dip marker");
    }

    #[test]
    fn import_validation_panel_surfaces_out_of_range_rows() {
        let mut import = parse_commercial_glucose_input(SAMPLE_LINGO_CSV, None).unwrap();
        import.diagnostics.out_of_range_rows = 12;
        import.diagnostics.skipped_invalid_rows = 1;
        let html = view! {
            <ImportValidationPanel diagnostics=import.diagnostics />
        }
        .to_html();
        assert!(html.contains("Cap / OOR / Invalid"));
        assert!(html.contains("0 / 12 / 1"));
    }
}
