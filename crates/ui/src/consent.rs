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

/// Single hydration-safe consent state (one signal avoids loaded/choices update races).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ConsentStatus {
    /// SSR + first hydration pass: localStorage not read yet; never show the banner.
    Pending,
    /// localStorage read, no saved decision.
    Undecided,
    /// localStorage read or user just saved.
    Decided(ConsentChoices),
}

pub fn provide_consent_context() {
    let status = RwSignal::new(ConsentStatus::Pending);
    let dialog_open = RwSignal::new(false);

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        Effect::new(move |_| {
            if !matches!(status.get_untracked(), ConsentStatus::Pending) {
                return;
            }
            status.set(match load_consent() {
                Some(choices) => {
                    apply_consent_scripts(&choices);
                    ConsentStatus::Decided(choices)
                }
                None => ConsentStatus::Undecided,
            });
        });
    }

    provide_context(status);
    provide_context(dialog_open);
}

pub fn use_consent() -> Memo<Option<ConsentChoices>> {
    let status = expect_context::<RwSignal<ConsentStatus>>();
    Memo::new(move |_| match status.get() {
        ConsentStatus::Decided(choices) => Some(choices),
        _ => None,
    })
}

#[component]
pub fn ConsentBanner() -> impl IntoView {
    let status = expect_context::<RwSignal<ConsentStatus>>();
    let dialog_open = expect_context::<RwSignal<bool>>();
    let needs_consent = Memo::new(move |_| status.get() == ConsentStatus::Undecided);
    let show_preferences = RwSignal::new(false);
    let analytics_on = RwSignal::new(false);
    let error_reporting_on = RwSignal::new(false);

    Effect::new(move |_| {
        if dialog_open.get() {
            if let ConsentStatus::Decided(choices) = status.get_untracked() {
                analytics_on.set(choices.analytics);
                error_reporting_on.set(choices.error_reporting);
            }
            show_preferences.set(true);
            dialog_open.set(false);
        }
    });

    let save = move |analytics: bool, error_reporting: bool| {
        let choices = ConsentChoices {
            analytics,
            error_reporting,
        };
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        if !store_consent(&choices) {
            leptos::logging::warn!("consent: localStorage write failed");
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        let _ = store_consent(&choices);
        status.set(ConsentStatus::Decided(choices.clone()));
        apply_consent_scripts(&choices);
        show_preferences.set(false);
    };

    let accept_all = move |_| save(true, true);
    let reject_optional = move |_| save(false, false);
    let save_preferences = move |_| {
        save(analytics_on.get(), error_reporting_on.get());
    };

    view! {
        <Show when=move || needs_consent.get() || show_preferences.get()>
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
                                        on:click=move |_| show_preferences.set(false)
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

/// Parse persisted consent JSON. `None` = missing or invalid (treat as undecided).
#[cfg(any(feature = "csr", feature = "hydrate", test))]
fn parse_consent_json(raw: &str) -> Option<ConsentChoices> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    Some(ConsentChoices {
        analytics: value.get("analytics")?.as_bool()?,
        error_reporting: value.get("error_reporting")?.as_bool()?,
    })
}

#[cfg(any(feature = "csr", feature = "hydrate", test))]
fn serialize_consent(choices: &ConsentChoices) -> String {
    serde_json::json!({
        "analytics": choices.analytics,
        "error_reporting": choices.error_reporting,
    })
    .to_string()
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn load_consent() -> Option<ConsentChoices> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(STORAGE_KEY).ok()??;
    parse_consent_json(&raw)
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn store_consent(choices: &ConsentChoices) -> bool {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return false;
    };
    storage
        .set_item(STORAGE_KEY, &serialize_consent(choices))
        .is_ok()
}

#[cfg(not(any(feature = "csr", feature = "hydrate")))]
fn store_consent(_choices: &ConsentChoices) -> bool {
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejected_consent() {
        let choices = parse_consent_json(r#"{"analytics":false,"error_reporting":false}"#).unwrap();
        assert!(!choices.analytics);
        assert!(!choices.error_reporting);
    }

    #[test]
    fn parse_accepted_consent() {
        let choices = parse_consent_json(r#"{"analytics":true,"error_reporting":true}"#).unwrap();
        assert!(choices.analytics);
        assert!(choices.error_reporting);
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert!(parse_consent_json("not json").is_none());
        assert!(parse_consent_json(r#"{"analytics":true}"#).is_none());
    }

    #[test]
    fn roundtrip_serialization() {
        let original = ConsentChoices {
            analytics: true,
            error_reporting: false,
        };
        let parsed = parse_consent_json(&serialize_consent(&original)).unwrap();
        assert_eq!(parsed, original);
    }
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
