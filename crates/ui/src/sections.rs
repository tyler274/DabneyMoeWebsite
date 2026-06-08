use leptos::prelude::*;

use crate::data::{
    ABOUT, ABOUT_PHOTO, EXPERIENCE, EXPERTISE, PROFILE_PHOTO, SERVICES, SKILLS, SUMMARY, TAGLINE,
};

/// On WASM builds, attempt to open `url` via the Tauri opener plugin, exposed
/// on `window.__TAURI__` (requires `app.withGlobalTauri = true`). Returns `true`
/// if the call was dispatched (so the caller can `prevent_default` on the
/// originating DOM event), `false` otherwise (e.g. running in a plain browser;
/// let the standard `href` behaviour take over).
#[cfg(not(feature = "ssr"))]
fn try_tauri_open(url: &str) -> bool {
    use js_sys::{Function, Object, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    let win = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let tauri =
        Reflect::get(win.as_ref(), &JsValue::from_str("__TAURI__")).unwrap_or(JsValue::UNDEFINED);
    if tauri.is_undefined() || tauri.is_null() {
        return false;
    }

    // Preferred path: the opener plugin's guest JS, `opener.openUrl(url)`.
    let opener = Reflect::get(&tauri, &JsValue::from_str("opener")).unwrap_or(JsValue::UNDEFINED);
    if !opener.is_undefined() && !opener.is_null() {
        let open_url =
            Reflect::get(&opener, &JsValue::from_str("openUrl")).unwrap_or(JsValue::UNDEFINED);
        if let Some(f) = open_url.dyn_ref::<Function>() {
            if f.call1(&opener, &JsValue::from_str(url)).is_ok() {
                return true;
            }
        }
    }

    // Fallback: the core IPC bridge, invoking the plugin command directly.
    let core = Reflect::get(&tauri, &JsValue::from_str("core")).unwrap_or(JsValue::UNDEFINED);
    if core.is_undefined() || core.is_null() {
        return false;
    }
    let invoke = Reflect::get(&core, &JsValue::from_str("invoke")).unwrap_or(JsValue::UNDEFINED);
    let invoke = match invoke.dyn_ref::<Function>() {
        Some(f) => f,
        None => return false,
    };
    let args = Object::new();
    let _ = Reflect::set(&args, &JsValue::from_str("path"), &JsValue::from_str(url));
    invoke
        .call2(
            &core,
            &JsValue::from_str("plugin:opener|open_url"),
            args.as_ref(),
        )
        .is_ok()
}

pub const EMAIL: &str = "tp@dabney.moe";
pub const GITHUB: &str = "https://github.com/tyler274";
pub const LINKEDIN: &str = "https://linkedin.com/in/tyler-port-9a795455";

/// Public, externally-resolvable resume URL. Used when handing the download
/// off to the system browser from the Tauri app, since bundled assets live at
/// the internal `tauri.localhost` origin that an external browser can't reach.
pub const RESUME_PUBLIC_URL: &str = "https://dabney.moe/resume.pdf";

#[component]
pub fn NavBar() -> impl IntoView {
    view! {
        <header class="sticky top-0 z-50 border-b border-white/5 bg-slate-950/70 backdrop-blur">
            <nav class="mx-auto flex max-w-5xl items-center justify-between px-6 py-4">
                <a href="#top" class="text-lg font-semibold tracking-tight text-white">
                    "Tyler Port"
                    <span class="text-cyan-400">"."</span>
                </a>
                <div class="hidden items-center gap-8 text-sm text-slate-300 md:flex">
                    <a href="#about" class="transition hover:text-white">"About"</a>
                    <a href="#expertise" class="transition hover:text-white">"Expertise"</a>
                    <a href="#services" class="transition hover:text-white">"Services"</a>
                    <a href="#experience" class="transition hover:text-white">"Experience"</a>
                    <a href="#skills" class="transition hover:text-white">"Skills"</a>
                    <a
                        href="#contact"
                        class="rounded-full bg-cyan-500 px-4 py-1.5 font-medium text-slate-950 transition hover:bg-cyan-400"
                    >
                        "Hire me"
                    </a>
                </div>
            </nav>
        </header>
    }
}

#[component]
pub fn Hero() -> impl IntoView {
    view! {
        <section
            id="top"
            class="relative overflow-hidden border-b border-white/5 px-6 py-24 sm:py-32"
        >
            <div class="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(60%_50%_at_50%_0%,rgba(34,211,238,0.18),transparent)]"></div>
            <div class="mx-auto grid max-w-5xl items-center gap-12 lg:grid-cols-[1fr_auto] lg:gap-16">
                <div>
                    <p class="mb-4 inline-flex items-center gap-2 rounded-full border border-cyan-400/30 bg-cyan-400/10 px-3 py-1 text-xs font-medium uppercase tracking-widest text-cyan-300">
                        "Available for freelance & contract work"
                    </p>
                    <h1 class="max-w-3xl text-4xl font-bold leading-tight tracking-tight text-white sm:text-6xl">
                        "Tyler Alamo Port"
                    </h1>
                    <p class="mt-4 text-xl font-medium text-cyan-300">{TAGLINE}</p>
                    <p class="mt-6 max-w-2xl text-lg leading-relaxed text-slate-300">{SUMMARY}</p>
                    <div class="mt-10 flex flex-wrap gap-4">
                        <a
                            href="#contact"
                            class="rounded-full bg-cyan-500 px-6 py-3 font-semibold text-slate-950 transition hover:bg-cyan-400"
                        >
                            "Start a project"
                        </a>
                        <a
                            href="/resume.pdf"
                            target="_blank"
                            rel="noopener"
                            on:click=move |_ev| {
                                // On Android WebViews, `target="_blank"` is silently
                                // swallowed. If Tauri's opener plugin is present, hand
                                // the URL off to the system browser. Bundled assets
                                // live at the internal `tauri.localhost` origin which
                                // an external browser can't resolve, so open the
                                // public URL instead.
                                #[cfg(not(feature = "ssr"))]
                                if try_tauri_open(RESUME_PUBLIC_URL) {
                                    _ev.prevent_default();
                                }
                            }
                            class="rounded-full border border-white/15 px-6 py-3 font-semibold text-white transition hover:border-white/40 hover:bg-white/5"
                        >
                            "Download résumé"
                        </a>
                    </div>
                </div>
                <div class="relative mx-auto w-full max-w-sm lg:max-w-none lg:w-80 xl:w-96">
                    <div class="absolute -inset-4 rounded-3xl bg-gradient-to-br from-cyan-400/20 to-transparent blur-2xl"></div>
                    <img
                        src=PROFILE_PHOTO
                        alt="Tyler Port standing on sand dunes"
                        class="relative aspect-[3/4] w-full rounded-2xl border border-white/10 object-cover shadow-2xl shadow-cyan-500/10"
                        loading="eager"
                    />
                </div>
            </div>
        </section>
    }
}

#[component]
pub fn About() -> impl IntoView {
    view! {
        <Section id="about" eyebrow="Who I am" title="About">
            <div class="grid items-center gap-10 lg:grid-cols-2 lg:gap-16">
                <div class="order-2 lg:order-1">
                    <p class="text-lg leading-relaxed text-slate-300">{ABOUT}</p>
                    <div class="mt-8 flex flex-wrap gap-3">
                        <span class="rounded-full border border-cyan-400/30 bg-cyan-400/10 px-3 py-1 text-xs font-medium text-cyan-300">
                            "Caltech CS '22"
                        </span>
                        <span class="rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-medium text-slate-300">
                            "GPU & HPC"
                        </span>
                        <span class="rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-medium text-slate-300">
                            "Rust & Systems"
                        </span>
                        <span class="rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-medium text-slate-300">
                            "Photography"
                        </span>
                    </div>
                </div>
                <div class="order-1 lg:order-2">
                    <img
                        src=ABOUT_PHOTO
                        alt="Tyler Port enjoying a traditional Japanese meal"
                        class="aspect-[4/3] w-full rounded-2xl border border-white/10 object-cover shadow-xl"
                        loading="lazy"
                    />
                </div>
            </div>
        </Section>
    }
}

#[component]
pub fn Expertise() -> impl IntoView {
    view! {
        <Section id="expertise" eyebrow="Where I shine" title="What I work with best">
            <div class="grid gap-8 sm:grid-cols-2">
                {EXPERTISE
                    .iter()
                    .map(|e| {
                        view! {
                            <div class="overflow-hidden rounded-2xl border border-white/10 bg-white/[0.02] transition hover:border-cyan-400/40 hover:bg-white/[0.04]">
                                <div class="flex h-44 items-center justify-center border-b border-white/5 bg-white/[0.03] p-6">
                                    <img
                                        src=e.image
                                        alt=e.image_alt
                                        class="max-h-full max-w-full object-contain"
                                        loading="lazy"
                                    />
                                </div>
                                <div class="p-6">
                                    <h3 class="text-lg font-semibold text-white">{e.title}</h3>
                                    <p class="mt-2 text-sm leading-relaxed text-slate-400">
                                        {e.blurb}
                                    </p>
                                </div>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </Section>
    }
}

#[component]
pub fn Services() -> impl IntoView {
    view! {
        <Section id="services" eyebrow="What I do" title="Services">
            <div class="grid gap-6 sm:grid-cols-2">
                {SERVICES
                    .iter()
                    .map(|s| {
                        view! {
                            <div class="group rounded-2xl border border-white/10 bg-white/[0.02] p-6 transition hover:border-cyan-400/40 hover:bg-white/[0.04]">
                                <div class="mb-4 flex h-11 w-11 items-center justify-center rounded-xl bg-cyan-400/10 text-xl text-cyan-300">
                                    {s.icon}
                                </div>
                                <h3 class="text-lg font-semibold text-white">{s.title}</h3>
                                <p class="mt-2 text-sm leading-relaxed text-slate-400">
                                    {s.blurb}
                                </p>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </Section>
    }
}

#[component]
pub fn Experience() -> impl IntoView {
    view! {
        <Section id="experience" eyebrow="Track record" title="Experience">
            <ol class="relative space-y-10 border-l border-white/10 pl-6">
                {EXPERIENCE
                    .iter()
                    .map(|r| {
                        view! {
                            <li class="relative">
                                <span class="absolute -left-[31px] top-1.5 h-3 w-3 rounded-full border-2 border-cyan-400 bg-slate-950"></span>
                                <div class="flex flex-wrap items-baseline justify-between gap-x-4">
                                    <h3 class="text-lg font-semibold text-white">
                                        {r.company}
                                        <span class="text-slate-500">" · "</span>
                                        <span class="text-slate-400">{r.location}</span>
                                    </h3>
                                    <span class="text-sm font-medium text-cyan-300">
                                        {r.period}
                                    </span>
                                </div>
                                <p class="mt-0.5 text-sm font-medium text-slate-300">{r.title}</p>
                                <ul class="mt-3 space-y-2">
                                    {r.points
                                        .iter()
                                        .map(|p| {
                                            view! {
                                                <li class="flex gap-3 text-sm leading-relaxed text-slate-400">
                                                    <span class="mt-2 h-1 w-1 shrink-0 rounded-full bg-cyan-400"></span>
                                                    <span>{*p}</span>
                                                </li>
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            </li>
                        }
                    })
                    .collect_view()}
            </ol>
        </Section>
    }
}

#[component]
pub fn Skills() -> impl IntoView {
    view! {
        <Section id="skills" eyebrow="Toolbox" title="Skills">
            <div class="grid gap-6 sm:grid-cols-3">
                {SKILLS
                    .iter()
                    .map(|g| {
                        view! {
                            <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                                <h3 class="mb-4 text-sm font-semibold uppercase tracking-widest text-cyan-300">
                                    {g.label}
                                </h3>
                                <ul class="flex flex-wrap gap-2">
                                    {g.items
                                        .iter()
                                        .map(|item| {
                                            view! {
                                                <li class="rounded-lg bg-white/5 px-2.5 py-1 text-xs text-slate-300">
                                                    {*item}
                                                </li>
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </Section>
    }
}

#[component]
pub fn Contact() -> impl IntoView {
    let mailto = format!("mailto:{EMAIL}");
    view! {
        <Section id="contact" eyebrow="Get in touch" title="Let's build something">
            <div class="rounded-2xl border border-white/10 bg-gradient-to-br from-cyan-500/10 to-transparent p-8 sm:p-10">
                <p class="max-w-2xl text-lg leading-relaxed text-slate-300">
                    "Have a project that needs systems expertise, GPU performance, or a "
                    "full-stack build? I take on freelance and contract work - let's talk."
                </p>
                <div class="mt-8 flex flex-wrap gap-4">
                    <a
                        href=mailto
                        class="rounded-full bg-cyan-500 px-6 py-3 font-semibold text-slate-950 transition hover:bg-cyan-400"
                    >
                        {EMAIL}
                    </a>
                    <a
                        href=GITHUB
                        target="_blank"
                        rel="noopener"
                        class="rounded-full border border-white/15 px-6 py-3 font-semibold text-white transition hover:border-white/40 hover:bg-white/5"
                    >
                        "GitHub"
                    </a>
                    <a
                        href=LINKEDIN
                        target="_blank"
                        rel="noopener"
                        class="rounded-full border border-white/15 px-6 py-3 font-semibold text-white transition hover:border-white/40 hover:bg-white/5"
                    >
                        "LinkedIn"
                    </a>
                </div>
            </div>
        </Section>
    }
}

#[component]
pub fn Footer() -> impl IntoView {
    view! {
        <footer class="border-t border-white/5 px-6 py-10">
            <div class="mx-auto flex max-w-5xl flex-col items-center gap-2 text-center text-sm text-slate-500">
                <div class="flex flex-col items-center gap-2 sm:flex-row sm:justify-between sm:gap-4 w-full">
                    <span>"© 2026 Tyler Port · dabney.moe"</span>
                    <span>"Built with Rust, Leptos & Tauri"</span>
                </div>
                <span class="text-xs text-slate-600">"Icon © HoYoverse. All Rights Reserved."</span>
            </div>
        </footer>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod render_tests {
    // `super::*` re-exports the section components, the contact constants,
    // the data tables, and the leptos prelude already imported above.
    use super::*;

    /// Render a component to an HTML string inside a fresh reactive owner,
    /// matching how the SSR server materializes the same components. The
    /// common HTML entities are decoded back so assertions can compare
    /// against the original (unescaped) source text.
    fn render(view: impl RenderHtml + 'static) -> String {
        let owner = Owner::new();
        let html = owner.with(|| view.to_html());
        drop(owner);
        // Decode `&amp;` last so already-decoded ampersands aren't re-expanded.
        html.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    #[test]
    fn about_renders_copy_and_photo() {
        let html = render(view! { <About /> });
        assert!(html.contains(ABOUT), "about missing copy");
        assert!(
            html.contains(ABOUT_PHOTO),
            "about missing photo: {}",
            ABOUT_PHOTO
        );
    }

    #[test]
    fn expertise_renders_every_area() {
        let html = render(view! { <Expertise /> });
        for e in EXPERTISE {
            assert!(
                html.contains(e.title),
                "expertise missing title: {}",
                e.title
            );
            assert!(
                html.contains(e.image),
                "expertise missing image: {}",
                e.title
            );
        }
    }

    #[test]
    fn hero_renders_profile_photo() {
        let html = render(view! { <Hero /> });
        assert!(
            html.contains(PROFILE_PHOTO),
            "hero missing profile photo: {}",
            PROFILE_PHOTO
        );
    }

    #[test]
    fn hero_renders_tagline_and_summary() {
        let html = render(view! { <Hero /> });
        assert!(html.contains("Tyler Alamo Port"), "hero missing name");
        assert!(html.contains(TAGLINE), "hero missing tagline");
        assert!(html.contains(SUMMARY), "hero missing summary");
    }

    #[test]
    fn services_render_every_entry() {
        let html = render(view! { <Services /> });
        for s in SERVICES {
            assert!(
                html.contains(s.title),
                "services missing title: {}",
                s.title
            );
        }
    }

    #[test]
    fn experience_renders_every_role() {
        let html = render(view! { <Experience /> });
        for r in EXPERIENCE {
            assert!(
                html.contains(r.company),
                "experience missing company: {}",
                r.company
            );
        }
    }

    #[test]
    fn skills_render_every_group() {
        let html = render(view! { <Skills /> });
        for g in SKILLS {
            assert!(html.contains(g.label), "skills missing group: {}", g.label);
        }
    }

    #[test]
    fn contact_renders_links() {
        let html = render(view! { <Contact /> });
        assert!(html.contains(EMAIL), "contact missing email");
        assert!(html.contains(GITHUB), "contact missing github link");
        assert!(html.contains(LINKEDIN), "contact missing linkedin link");
    }
}

/// Shared section wrapper: consistent spacing, eyebrow label, and heading.
#[component]
fn Section(
    id: &'static str,
    eyebrow: &'static str,
    title: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <section id=id class="border-b border-white/5 px-6 py-20">
            <div class="mx-auto max-w-5xl">
                <p class="mb-2 text-sm font-semibold uppercase tracking-widest text-cyan-400">
                    {eyebrow}
                </p>
                <h2 class="mb-10 text-3xl font-bold tracking-tight text-white sm:text-4xl">
                    {title}
                </h2>
                {children()}
            </div>
        </section>
    }
}
