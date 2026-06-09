//! Privacy policy page (`/privacy`).

use leptos::prelude::*;
use leptos_router::components::A;

use crate::consent::OpenConsentButton;
use crate::sections::{Footer, NavBar, EMAIL};

#[component]
pub fn PrivacyPage() -> impl IntoView {
    view! {
        <NavBar />
        <article class="px-6 py-20">
            <div class="prose prose-invert mx-auto max-w-3xl prose-headings:text-white prose-a:text-cyan-400">
                <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                    "Privacy Policy"
                </h1>
                <p class="text-slate-400">"Last updated: June 2026"</p>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Who we are"</h2>
                    <p>
                        "This site (dabney.moe) is operated by Tyler Port. "
                        "For privacy questions contact "
                        <a href=format!("mailto:{EMAIL}") class="text-cyan-400 underline">{EMAIL}</a>"."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"What we collect"</h2>
                    <h3 class="text-lg font-medium text-slate-200">"Server logs (legitimate interest)"</h3>
                    <p>
                        "Our hosting provider (Google Cloud Run) and our observability stack "
                        "(Grafana Cloud, EU region) receive structured server logs: HTTP method, "
                        "path, status code, request duration, and an anonymous request ID. "
                        "We do not log cookies, authorization headers, or query strings that may "
                        "contain personal data. Retention: 30 days."
                    </p>
                    <h3 class="text-lg font-medium text-slate-200">"Analytics (consent required)"</h3>
                    <p>
                        "If you opt in, Google Analytics 4 records page views and basic device "
                        "information. Data is processed by Google and may be exported to BigQuery "
                        "in the EU for reporting. IP addresses are anonymized. Retention: 2 months "
                        "in GA4."
                    </p>
                    <h3 class="text-lg font-medium text-slate-200">"Error reporting (consent required)"</h3>
                    <p>
                        "If you opt in, anonymous client error messages (no form data or personal "
                        "content) are sent to our server and Grafana Cloud Loki (EU). Retention: 30 days."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Data processors"</h2>
                    <ul class="list-disc space-y-2 pl-6">
                        <li>"Google Cloud Platform (hosting, BigQuery)"</li>
                        <li>"Google Analytics (analytics, when consented)"</li>
                        <li>"Grafana Labs / Grafana Cloud (logs and dashboards, EU)"</li>
                        <li>"Cloudflare (DNS)"</li>
                    </ul>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Your rights"</h2>
                    <p>
                        "Under GDPR you may request access, correction, or deletion of personal data "
                        "we hold. Because analytics and error data are pseudonymous, we may not be able "
                        "to identify your records without additional information you provide. "
                        "Contact us at "
                        <a href=format!("mailto:{EMAIL}") class="text-cyan-400 underline">{EMAIL}</a>"."
                    </p>
                    <p>
                        <OpenConsentButton />
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Cookies"</h2>
                    <p>
                        "Necessary cookies are not used for tracking. Optional analytics cookies "
                        "are set only after you consent. See "
                        <A href="/" attr:class="text-cyan-400 underline">"home"</A>
                        " and use the cookie banner to change your choices."
                    </p>
                </section>
            </div>
        </article>
        <Footer />
    }
}
