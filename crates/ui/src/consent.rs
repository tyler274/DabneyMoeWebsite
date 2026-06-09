//! Cookie consent banner and preference storage (GDPR).
//!
//! Analytics and client error reporting scripts load only after explicit opt-in.
//! Choices persist in `localStorage` under [`STORAGE_KEY`].

use leptos::prelude::*;

pub const STORAGE_KEY: &str = "dabney_consent";

/// User consent choices persisted in the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentChoices {
    pub analytics: bool,
    pub error_reporting: bool,
}

/// Reactive signal holding the current consent state. `None` means no decision yet.
pub fn provide_consent_context() -> ReadSignal<Option<ConsentChoices>> {
    // Start with no stored choice so SSR and the hydration pass match. localStorage is
    // read after mount in a client Effect (see consent_loaded).
    let consent = RwSignal::new(None::<ConsentChoices>);
    let dialog_open = RwSignal::new(false);
    let consent_loaded = RwSignal::new(false);

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        Effect::new(move |_| {
            if consent_loaded.get_untracked() {
                return;
            }
            consent.set(load_consent());
            consent_loaded.set(true);
        });
    }

    provide_context(consent);
    provide_context(dialog_open);
    provide_context(consent_loaded);
    consent.read_only()
}

pub fn use_consent() -> ReadSignal<Option<ConsentChoices>> {
    expect_context::<RwSignal<Option<ConsentChoices>>>().read_only()
}

pub fn use_consent_writer() -> RwSignal<Option<ConsentChoices>> {
    expect_context()
}

#[component]
pub fn ConsentBanner() -> impl IntoView {
    let consent = expect_context::<RwSignal<Option<ConsentChoices>>>();
    let dialog_open = expect_context::<RwSignal<bool>>();
    let consent_loaded = expect_context::<RwSignal<bool>>();
    // Hidden until client has read localStorage so SSR HTML matches hydration.
    let show_banner = RwSignal::new(false);
    let show_preferences = RwSignal::new(false);
    let analytics_on = RwSignal::new(false);
    let error_reporting_on = RwSignal::new(false);

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        Effect::new(move |_| {
            if consent_loaded.get() {
                show_banner.set(consent.get().is_none());
            }
        });
    }

    Effect::new(move |_| {
        if dialog_open.get() {
            if let Some(choices) = consent.get_untracked() {
                analytics_on.set(choices.analytics);
                error_reporting_on.set(choices.error_reporting);
            }
            show_preferences.set(true);
            show_banner.set(true);
            dialog_open.set(false);
        }
    });

    let save = move |analytics: bool, error_reporting: bool| {
        let choices = ConsentChoices {
            analytics,
            error_reporting,
        };
        store_consent(&choices);
        consent.set(Some(choices.clone()));
        apply_consent_scripts(&choices);
        show_banner.set(false);
        show_preferences.set(false);
    };

    let accept_all = move |_| save(true, true);
    let reject_optional = move |_| save(false, false);
    let save_preferences = move |_| {
        save(analytics_on.get(), error_reporting_on.get());
    };

    view! {
        <Show when=move || show_banner.get()>
            <div
                class="fixed inset-x-0 bottom-0 z-50 border-t border-white/10 bg-slate-900/95 p-4 shadow-lg backdrop-blur sm:p-6"
                role="dialog"
                aria-label="Cookie consent"
            >
                <div class="mx-auto flex max-w-5xl flex-col gap-4">
                    <Show
                        when=move || !show_preferences.get()
                        fallback=move || view! {
                            <div class="space-y-4">
                                <h2 class="text-lg font-semibold text-white">"Cookie preferences"</h2>
                                <label class="flex items-start gap-3">
                                    <input type="checkbox" checked disabled class="mt-1" />
                                    <span>
                                        <span class="font-medium text-white">"Necessary"</span>
                                        <span class="block text-sm text-slate-400">
                                            "Required for the site to function. Always enabled."
                                        </span>
                                    </span>
                                </label>
                                <label class="flex items-start gap-3">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || analytics_on.get()
                                        on:change=move |ev| {
                                            analytics_on.set(event_target_checked(&ev));
                                        }
                                    />
                                    <span>
                                        <span class="font-medium text-white">"Analytics"</span>
                                        <span class="block text-sm text-slate-400">
                                            "Google Analytics 4 page views to understand how visitors use the site."
                                        </span>
                                    </span>
                                </label>
                                <label class="flex items-start gap-3">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || error_reporting_on.get()
                                        on:change=move |ev| {
                                            error_reporting_on.set(event_target_checked(&ev));
                                        }
                                    />
                                    <span>
                                        <span class="font-medium text-white">"Error reporting"</span>
                                        <span class="block text-sm text-slate-400">
                                            "Anonymous client error reports to help fix bugs."
                                        </span>
                                    </span>
                                </label>
                                <div class="flex flex-wrap gap-3">
                                    <button
                                        type="button"
                                        class="rounded-lg bg-cyan-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-cyan-400"
                                        on:click=save_preferences
                                    >
                                        "Save preferences"
                                    </button>
                                    <button
                                        type="button"
                                        class="rounded-lg border border-white/20 px-4 py-2 text-sm text-slate-300 hover:bg-white/5"
                                        on:click=move |_| show_banner.set(false)
                                    >
                                        "Cancel"
                                    </button>
                                </div>
                            </div>
                        }
                    >
                        <div class="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
                            <div class="max-w-2xl space-y-2">
                                <p class="text-sm text-slate-300">
                                    "We use optional cookies for analytics and error reporting. "
                                    "You can accept all, reject optional cookies, or customize your choices. "
                                    <a href="/privacy" class="text-cyan-400 underline hover:text-cyan-300">
                                        "Privacy policy"
                                    </a>
                                </p>
                            </div>
                            <div class="flex flex-wrap gap-2">
                                <button
                                    type="button"
                                    class="rounded-lg border border-white/20 px-4 py-2 text-sm text-slate-300 hover:bg-white/5"
                                    on:click=move |_| show_preferences.set(true)
                                >
                                    "Customize"
                                </button>
                                <button
                                    type="button"
                                    class="rounded-lg border border-white/20 px-4 py-2 text-sm text-slate-300 hover:bg-white/5"
                                    on:click=reject_optional
                                >
                                    "Reject optional"
                                </button>
                                <button
                                    type="button"
                                    class="rounded-lg bg-cyan-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-cyan-400"
                                    on:click=accept_all
                                >
                                    "Accept all"
                                </button>
                            </div>
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn event_target_checked(ev: &leptos::ev::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.checked())
        .unwrap_or(false)
}

