//! Shared Leptos UI for dabney.moe.
#![recursion_limit = "1024"]
//!
//! The same components render in three configurations selected by feature:
//! - `ssr`     : server-side rendering on the Axum web server.
//! - `hydrate` : browser hydration of the server-rendered HTML.
//! - `csr`     : pure client-side rendering for the Tauri (Trunk) build.

pub mod analytics;
pub mod android_download;
pub mod clinical;
pub mod commissions;
pub mod commissions_data;
pub mod consent;
pub mod creators;
pub mod data;
pub mod health_bridge;
pub mod local_ai_chat;
pub mod mental_health;
pub mod mental_health_3d;
pub mod mental_health_cgm;
pub mod mental_health_cgm_alerts;
pub mod mental_health_cgm_alerts_view;
pub mod mental_health_cgm_analytics;
pub mod mental_health_cgm_views;
pub mod mental_health_episodes;
pub mod mental_health_exports;
pub mod mental_health_lingo;
pub mod mental_health_support;
pub mod ownership;
pub mod patient_demo;
pub mod portfolio;
pub mod portfolio_chart;
pub mod privacy;
pub mod sections;
pub mod tools;

use leptos::prelude::*;
use leptos_meta::{provide_meta_context, HashedStylesheet, Link, Meta, MetaTags, Title};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::StaticSegment;

use analytics::{AnalyticsHead, AnalyticsRuntime};
use commissions::{ArtworkPage, LapidaryPage, SongsPage};
use consent::{provide_consent_context, ConsentBanner};
use privacy::PrivacyPage;
use tools::InvestmentAccountPage;

use sections::{
    About, Contact, Experience, Expertise, Footer, Hero, NavBar, Projects, Services, Skills,
};

/// Presentation mode shared between the full dabney.moe website and the
/// standalone zero-PHI mobile/desktop clinical demo app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppShellMode {
    /// Full dabney.moe web experience (résumé homepage at `/`, optional web consent/analytics).
    Website,
    /// Standalone Dabney.moe Clinical & Patient Demo app: lands directly on the
    /// synthetic zero-PHI Patient Demo, uses mobile-first clinical suite chrome,
    /// and disables cookies/analytics/telemetry completely.
    ClinicalDemoApp,
}

/// Server-rendered HTML document shell. Used only by the SSR build; the Tauri
/// CSR build supplies its own `index.html`.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en" class="scroll-smooth">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HashedStylesheet options=options.clone() id="leptos" />
                <HydrationScripts options />
                <MetaTags />
                <AnalyticsHead />
            </head>
            <body class="bg-slate-950 font-sans text-slate-200 antialiased">
                <App />
            </body>
        </html>
    }
}

/// Root application component for the dabney.moe web server and browser hydration.
#[component]
pub fn App() -> impl IntoView {
    view! { <AppWithMode mode=AppShellMode::Website /> }
}

/// Root application component parameterized by [`AppShellMode`].
#[component]
pub fn AppWithMode(mode: AppShellMode) -> impl IntoView {
    provide_meta_context();
    provide_consent_context();
    provide_context(mode);

    let is_demo_app = mode == AppShellMode::ClinicalDemoApp;

    view! {
        <Link rel="icon" href="/favicon.svg" type_="image/svg+xml" />
        <Link rel="icon" href="/favicon.png" type_="image/png" />

        {(!is_demo_app).then(|| view! { <ConsentBanner /> })}

        <Router>
            {(!is_demo_app).then(|| view! {
                <PageMetadata />
                <AnalyticsRuntime />
            })}
            <main>
                <Routes fallback=move || {
                    if is_demo_app {
                        view! { <patient_demo::PatientDemoPage /> }.into_any()
                    } else {
                        view! { <HomePage /> }.into_any()
                    }
                }>
                    <Route
                        path=StaticSegment("")
                        view=move || {
                            if is_demo_app {
                                view! { <patient_demo::PatientDemoPage /> }.into_any()
                            } else {
                                view! { <HomePage /> }.into_any()
                            }
                        }
                    />
                    <Route path=StaticSegment("privacy") view=PrivacyPage />
                    <Route path=StaticSegment("android") view=android_download::AndroidDownloadPage />
                    <Route path=StaticSegment("clinical") view=clinical::ClinicalOverviewPage />
                    <Route
                        path=(StaticSegment("clinical"), StaticSegment("patient"))
                        view=patient_demo::PatientDemoPage
                    />
                    <Route
                        path=(StaticSegment("clinical"), StaticSegment("physician"))
                        view=clinical::PhysicianWorkspacePage
                    />
                    <Route path=StaticSegment("mental-health") view=mental_health::MentalHealthPage />
                    <Route path=StaticSegment("ownership") view=ownership::OwnershipReportPage />
                    <Route path=StaticSegment("creators") view=creators::CreatorsPage />
                    <Route
                        path=(StaticSegment("tools"), StaticSegment("investment-account"))
                        view=InvestmentAccountPage
                    />
                    <Route
                        path=(StaticSegment("tools"), StaticSegment("investment-account-sell"))
                        view=InvestmentAccountPage
                    />
                    <Route
                        path=(StaticSegment("commissions"), StaticSegment("lapidary"))
                        view=LapidaryPage
                    />
                    <Route
                        path=(StaticSegment("commissions"), StaticSegment("artwork"))
                        view=ArtworkPage
                    />
                    <Route
                        path=(StaticSegment("commissions"), StaticSegment("songs"))
                        view=SongsPage
                    />
                </Routes>
            </main>
        </Router>
    }
}

