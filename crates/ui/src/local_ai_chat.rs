//! Reusable Leptos UI components for opt-in encrypted local vault controls and
//! on-device LLM chat (Android AICore / Gemini Nano with automatic grounded Rust fallback).

use leptos::prelude::*;

use crate::health_bridge::{
    LocalChatTurn, LocalLlmAvailability, LocalLlmStatusUi, PersonalVaultStatusUi,
};

const CGM_CHAT_PRESETS: &[&str] = &[
    "Summarize my glucose stability, TIR, and biggest spikes",
    "How do my last 14 days compare to the prior 14 days?",
    "How are my overnight glucose and low episodes?",
    "What time of day do my glucose spikes peak in my 24h AGP?",
    "How do my mood and anxiety check-ins line up with glucose swings?",
    "Draft 3 focused questions to ask my doctor at my next visit",
];

/// Compact encrypted personal vault status bar rendered inside `moe.dabney.personal`
/// (`vault.enabled == true`). Absent on web browsers and `moe.dabney.demo`.
#[component]
pub fn PersonalVaultBar(
    status: PersonalVaultStatusUi,
    busy: RwSignal<bool>,
    notice: RwSignal<Option<String>>,
    on_wipe: Callback<()>,
) -> impl IntoView {
    let kb = status.total_bytes as f64 / 1024.0;
    let cipher = status.cipher.clone();
    let cgm_count = status.cgm_reading_count;
    let checkins = status.checkin_count;
    let chats = status.chat_turn_count;
    let has_data = status.total_bytes > 0 || status.has_cgm_snapshot || checkins > 0 || chats > 0;
    let confirm_wipe = RwSignal::new(false);

    view! {
        <div
            data-personal-vault-bar="true"
            class="print-hide print:hidden mb-6 rounded-2xl border border-cyan-400/30 bg-cyan-500/10 p-4 text-xs text-slate-200"
        >
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div>
                    <p class="text-[11px] font-semibold uppercase tracking-wider text-cyan-300">
                        "Encrypted Local Vault · On-Device Only"
                    </p>
                    <p class="mt-0.5 text-[11px] text-slate-300">
                        {format!(
                            "Stored encrypted on this device only · {cgm_count} glucose readings · {checkins} check-ins · {chats} chat turns · {kb:.1} KB (cloud backup disabled)"
                        )}
                    </p>
                    <details class="mt-1 text-[10px] text-slate-400">
                        <summary class="cursor-pointer hover:text-slate-300">
                            "Technical encryption details"
                        </summary>
                        <p class="mt-0.5 font-mono text-[10px] text-slate-300">{cipher}</p>
                    </details>
                </div>
                {has_data.then(|| view! {
                    <div class="flex flex-wrap items-center gap-2">
                        {move || if confirm_wipe.get() {
                            view! {
                                <div class="flex flex-wrap items-center gap-1.5 rounded-xl border border-rose-400/40 bg-rose-950/60 px-2.5 py-1.5">
                                    <span class="text-[11px] font-medium text-rose-100">
                                        "Permanently erase all saved local data?"
                                    </span>
                                    <button
                                        type="button"
                                        disabled=move || busy.get()
                                        on:click=move |_| {
                                            confirm_wipe.set(false);
                                            on_wipe.run(());
                                        }
                                        class="min-h-[36px] rounded-full bg-rose-500 px-3 py-1 text-[11px] font-semibold text-white transition hover:bg-rose-400 disabled:opacity-50"
                                    >
                                        {move || if busy.get() { "Wiping…" } else { "Confirm wipe" }}
                                    </button>
                                    <button
                                        type="button"
                                        disabled=move || busy.get()
                                        on:click=move |_| confirm_wipe.set(false)
                                        class="min-h-[36px] rounded-full border border-white/20 px-3 py-1 text-[11px] font-medium text-slate-200 transition hover:bg-white/10 disabled:opacity-50"
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
                                    disabled=move || busy.get()
                                    on:click=move |_| confirm_wipe.set(true)
                                    class="min-h-[36px] rounded-full border border-rose-400/40 bg-rose-500/15 px-3.5 py-1.5 text-[11px] font-semibold text-rose-200 transition hover:bg-rose-500/25 disabled:opacity-50"
                                >
                                    {move || if busy.get() { "Wiping…" } else { "Erase local data" }}
                                </button>
                            }
                            .into_any()
                        }}
                    </div>
                })}
            </div>
            {move || notice.get().map(|msg| view! {
                <p role="status" aria-live="polite" class="mt-2 text-[11px] text-cyan-200">{msg}</p>
            })}
        </div>
    }
}

