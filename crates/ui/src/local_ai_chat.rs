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

    view! {
        <div
            data-personal-vault-bar="true"
            class="mb-6 rounded-2xl border border-cyan-400/30 bg-cyan-500/10 p-4 text-xs text-slate-200"
        >
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div>
                    <p class="text-[11px] font-semibold uppercase tracking-wider text-cyan-300">
                        "Encrypted Local Vault · On-Device Only"
                    </p>
                    <p class="mt-0.5 text-[11px] text-slate-300">
                        {format!(
                            "{cipher} · {cgm_count} CGM readings · {checkins} check-ins · {chats} chat turns · {kb:.1} KB (cloud backup disabled)"
                        )}
                    </p>
                </div>
                {has_data.then(|| view! {
                    <button
                        type="button"
                        disabled=move || busy.get()
                        on:click=move |_| on_wipe.run(())
                        class="rounded-full border border-rose-400/40 bg-rose-500/15 px-3 py-1 text-[11px] font-semibold text-rose-200 transition hover:bg-rose-500/25 disabled:opacity-50"
                    >
                        {move || if busy.get() { "Wiping…" } else { "Cryptographic Wipe" }}
                    </button>
                })}
            </div>
            {move || notice.get().map(|msg| view! {
                <p class="mt-2 text-[11px] text-cyan-200">{msg}</p>
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
            "Grounded Rust Fallback (AICore Unavailable)",
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
                            class="rounded-full bg-amber-400 px-3 py-1 text-[11px] font-semibold text-slate-950 transition hover:bg-amber-300 disabled:opacity-50"
                        >
                            {move || if preparing.get() { "Downloading via AICore…" } else { "Download Gemini Nano (AICore)" }}
                        </button>
                    })}
            </div>
            {move || prepare_notice.get().map(|msg| view! {
                <p class="text-[11px] text-cyan-200">{msg}</p>
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
            class="mb-8 rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-6 space-y-4"
        >
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                        "On-Device Metabolic & Mood Chat · Zero Cloud Egress"
                    </p>
                    <h2 class="mt-1 text-lg font-semibold text-white">
                        "Ask About Your Glucose & Mood Trends"
                    </h2>
                    <p class="mt-1 text-xs text-slate-400">
                        "Grounded strictly in your loaded CGM metrics, 24h AGP, and logged check-ins. "
                        "Uses Android AICore / Gemini Nano on supported devices and falls back to local Rust synthesis elsewhere."
                    </p>
                </div>
                {move || (!chat_turns.get().is_empty()).then(|| view! {
                    <button
                        type="button"
                        on:click=move |_| on_clear_chat.run(())
                        class="rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:bg-white/5"
                    >
                        "Clear thread"
                    </button>
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
                <p class="text-xs text-rose-300">{err}</p>
            })}

            <div aria-live="polite" class="space-y-3">
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
        assert!(vault_html.contains("144 CGM readings"));
        assert!(vault_html.contains("Cryptographic Wipe"));

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
        assert!(fb_html.contains("Grounded Rust Fallback"));

        let chat_html = view! {
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
        assert!(chat_html.contains("How do my last 14 days compare to the prior 14 days?"));
        assert!(chat_html.contains("How are my overnight glucose and low episodes?"));
    }
}
