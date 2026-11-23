//! Tyler Port's résumé in a VTuber debut-card layout at `/vtuber`.
//!
//! The résumé itself is stated once, in [`crate::data`]; this page lays the
//! same experience, skills and projects out under different headings.

use leptos::prelude::*;
use leptos_meta::Title;

use crate::data::{ALMA_MATER, TAGLINE};
use crate::sections::{ExperienceTimeline, Footer, NavBar, ProjectCards, SkillGroups};

/// Where the self-hosted Ayagami model viewer is served. `crates/web` mounts
/// the static build from `public/vtuber/demo` at this path.
pub const DEMO_HREF: &str = "/vtuber/demo/";
const DEMO_NOTICE_HREF: &str = "/vtuber/demo/NOTICE.txt";
pub const AYAGAMI_REPOSITORY: &str = "https://github.com/AyagamiDev/ayagami";

const HEADING: &str = "text-2xl font-semibold text-white";
const LINK: &str = "text-cyan-400 underline";

#[component]
pub fn VtuberPage() -> impl IntoView {
    view! {
        <Title text="Tyler Port — VTuber-style Résumé | dabney.moe" />
        <NavBar />
        <article class="mx-auto max-w-5xl px-6 py-20 text-slate-300">
            <header class="space-y-6 rounded-2xl border border-cyan-400/30 bg-gradient-to-br from-cyan-500/10 to-transparent p-8 sm:p-10">
                <p class="text-sm font-semibold uppercase tracking-widest text-cyan-400">"Debut card"</p>
                <h1 class="text-4xl font-bold tracking-tight text-white sm:text-6xl">"Tyler Port"</h1>
                <p class="text-xl font-medium text-cyan-300">{TAGLINE}</p>
                <p>
                    <span class="rounded-full border border-cyan-400/30 bg-cyan-400/10 px-3 py-1 text-xs font-medium text-cyan-300">
                        {ALMA_MATER}
                    </span>
                </p>
                <p class="text-sm text-slate-400">
                    "This is Tyler Port's real résumé in a VTuber-style presentation. There is no separate character and no avatar yet."
                </p>
                <div class="flex flex-wrap gap-6">
                    <a
                        class=LINK
                        href="/resume.pdf"
                        target="_blank"
                        rel="noopener"
                        on:click=move |_ev| {
                            #[cfg(any(feature = "csr", feature = "hydrate"))]
                            if crate::sections::try_tauri_open(crate::sections::RESUME_PUBLIC_URL) {
                                _ev.prevent_default();
                            }
                        }
                    >
                        "Download résumé (PDF)"
                    </a>
                    <a class=LINK href="/#contact">"Contact"</a>
                </div>
            </header>

            <section class="mt-12 space-y-6" aria-labelledby="lore-heading">
                <h2 id="lore-heading" class=HEADING>"Lore"</h2>
                <ExperienceTimeline />
            </section>

            <section class="mt-12 space-y-6" aria-labelledby="skills-heading">
                <h2 id="skills-heading" class=HEADING>"Skills"</h2>
                <SkillGroups />
            </section>

            <section class="mt-12 space-y-6" aria-labelledby="projects-heading">
                <h2 id="projects-heading" class=HEADING>"Projects"</h2>
                <ProjectCards />
            </section>

            <section class="mt-12 space-y-4 leading-relaxed" aria-labelledby="demo-heading">
                <h2 id="demo-heading" class=HEADING>"Model viewer demo"</h2>
                <div class="space-y-4 rounded-xl border border-slate-800 bg-slate-900 p-6">
                    <p>
                        "A 2D puppet model viewer and poser that runs in your browser. It ships with no model: open a model archive of your own and the viewer reads it locally on your device."
                    </p>
                    <p>
                        // `rel="external"` keeps the Leptos router from treating
                        // this static app as one of its own routes.
                        <a
                            class="inline-block rounded-full bg-cyan-500 px-6 py-3 font-semibold text-slate-950 transition hover:bg-cyan-400"
                            href=DEMO_HREF
                            rel="external"
                            on:click=move |_ev| {
                                #[cfg(any(feature = "csr", feature = "hydrate"))]
                                if crate::sections::open_site_or_external(DEMO_HREF) {
                                    _ev.prevent_default();
                                }
                            }
                        >
                            "Open the model viewer"
                        </a>
                    </p>
                    <p class="text-sm text-slate-400">
                        "Powered by "
                        <a class=LINK href=AYAGAMI_REPOSITORY target="_blank" rel="noopener">"Ayagami"</a>
                        ", an open-source 2D puppet renderer written in Rust, © The Ayagami Project Contributors, used under the MIT or Apache-2.0 licence. The viewer is an unmodified build of its demo app, pinned to one upstream commit: "
                        <a
                            class=LINK
                            href=DEMO_NOTICE_HREF
                            rel="external"
                            on:click=move |_ev| {
                                #[cfg(any(feature = "csr", feature = "hydrate"))]
                                if crate::sections::open_site_or_external(DEMO_NOTICE_HREF) {
                                    _ev.prevent_default();
                                }
                            }
                        >
                            "build details and licence notices"
                        </a>
                        "."
                    </p>
                    <p class="text-sm text-slate-400">
                        "The viewer is compatible with the Live2D model format. It is not affiliated with or endorsed by Live2D Inc.; Live2D is a trademark of Live2D Inc., named here only to describe file-format compatibility."
                    </p>
                </div>
            </section>
        </article>
        <Footer />
    }
}