/// Compact hardware & on-device LLM status chip shared by `/mental-health`,
/// `/clinical/patient`, and `/clinical/physician`.
#[component]
pub fn LocalLlmStatusBanner(
    status: LocalLlmStatusUi,
    use_local_llm: RwSignal<bool>,
    preparing: RwSignal<bool>,
    prepare_notice: RwSignal<Option<String>>,
    on_prepare: Callback<()>,
) -> impl IntoView {
    let availability = status.availability_kind();
    let (pill_label, pill_cls) = match availability {
        LocalLlmAvailability::Available => (
            "Gemini Nano · On-Device NPU Ready",
            "border-emerald-400/40 bg-emerald-500/15 text-emerald-200",
        ),
        LocalLlmAvailability::Downloadable => (
            "Gemini Nano · Downloadable via AICore",
            "border-amber-400/40 bg-amber-500/15 text-amber-200",
        ),
        LocalLlmAvailability::Downloading => (
            "Gemini Nano · Downloading via AICore…",
            "border-cyan-400/40 bg-cyan-500/15 text-cyan-200",
        ),
        LocalLlmAvailability::Unavailable => (
            "On-Device Summary Mode (Local Rules)",
            "border-slate-500/40 bg-slate-800/80 text-slate-300",
        ),
    };

    let device_summary = status.device_summary.clone();
    let detail = status.detail.clone();

    view! {
        <div
            data-local-llm-banner="true"
            class="rounded-xl border border-white/10 bg-slate-950/70 p-3 text-xs text-slate-300 space-y-2"
        >
            <div class="flex flex-wrap items-center justify-between gap-2">
                <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {pill_cls}")>
                    {pill_label}
                </span>
                <span class="font-mono text-[10px] text-slate-400">{device_summary}</span>
            </div>
            <p class="text-[11px] text-slate-300">{detail}</p>
            <div class="flex flex-wrap items-center gap-2">
                {(availability == LocalLlmAvailability::Available).then(|| view! {
                    <label class="inline-flex cursor-pointer items-center gap-2 text-[11px] text-emerald-200">
                        <input
                            type="checkbox"
                            prop:checked=move || use_local_llm.get()
                            on:change=move |ev| use_local_llm.set(event_target_checked(&ev))
                            class="h-3.5 w-3.5 rounded border-emerald-400/50 bg-slate-900 accent-emerald-400"
                        />
                        <span>"Use on-device Gemini Nano for grounded replies (100% offline)"</span>
                    </label>
                })}
                {(availability == LocalLlmAvailability::Downloadable
                    || availability == LocalLlmAvailability::Downloading)
                    .then(|| view! {
                        <button
                            type="button"
                            disabled=move || preparing.get()
                            on:click=move |_| on_prepare.run(())
                            class="inline-flex min-h-[36px] items-center gap-2 rounded-full bg-amber-400 px-3.5 py-1.5 text-[11px] font-semibold text-slate-950 transition hover:bg-amber-300 disabled:opacity-50"
                        >
                            {move || preparing.get().then(|| view! {
                                <span
                                    aria-hidden="true"
                                    class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-slate-950/30 border-t-slate-950"
                                />
                            })}
                            <span>
                                {move || if preparing.get() { "Downloading on-device model…" } else { "Download On-Device Model" }}
                            </span>
                        </button>
                    })}
            </div>
            {move || prepare_notice.get().map(|msg| view! {
                <p role="status" aria-live="polite" class="text-[11px] text-cyan-200">{msg}</p>
            })}
        </div>
    }
}

