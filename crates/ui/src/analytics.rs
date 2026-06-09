//! GA4 analytics and client error reporting (consent-gated).

use leptos::prelude::*;
use leptos_router::hooks::use_location;

use crate::consent::use_consent;

#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::consent::{apply_consent_scripts, ConsentChoices};

/// GA4 measurement ID from the `GA4_MEASUREMENT_ID` env var (set in Cloud Run / dev).
pub fn ga4_measurement_id() -> Option<String> {
    #[cfg(feature = "ssr")]
    {
        std::env::var("GA4_MEASUREMENT_ID")
            .ok()
            .filter(|id| !id.is_empty())
    }
    #[cfg(not(feature = "ssr"))]
    {
        None
    }
}

/// Consent Mode v2 defaults and deferred gtag loader injected in the document head.
#[component]
pub fn AnalyticsHead() -> impl IntoView {
    let measurement_id = ga4_measurement_id().unwrap_or_default();
    let has_ga4 = !measurement_id.is_empty();

    view! {
        <script>
            {r#"
window.dataLayer = window.dataLayer || [];
function gtag(){dataLayer.push(arguments);}
gtag('consent', 'default', {
  analytics_storage: 'denied',
  ad_storage: 'denied',
  ad_user_data: 'denied',
  ad_personalization: 'denied',
  wait_for_update: 500
});
"#
            .to_string()}
        </script>
        <Show when=move || has_ga4>
            <script async src=format!(
                "https://www.googletagmanager.com/gtag/js?id={}",
                measurement_id
            )></script>
            <script>
                {format!(
                    r#"
gtag('js', new Date());
gtag('config', '{measurement_id}', {{
  anonymize_ip: true,
  allow_google_signals: false,
  allow_ad_personalization_signals: false
}});
"#
                )}
            </script>
        </Show>
    }
}

/// Restores consent from storage on mount and tracks SPA page views when allowed.
#[component]
pub fn AnalyticsRuntime() -> impl IntoView {
    let consent = use_consent();
    let location = use_location();

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        Effect::new(move |_| {
            if let Some(choices) = consent.get() {
                apply_consent_scripts(&choices);
            }
        });

        Effect::new(move |_| {
            let path = location.pathname.get();
            let _hash = location.hash.get();
            if let Some(ConsentChoices {
                analytics: true, ..
            }) = consent.get()
            {
                send_page_view(&path);
            }
        });

        install_error_reporter(consent);
    }

    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    {
        let _ = (consent, location);
    }

    view! {}
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn send_page_view(path: &str) {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    let Some(window) = web_sys::window() else {
        return;
    };
    let gtag = Reflect::get(window.as_ref(), &JsValue::from_str("gtag")).ok();
    let Some(gtag) = gtag.and_then(|f| f.dyn_into::<Function>().ok()) else {
        return;
    };
    let params = js_sys::Object::new();
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("page_path"),
        &JsValue::from_str(path),
    );
    let _ = gtag.call3(
        &JsValue::NULL,
        &JsValue::from_str("event"),
        &JsValue::from_str("page_view"),
        &params,
    );
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn install_error_reporter(consent: leptos::prelude::ReadSignal<Option<ConsentChoices>>) {
    use std::cell::Cell;
    use std::rc::Rc;
    use wasm_bindgen::{prelude::*, JsCast};

    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }

    if INSTALLED.with(|c| c.get()) {
        return;
    }
    INSTALLED.with(|c| c.set(true));

    let consent_error = Rc::new(consent);
    let consent_rejection = Rc::clone(&consent_error);
    let on_error = Closure::wrap(Box::new(move |event: web_sys::ErrorEvent| {
        if let Some(ConsentChoices {
            error_reporting: true,
            ..
        }) = consent_error.get()
        {
            let message = event.message();
            let filename = event.filename();
            let lineno = event.lineno();
            let _ = report_client_error(&message, &filename, lineno);
        }
    }) as Box<dyn FnMut(_)>);

    let on_rejection = Closure::wrap(Box::new(move |event: web_sys::PromiseRejectionEvent| {
        if let Some(ConsentChoices {
            error_reporting: true,
            ..
        }) = consent_rejection.get()
        {
            let reason = js_sys::JSON::stringify(&event.reason()).unwrap_or_default();
            let msg = reason
                .as_string()
                .unwrap_or_else(|| "unhandled rejection".into());
            let _ = report_client_error(&msg, "", 0);
        }
    }) as Box<dyn FnMut(_)>);

    if let Some(window) = web_sys::window() {
        let _ = window.add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback(
            "unhandledrejection",
            on_rejection.as_ref().unchecked_ref(),
        );
    }
    on_error.forget();
    on_rejection.forget();
}

/// Enable error reporting hooks after consent is granted mid-session.
#[cfg(any(feature = "csr", feature = "hydrate"))]
pub fn enable_error_reporting() {
    // Hooks are installed once; they check consent on each event.
}

#[cfg(not(any(feature = "csr", feature = "hydrate")))]
pub fn enable_error_reporting() {}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn report_client_error(
    message: &str,
    filename: &str,
    lineno: u32,
) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;
    use web_sys::{Request, RequestInit, RequestMode};

    let body = serde_json::json!({
        "message": scrub_client_text(message),
        "filename": scrub_client_text(filename),
        "lineno": lineno,
    });

    let opts = RequestInit::new();
    opts.set_method("POST");
    opts.set_mode(RequestMode::Cors);
    let headers = web_sys::Headers::new()?;
    headers.set("Content-Type", "application/json")?;
    headers.set("X-Consent", "error-reporting")?;
    opts.set_headers(&headers);
    opts.set_body(&JsValue::from_str(&body.to_string()));

    let request = Request::new_with_str_and_init("/api/telemetry/errors", &opts)?;
    let window = web_sys::window().ok_or(JsValue::UNDEFINED)?;
    let _ = window.fetch_with_request(&request);
    Ok(())
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn scrub_client_text(input: &str) -> String {
    let mut out = input.to_string();
    if let Some(q) = out.find('?') {
        out.truncate(q);
    }
    for pattern in ["@", "token=", "password=", "secret="] {
        if out.to_lowercase().contains(pattern) {
            return "[redacted]".into();
        }
    }
    if out.len() > 500 {
        out.truncate(500);
        out.push_str("…");
    }
    out
}