#[cfg(not(any(feature = "csr", feature = "hydrate")))]
fn event_target_checked(_ev: &leptos::ev::Event) -> bool {
    false
}

fn load_consent() -> Option<ConsentChoices> {
    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        let storage = web_sys::window()?.local_storage().ok()??;
        let raw = storage.get_item(STORAGE_KEY).ok()??;
        parse_consent(&raw)
    }
    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    {
        let _ = STORAGE_KEY;
        None
    }
}

fn store_consent(choices: &ConsentChoices) {
    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let json = format!(
                r#"{{"analytics":{},"error_reporting":{}}}"#,
                choices.analytics, choices.error_reporting
            );
            let _ = storage.set_item(STORAGE_KEY, &json);
        }
    }
    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    {
        let _ = choices;
    }
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn parse_consent(raw: &str) -> Option<ConsentChoices> {
    let analytics = raw.contains(r#""analytics":true"#);
    let error_reporting = raw.contains(r#""error_reporting":true"#);
    Some(ConsentChoices {
        analytics,
        error_reporting,
    })
}

/// Apply consent updates to third-party scripts (GA4 consent mode, error reporter).
pub fn apply_consent_scripts(choices: &ConsentChoices) {
    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        update_gtag_consent(choices.analytics);
        if choices.error_reporting {
            crate::analytics::enable_error_reporting();
        }
    }
    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    {
        let _ = choices;
    }
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn update_gtag_consent(analytics: bool) {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    let Some(window) = web_sys::window() else {
        return;
    };
    let gtag = Reflect::get(window.as_ref(), &JsValue::from_str("gtag")).ok();
    let Some(gtag) = gtag.and_then(|f| f.dyn_into::<Function>().ok()) else {
        return;
    };
    let granted = if analytics { "granted" } else { "denied" };
    let params = js_sys::Object::new();
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("analytics_storage"),
        &JsValue::from_str(granted),
    );
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("ad_storage"),
        &JsValue::from_str("denied"),
    );
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("ad_user_data"),
        &JsValue::from_str("denied"),
    );
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("ad_personalization"),
        &JsValue::from_str("denied"),
    );
    let _ = gtag.call3(
        &JsValue::NULL,
        &JsValue::from_str("consent"),
        &JsValue::from_str("update"),
        &params,
    );
}

/// Re-open the consent banner from the footer or privacy page.
#[component]
pub fn OpenConsentButton() -> impl IntoView {
    let dialog_open = expect_context::<RwSignal<bool>>();
    view! {
        <button
            type="button"
            class="text-cyan-400 underline hover:text-cyan-300"
            on:click=move |_| dialog_open.set(true)
        >
            "Cookie preferences"
        </button>
    }
}
