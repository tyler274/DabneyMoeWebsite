//! Privacy policy page (`/privacy`).

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;

use crate::consent::OpenConsentButton;
use crate::sections::{Footer, NavBar, EMAIL};
use crate::AppShellMode;

#[component]
pub fn PrivacyPage() -> impl IntoView {
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);
    let is_demo_app = app_mode == AppShellMode::ClinicalDemoApp;

    let article_cls = if is_demo_app {
        "px-6 pt-8 pb-28 sm:pt-12"
    } else {
        "px-6 py-16 sm:py-20"
    };

    view! {
        <Title text="Privacy Policy — Dabney.moe & Android Clinical Demo | dabney.moe" />
        <Meta
            name="description"
            content="Privacy policy for dabney.moe and the zero-PHI Dabney.moe Clinical Demo Android app (moe.dabney.demo): zero personal health data collected, stored, or shared."
        />
        <Link rel="canonical" href="https://dabney.moe/privacy" />

        <NavBar />
        <article class=article_cls>
            <div class="prose prose-invert mx-auto max-w-3xl prose-headings:text-white prose-a:text-cyan-400">
                <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                    "Privacy Policy"
                </h1>
                <p class="text-slate-400">"Last updated: November 2026"</p>

                <section class="mt-8 rounded-2xl border border-emerald-400/35 bg-emerald-500/10 p-6 space-y-3 text-slate-200">
                    <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                        "Dabney.moe Clinical Demo Android App (moe.dabney.demo)"
                    </p>
                    <h2 class="text-xl font-semibold text-white">
                        "Zero PHI · Zero Data Collection · On-Device Only"
                    </h2>
                    <p class="text-sm leading-relaxed">
                        "The Dabney.moe Clinical Demo Android app ("
                        <code class="rounded bg-slate-950/70 px-1.5 py-0.5 text-emerald-200">"moe.dabney.demo"</code>
                        ") and the in-browser clinical demos at "
                        <A href="/clinical" attr:class="text-cyan-300 underline">"dabney.moe/clinical"</A>
                        " contain exclusively fictional, synthetic sample data. They do not collect, transmit, "
                        "store on any server, or share Protected Health Information (PHI) or personal user data."
                    </p>
                    <ul class="list-disc space-y-1.5 pl-6 text-sm text-slate-200">
                        <li>
                            <strong>"Do not enter real health information: "</strong>
                            "All patient names, MRNs, lab panels, CGM readings, and visit notes are fictional examples."
                        </li>
                        <li>
                            <strong>"No data collected or shared by the Android app: "</strong>
                            "The Android app runs offline in Rust/WebAssembly inside an on-device WebView. "
                            "It includes no analytics SDKs, no advertising SDKs, no crash-reporting uploads, "
                            "and no user accounts or cloud sync."
                        </li>
                        <li>
                            <strong>"Minimal permissions: "</strong>
                            "The Android demo app does not request Health Connect, location, camera, microphone, "
                            "contacts, phone, or background telemetry permissions."
                        </li>
                        <li>
                            <strong>"Not medical advice or a medical device: "</strong>
                            "This software is a technical and product demonstration only and is not intended for "
                            "diagnosis, treatment, clinical decision-making, or emergency use."
                        </li>
                    </ul>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Who we are"</h2>
                    <p>
                        "This site (dabney.moe) and the Dabney.moe Clinical Demo Android app are operated by Tyler Port. "
                        "For privacy questions or deletion inquiries contact "
                        <a href=format!("mailto:{EMAIL}") class="text-cyan-400 underline">{EMAIL}</a>"."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"What the website collects (dabney.moe web only)"</h2>
                    <h3 class="text-lg font-medium text-slate-200">"Server logs (legitimate interest)"</h3>
                    <p>
                        "Our web hosting provider and observability stack receive structured HTTP server logs: "
                        "HTTP method, path, status code, request duration, and an anonymous request ID. "
                        "We do not log cookies, authorization headers, clinical demo input, or query strings that may "
                        "contain personal data. Retention: 30 days."
                    </p>
                    <h3 class="text-lg font-medium text-slate-200">"Analytics (website consent required; disabled in Android app)"</h3>
                    <p>
                        "On the website, if you opt in, Google Analytics 4 records page views and basic device "
                        "information. IP addresses are anonymized. Retention: 2 months in GA4. "
                        "Analytics are completely disabled inside the Android demo app."
                    </p>
                    <h3 class="text-lg font-medium text-slate-200">"Error reporting (website consent required; disabled in Android app)"</h3>
                    <p>
                        "On the website, if you opt in, anonymous client error messages (no form data or personal "
                        "content) are sent to our server. Retention: 30 days. "
                        "Error reporting is completely disabled inside the Android demo app."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Website data processors"</h2>
                    <ul class="list-disc space-y-2 pl-6">
                        <li>"Cloud hosting infrastructure (web serving)"</li>
                        <li>"Google Analytics (website analytics only, when consented)"</li>
                        <li>"Grafana Labs / Grafana Cloud (website logs and dashboards)"</li>
                        <li>"Cloudflare (DNS)"</li>
                    </ul>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Your rights"</h2>
                    <p>
                        "Under GDPR and applicable privacy laws you may request access, correction, or deletion of personal data "
                        "we hold. Because website analytics and error data are pseudonymous—and the Android app collects no user "
                        "data at all—we may not hold any identifiable records for you. "
                        "Contact us at "
                        <a href=format!("mailto:{EMAIL}") class="text-cyan-400 underline">{EMAIL}</a>"."
                    </p>
                    {(!is_demo_app).then(|| view! {
                        <p>
                            <OpenConsentButton />
                        </p>
                    })}
                </section>

                {(!is_demo_app).then(|| view! {
                    <section class="mt-10 space-y-4 text-slate-300">
                        <h2 class="text-xl font-semibold text-white">"Cookies (website only)"</h2>
                        <p>
                            "Necessary cookies are not used for tracking. Optional analytics cookies "
                            "are set only after you consent. See "
                            <A href="/" attr:class="text-cyan-400 underline">"home"</A>
                            " and use the cookie banner to change your choices."
                        </p>
                    </section>
                })}
            </div>
        </article>
        <Footer />
    }
}