/// Keep the existing site defaults everywhere except the creator directory.
/// Selecting one metadata owner avoids duplicate social tags and owner portraits
/// leaking into creator previews, including after client-side navigation.
#[component]
fn PageMetadata() -> impl IntoView {
    let location = use_location();
    view! {
        <Show
            when=move || location.pathname.get().trim_end_matches('/') == "/creators"
            fallback=OwnerMetadata
        >
            <creators::CreatorsMetadata />
        </Show>
    }
}

#[component]
fn OwnerMetadata() -> impl IntoView {
    view! {
        <Title text="Tyler Port - Freelance Software Engineer" />
        <Meta
            name="description"
            content="Tyler Port is a Caltech-trained freelance software engineer specializing in Rust, GPU computing, and high-performance systems. Available for contract work."
        />
        <Meta name="author" content="Tyler Alamo Port" />
        <Link rel="canonical" href="https://dabney.moe/" />
        <Meta property="og:type" content="website" />
        <Meta property="og:title" content="Tyler Port - Freelance Software Engineer" />
        <Meta
            property="og:description"
            content="Rust, GPU computing, and high-performance systems. Available for freelance and contract work."
        />
        <Meta property="og:url" content="https://dabney.moe/" />
        <Meta property="og:site_name" content="dabney.moe" />
        <Meta property="og:image" content="https://dabney.moe/photos/tyler-dunes.jpg" />
        <Meta name="twitter:card" content="summary_large_image" />
        <Meta name="twitter:image" content="https://dabney.moe/photos/tyler-dunes.jpg" />
        <Meta name="twitter:title" content="Tyler Port - Freelance Software Engineer" />
        <Meta
            name="twitter:description"
            content="Rust, GPU computing, and high-performance systems. Available for freelance and contract work."
        />
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! {
        <HomeHashScroll />
        <NavBar />
        <Hero />
        <About />
        <Expertise />
        <Services />
        <Projects />
        <Experience />
        <Skills />
        <Contact />
        <Footer />
    }
}

/// When navigating from another route to `/#section`, the browser tries to
/// scroll before home sections exist. Re-scroll after the home page mounts.
#[component]
#[allow(clippy::unused_unit)]
fn HomeHashScroll() -> impl IntoView {
    let location = use_location();

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    Effect::new(move |_| {
        let hash = location.hash.get();
        if hash.len() > 1 {
            schedule_scroll_to_hash(&hash);
        }
    });

    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    let _ = location;

    view! {}
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn schedule_scroll_to_hash(hash: &str) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let id = hash.trim_start_matches('#').to_string();

    let scroll = move || {
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(&id))
            .map(|element| element.scroll_into_view())
            .is_some()
    };

    if scroll() {
        return;
    }

    let closure = Closure::once(Box::new(move || {
        scroll();
    }) as Box<dyn FnMut()>);

    if let Some(window) = web_sys::window() {
        let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
    }
    closure.forget();
}