/// Interactive on-device CGM & mood reflection chat card for `/mental-health`.
/// Uses Android AICore / Gemini Nano on supported `moe.dabney.personal` devices
/// and falls back seamlessly to deterministic grounded Rust synthesis everywhere else.
#[component]
pub fn PersonalCgmAiChatSection(
    llm_status: RwSignal<Option<LocalLlmStatusUi>>,
    use_local_llm: RwSignal<bool>,
    llm_preparing: RwSignal<bool>,
    llm_prepare_notice: RwSignal<Option<String>>,
    chat_input: RwSignal<String>,
    chat_turns: RwSignal<Vec<LocalChatTurn>>,
    chat_busy: RwSignal<bool>,
    chat_error: RwSignal<Option<String>>,
    on_prepare_llm: Callback<()>,
    on_send_question: Callback<String>,
    on_clear_chat: Callback<()>,
) -> impl IntoView {
    let confirm_clear = RwSignal::new(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let q = chat_input.get().trim().to_string();
        if !q.is_empty() && !chat_busy.get() {
            chat_input.set(String::new());
            on_send_question.run(q);
        }
    };

    view! {
        <section
            data-personal-cgm-chat="true"
            class="print-hide print:hidden mb-8 rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-6 space-y-4"
        >
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                        "On-Device Metabolic & Mood Assistant · Private"
                    </p>
                    <h2 class="mt-1 text-lg font-semibold text-white">
                        "Ask About Your Glucose & Mood Trends"
                    </h2>
                    <p class="mt-1 text-xs text-slate-400">
                        "Answers are grounded strictly in your loaded glucose trends, 24-hour patterns, and logged check-ins — everything stays privately on your device."
                    </p>
                </div>
                {move || (!chat_turns.get().is_empty()).then(|| view! {
                    <div>
                        {move || if confirm_clear.get() {
                            view! {
                                <div class="flex flex-wrap items-center gap-1.5 rounded-xl border border-rose-400/35 bg-rose-950/50 px-2.5 py-1.5">
                                    <span class="text-[11px] text-rose-100">"Clear conversation?"</span>
                                    <button
                                        type="button"
                                        on:click=move |_| {
                                            confirm_clear.set(false);
                                            on_clear_chat.run(());
                                        }
                                        class="min-h-[34px] rounded-full bg-rose-500 px-3 py-1 text-xs font-semibold text-white transition hover:bg-rose-400"
                                    >
                                        "Confirm wipe"
                                    </button>
                                    <button
                                        type="button"
                                        on:click=move |_| confirm_clear.set(false)
                                        class="min-h-[34px] rounded-full border border-white/20 px-3 py-1 text-xs text-slate-200 transition hover:bg-white/10"
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
                                    on:click=move |_| confirm_clear.set(true)
                                    class="min-h-[36px] rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:bg-white/5"
                                >
                                    "Clear thread"
                                </button>
                            }
                            .into_any()
                        }}
                    </div>
                })}
            </div>

            {move || {
                llm_status.get().filter(|s| s.enabled).map(|status| {
                    view! {
                        <LocalLlmStatusBanner
                            status=status
                            use_local_llm=use_local_llm
                            preparing=llm_preparing
                            prepare_notice=llm_prepare_notice
                            on_prepare=on_prepare_llm
                        />
                    }
                })
            }}

            <div class="flex flex-wrap gap-2">
                {CGM_CHAT_PRESETS
                    .iter()
                    .map(|&preset| {
                        view! {
                            <button
                                type="button"
                                disabled=move || chat_busy.get()
                                on:click=move |_| on_send_question.run(preset.to_string())
                                class="rounded-full border border-white/15 bg-slate-950/70 px-3 py-1.5 text-xs text-slate-200 transition hover:border-cyan-400/50 hover:text-white disabled:opacity-50"
                            >
                                {preset}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>

            <form on:submit=on_submit class="flex flex-col gap-2.5 sm:flex-row">
                <input
                    type="text"
                    aria-label="Ask an on-device question about your loaded CGM and mood data"
                    placeholder="Ask about spikes, 24h AGP hours, mood patterns, or visit questions..."
                    prop:value=move || chat_input.get()
                    on:input=move |ev| chat_input.set(event_target_value(&ev))
                    class="min-h-[44px] flex-1 rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2.5 text-sm text-white placeholder:text-slate-500 focus:border-cyan-400/60 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                />
                <button
                    type="submit"
                    disabled=move || chat_busy.get()
                    class="min-h-[44px] rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
                >
                    {move || if chat_busy.get() { "Thinking on-device…" } else { "Ask locally" }}
                </button>
            </form>

            {move || chat_error.get().map(|err| view! {
                <p role="alert" class="text-xs text-rose-300">{err}</p>
            })}

            <div aria-live="polite" class="space-y-3">
                {move || chat_busy.get().then(|| view! {
                    <div
                        role="status"
                        class="flex items-center gap-2.5 rounded-xl border border-cyan-400/25 bg-cyan-500/10 px-4 py-3 text-xs text-cyan-100"
                    >
                        <span
                            aria-hidden="true"
                            class="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-cyan-300/30 border-t-cyan-300"
                        />
                        <span>"Reviewing your loaded glucose trends and check-ins on-device…"</span>
                    </div>
                })}
                {move || {
                    let turns = chat_turns.get();
                    turns
                        .into_iter()
                        .rev()
                        .map(|turn| {
                            let latency = turn
                                .latency_ms
                                .map(|ms| format!(" · {ms} ms"))
                                .unwrap_or_default();
                            view! {
                                <article class="rounded-xl border border-white/10 bg-slate-950/75 p-4 space-y-2.5">
                                    <div class="flex flex-wrap items-center justify-between gap-2">
                                        <p class="text-xs font-semibold text-cyan-200">
                                            {turn.question}
                                        </p>
                                        <span class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2.5 py-0.5 font-mono text-[10px] text-emerald-200">
                                            {format!("{}{latency}", turn.engine_label)}
                                        </span>
                                    </div>
                                    <p class="whitespace-pre-line text-sm leading-relaxed text-slate-200">
                                        {turn.answer}
                                    </p>
                                    {(!turn.tool_steps.is_empty()).then(|| {
                                        let step_count = turn.tool_steps.len();
                                        let step_word = if step_count == 1 { "step" } else { "steps" };
                                        view! {
                                            <details class="rounded-lg border border-white/10 bg-slate-900/60 px-3 py-2">
                                                <summary class="cursor-pointer select-none text-[11px] font-medium text-cyan-300 transition hover:text-cyan-200">
                                                    {format!("Agent trace · {step_count} local tool {step_word}")}
                                                </summary>
                                                <ol class="mt-2 space-y-1.5 border-t border-white/10 pt-2">
                                                    {turn
                                                        .tool_steps
                                                        .into_iter()
                                                        .enumerate()
                                                        .map(|(idx, step)| {
                                                            let badge_cls = if step.status == "ok" {
                                                                "border-emerald-400/35 bg-emerald-500/10 text-emerald-200"
                                                            } else {
                                                                "border-amber-400/35 bg-amber-500/10 text-amber-200"
                                                            };
                                                            view! {
                                                                <li class="rounded-md border border-white/5 bg-slate-950/75 px-2.5 py-1.5 text-[11px]">
                                                                    <div class="flex flex-wrap items-center justify-between gap-1.5">
                                                                        <div class="flex items-center gap-1.5">
                                                                            <span class="font-mono text-[10px] text-slate-400">
                                                                                {format!("#{}", idx + 1)}
                                                                            </span>
                                                                            <span class="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[10px] text-cyan-200">
                                                                                {step.tool_name}
                                                                            </span>
                                                                            <span class="font-medium text-slate-200">
                                                                                {step.label}
                                                                            </span>
                                                                        </div>
                                                                        <span class=format!("rounded-full border px-2 py-0.5 font-mono text-[10px] {badge_cls}")>
                                                                            {step.status}
                                                                        </span>
                                                                    </div>
                                                                    <p class="mt-1 text-slate-300">{step.summary}</p>
                                                                </li>
                                                            }
                                                        })
                                                        .collect_view()}
                                                </ol>
                                            </details>
                                        }
                                    })}
                                    {(!turn.grounding_facts.is_empty()).then(|| view! {
                                        <div class="flex flex-wrap gap-1.5 border-t border-white/5 pt-2">
                                            {turn
                                                .grounding_facts
                                                .into_iter()
                                                .map(|fact| view! {
                                                    <span class="rounded-md border border-white/10 bg-white/[0.03] px-2 py-0.5 text-[10px] text-slate-400">
                                                        {fact}
                                                    </span>
                                                })
                                                .collect_view()}
                                        </div>
                                    })}
                                </article>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    #[test]
    fn vault_bar_and_local_llm_banner_render_cleanly_for_supported_and_fallback_devices() {
        let vault = PersonalVaultStatusUi {
            enabled: true,
            encrypted: true,
            cipher: "AES-256-GCM (Android Keystore)".to_string(),
            storage_backend: "android_keystore_files".to_string(),
            has_cgm_snapshot: true,
            cgm_reading_count: 144,
            cgm_saved_at: Some("2026-11-25T19:00:00Z".to_string()),
            checkin_count: 3,
            note_count: 1,
            chat_turn_count: 2,
            total_bytes: 10240,
        };
        let busy = RwSignal::new(false);
        let notice = RwSignal::new(None);
        let on_wipe = Callback::new(|()| {});
        let vault_html = view! {
            <PersonalVaultBar status=vault busy=busy notice=notice on_wipe=on_wipe />
        }
        .to_html();
        assert!(vault_html.contains("AES-256-GCM (Android Keystore)"));
        assert!(vault_html.contains("144 glucose readings"));
        assert!(vault_html.contains("Erase local data"));
        assert!(vault_html.contains("Stored encrypted on this device only"));

        let llm_ready = LocalLlmStatusUi {
            enabled: true,
            backend: "android_aicore_gemini_nano".to_string(),
            model_name: "Gemini Nano (Android AICore)".to_string(),
            availability: "available".to_string(),
            device_summary: "Android Flagship · Snapdragon · 12.0 GB RAM · API 36".to_string(),
            total_ram_mb: 12288,
            aicore_installed: true,
            detail: "On-device Gemini Nano is ready.".to_string(),
        };
        let use_llm = RwSignal::new(true);
        let prep = RwSignal::new(false);
        let prep_notice = RwSignal::new(None);
        let on_prep = Callback::new(|()| {});
        let ready_html = view! {
            <LocalLlmStatusBanner
                status=llm_ready.clone()
                use_local_llm=use_llm
                preparing=prep
                prepare_notice=prep_notice
                on_prepare=on_prep
            />
        }
        .to_html();
        assert!(ready_html.contains("Gemini Nano · On-Device NPU Ready"));
        assert!(ready_html.contains("Snapdragon"));

        let llm_fallback = LocalLlmStatusUi {
            availability: "unavailable".to_string(),
            aicore_installed: false,
            detail: "Grounded Rust synthesis active.".to_string(),
            ..llm_ready
        };
        let fb_html = view! {
            <LocalLlmStatusBanner
                status=llm_fallback
                use_local_llm=use_llm
                preparing=prep
                prepare_notice=prep_notice
                on_prepare=on_prep
            />
        }
        .to_html();
        assert!(fb_html.contains("On-Device Summary Mode (Local Rules)"));

        let chat_turns = RwSignal::new(vec![LocalChatTurn {
            id: "cgm-chat-1".to_string(),
            question: "Summarize my glucose stability".to_string(),
            answer: "Mean glucose is 104 mg/dL with 96% TIR.".to_string(),
            engine_label: "Grounded Rust · On-Device".to_string(),
            latency_ms: None,
            grounding_facts: vec!["TIR 70–180: 96%".to_string()],
            tool_steps: vec![crate::health_bridge::LocalToolTraceStep {
                tool_name: "cgm_window_metrics".to_string(),
                label: "Compute CGM window metrics".to_string(),
                status: "ok".to_string(),
                summary: "Abbott Lingo · 144 readings".to_string(),
            }],
        }]);
        let chat_html = view! {
            <PersonalCgmAiChatSection
                llm_status=RwSignal::new(None)
                use_local_llm=use_llm
                llm_preparing=prep
                llm_prepare_notice=prep_notice
                chat_input=RwSignal::new(String::new())
                chat_turns=chat_turns
                chat_busy=RwSignal::new(false)
                chat_error=RwSignal::new(None)
                on_prepare_llm=on_prep
                on_send_question=Callback::new(|_| {})
                on_clear_chat=Callback::new(|()| {})
            />
        }
        .to_html();
        assert!(chat_html.contains("Agent trace · 1 local tool step"));
        assert!(chat_html.contains("cgm_window_metrics"));

        let empty_chat_html = view! {
            <PersonalCgmAiChatSection
                llm_status=RwSignal::new(None)
                use_local_llm=use_llm
                llm_preparing=prep
                llm_prepare_notice=prep_notice
                chat_input=RwSignal::new(String::new())
                chat_turns=RwSignal::new(Vec::new())
                chat_busy=RwSignal::new(false)
                chat_error=RwSignal::new(None)
                on_prepare_llm=on_prep
                on_send_question=Callback::new(|_| {})
                on_clear_chat=Callback::new(|()| {})
            />
        }
        .to_html();
        assert!(empty_chat_html.contains("How do my last 14 days compare to the prior 14 days?"));
        assert!(empty_chat_html.contains("How are my overnight glucose and low episodes?"));
    }
}
