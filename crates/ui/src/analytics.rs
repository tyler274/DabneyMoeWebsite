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

/// Name of the `<meta>` tag that carries the GA4 measurement ID to the client.
/// The tag itself makes no request; the client loads Google's tag only after
/// the visitor opts in to analytics.
pub const GA4_META_NAME: &str = "ga4-measurement-id";

/// Whether `id` has the shape of a GA4 measurement ID (`G-` followed by
/// uppercase letters and digits). Anything else is never sent to Google.
pub fn is_valid_measurement_id(id: &str) -> bool {
    id.strip_prefix("G-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    })
}

/// Consent Mode v2 defaults injected in the document head.
///
/// The head never loads Google's tag: with no opt-in there must be zero
/// requests to Google. It only defines a local `gtag` queue with everything
/// denied and publishes the measurement ID for the client-side loader
/// (`sync_ga4_with_consent`).
#[component]
pub fn AnalyticsHead() -> impl IntoView {
    view! { <AnalyticsHeadFor measurement_id=ga4_measurement_id() /> }
}

#[component]
fn AnalyticsHeadFor(measurement_id: Option<String>) -> impl IntoView {
    let measurement_id = measurement_id.filter(|id| is_valid_measurement_id(id));

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
        {measurement_id.map(|id| view! { <meta name=GA4_META_NAME content=id /> })}
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

    // Invisible anchor so the component has a stable view tree (no empty `view! {}` / `()`).
    view! { <span class="hidden" aria-hidden="true"></span> }
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
const GA4_SCRIPT_ID: &str = "ga4-gtag-loader";

/// Brings Google's tag in line with the visitor's analytics choice.
///
/// Opting in loads `gtag.js` once and configures it. Opting out (or never
/// loading it) sets Google's documented `ga-disable-<ID>` switch, which stops
/// every hit for the ID, including the cookieless pings Consent Mode would
/// otherwise still send after a mid-session revocation.
#[cfg(any(feature = "csr", feature = "hydrate"))]
pub(crate) fn sync_ga4_with_consent(analytics: bool) {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(id) = document
        .query_selector(&format!("meta[name=\"{GA4_META_NAME}\"]"))
        .ok()
        .flatten()
        .and_then(|meta| meta.get_attribute("content"))
        .filter(|id| is_valid_measurement_id(id))
    else {
        return;
    };

    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(&format!("ga-disable-{id}")),
        &JsValue::from_bool(!analytics),
    );
    if !analytics || document.get_element_by_id(GA4_SCRIPT_ID).is_some() {
        return;
    }

    let Some(gtag) = Reflect::get(window.as_ref(), &JsValue::from_str("gtag"))
        .ok()
        .and_then(|f| f.dyn_into::<Function>().ok())
    else {
        return;
    };
    let config = js_sys::Object::new();
    for (key, value) in [
        ("anonymize_ip", true),
        ("allow_google_signals", false),
        ("allow_ad_personalization_signals", false),
        // AnalyticsRuntime sends page views itself, once per route.
        ("send_page_view", false),
    ] {
        let _ = Reflect::set(&config, &JsValue::from_str(key), &JsValue::from_bool(value));
    }
    let _ = gtag.call2(
        &JsValue::NULL,
        &JsValue::from_str("js"),
        &js_sys::Date::new_0(),
    );
    let _ = gtag.call3(
        &JsValue::NULL,
        &JsValue::from_str("config"),
        &JsValue::from_str(&id),
        &config,
    );

    let Ok(script) = document.create_element("script") else {
        return;
    };
    script.set_id(GA4_SCRIPT_ID);
    let _ = script.set_attribute("async", "");
    let _ = script.set_attribute(
        "src",
        &format!("https://www.googletagmanager.com/gtag/js?id={id}"),
    );
    if let Some(head) = document.query_selector("head").ok().flatten() {
        let _ = head.append_child(&script);
    }
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
fn install_error_reporter(consent: leptos::prelude::Memo<Option<ConsentChoices>>) {
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
        out.push('…');
    }
    out
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn render_head(measurement_id: Option<&str>) -> String {
        let owner = Owner::new();
        let id = measurement_id.map(str::to_string);
        let html = owner.with(|| view! { <AnalyticsHeadFor measurement_id=id /> }.to_html());
        drop(owner);
        html
    }

    #[test]
    fn head_never_loads_google_before_opt_in() {
        let html = render_head(Some("G-TEST123"));
        for forbidden in [
            "googletagmanager.com",
            "google-analytics.com",
            "gtag('config'",
            "gtag('js'",
            "<script async",
            "src=",
        ] {
            assert!(
                !html.contains(forbidden),
                "consent-less head must not contain {forbidden:?}: {html}"
            );
        }
        assert!(html.contains("analytics_storage: 'denied'"));
        assert!(html.contains(r#"<meta name="ga4-measurement-id" content="G-TEST123""#));
    }

    #[test]
    fn head_omits_invalid_or_missing_measurement_ids() {
        for id in [
            None,
            Some(""),
            Some("G-"),
            Some("UA-1234"),
            Some("G-abc\"><script>"),
        ] {
            let html = render_head(id);
            assert!(!html.contains(GA4_META_NAME), "{id:?}: {html}");
        }
    }

    #[test]
    fn measurement_id_shape_is_checked() {
        assert!(is_valid_measurement_id("G-AB12CD34EF"));
        assert!(!is_valid_measurement_id("G-ab12"));
        assert!(!is_valid_measurement_id("AW-123"));
        assert!(!is_valid_measurement_id("G-12 34"));
    }
}
