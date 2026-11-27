//! App distribution hub for Dabney.moe Clinical Demo (`moe.dabney.demo`) at `/android`:
//! live self-hosted F-Droid repository (Codeberg HTTPS), one-tap `fdroidrepos://` link
//! & scannable QR code, direct signed universal APK with SHA-256 verification, Obtainium
//! auto-update config, and zero-PHI / FOSS transparency audit.

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;

use crate::sections::{Footer, NavBar};

pub const ANDROID_APP_PACKAGE: &str = "moe.dabney.demo";
pub const ANDROID_APP_VERSION: &str = "1.0.0";
pub const ANDROID_APP_VERSION_CODE: u32 = 1;
pub const FDROID_REPO_URL: &str = "https://codeberg.org/tyler274/fdroid/raw/branch/pages/repo";
pub const FDROID_REPO_FINGERPRINT: &str =
    "BBA10E130E00DC452ED1B1C33D8D6074A098A5874F5DE3AE1390171550EDA5B7";
pub const FDROID_REPO_URI: &str =
    "fdroidrepos://codeberg.org/tyler274/fdroid/raw/branch/pages/repo?fingerprint=BBA10E130E00DC452ED1B1C33D8D6074A098A5874F5DE3AE1390171550EDA5B7";
pub const DIRECT_APK_URL: &str =
    "https://codeberg.org/tyler274/fdroid/raw/branch/pages/repo/moe.dabney.demo_1.apk";
pub const APK_SHA256: &str = "f5b2b494a5873f4eda33e93efb6e44242fa9aeac9165c1d37894097477f742d6";
pub const SIGNING_CERT_SHA256: &str =
    "C2:CB:53:EE:B4:36:78:50:CF:E9:FC:14:4E:97:FF:CC:7F:8D:23:AC:41:BF:73:9F:27:0E:79:17:22:99:66:C6";
pub const CODEBERG_SOURCE_URL: &str = "https://codeberg.org/tyler274/dabney-moe-website";
pub const GITHUB_RELEASE_URL: &str =
    "https://github.com/tyler274/DabneyMoeWebsite/releases/tag/v1.0.0";
pub const IZZYONDROID_ISSUE_URL: &str = "https://codeberg.org/IzzyOnDroid/repodata/issues/966";
pub const FDROID_MR_URL: &str = "https://gitlab.com/fdroid/fdroiddata/-/merge_requests/59140";

#[component]
pub fn AndroidDownloadPage() -> impl IntoView {
    view! {
        <Title text="Install Android App (F-Droid & Direct APK) — Dabney.moe Clinical Demo" />
        <Meta
            name="description"
            content="Install Dabney.moe Clinical Demo (moe.dabney.demo) on Android via our live cryptographically signed F-Droid repository, Obtainium, or direct signed universal APK. 100% synthetic zero-PHI, MIT open source, zero trackers."
        />
        <Link rel="canonical" href="https://dabney.moe/android" />

        <NavBar />

        <div class="mx-auto max-w-5xl px-4 py-8 sm:px-6 sm:py-12 space-y-8">
            // Hero
            <section class="relative overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-br from-slate-900/90 via-slate-950 to-slate-950 p-7 sm:p-10">
                <div class="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(60%_50%_at_80%_0%,rgba(16,185,129,0.16),transparent)]"></div>
                <div class="flex flex-col gap-6 sm:flex-row sm:items-start sm:justify-between">
                    <div class="max-w-2xl space-y-4">
                        <div class="inline-flex items-center gap-3">
                            <div class="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-[#01081b] p-1 shadow-sm">
                                <img
                                    src="/dabney-elephant.svg"
                                    alt="Dabney.moe minimalist crest icon"
                                    class="h-full w-full object-contain"
                                />
                            </div>
                            <div>
                                <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                                    {format!("Android · {ANDROID_APP_PACKAGE} · v{ANDROID_APP_VERSION} (Build {ANDROID_APP_VERSION_CODE})")}
                                </p>
                                <h1 class="text-2xl font-bold tracking-tight text-white sm:text-3xl">
                                    "Dabney.moe Clinical Demo for Android"
                                </h1>
                            </div>
                        </div>
                        <p class="text-sm leading-relaxed text-slate-300 sm:text-base">
                            "Run the complete offline Patient Experience Demo, Physician Workspace Demo, "
                            "and Mental Health & Commercial CGM analytics natively on Android—compiled "
                            "from the exact same pure Rust & WebAssembly codebase powering "
                            <a href="https://dabney.moe/clinical" class="text-cyan-400 underline hover:text-cyan-300">
                                "dabney.moe/clinical"
                            </a>
                            "."
                        </p>
                        <div class="flex flex-wrap gap-2 text-[11px] font-semibold">
                            <span class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-3 py-1 text-emerald-200">
                                "100% Synthetic · Zero PHI"
                            </span>
                            <span class="rounded-full border border-cyan-400/35 bg-cyan-500/10 px-3 py-1 text-cyan-200">
                                "MIT Open Source · Zero Trackers"
                            </span>
                            <span class="rounded-full border border-white/15 bg-white/5 px-3 py-1 text-slate-200">
                                "arm64-v8a + x86_64 · 16 KB ELF Aligned"
                            </span>
                        </div>
                    </div>
                </div>
            </section>

            // Primary Install Grid: F-Droid Repo + Direct Signed APK
            <div class="grid gap-6 lg:grid-cols-2">
                // Card 1: Self-Hosted F-Droid Repo (Live Today)
                <section class="rounded-2xl border border-emerald-400/35 bg-slate-900/60 p-6 space-y-4">
                    <div class="flex items-center justify-between gap-2">
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "Recommended · Auto-Updates Live Today"
                        </p>
                        <span class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-2.5 py-0.5 text-[10px] font-semibold uppercase text-emerald-200">
                            "Signed F-Droid Repo"
                        </span>
                    </div>
                    <h2 class="text-xl font-bold text-white">
                        "Install via F-Droid, Droid-ify, or Neo Store"
                    </h2>
                    <p class="text-sm text-slate-300">
                        "Add the cryptographically signed Dabney.moe F-Droid repository on your Android phone "
                        "to install and receive signed updates automatically without Google Play."
                    </p>
                    <div class="flex flex-wrap items-center gap-3">
                        <a
                            href=FDROID_REPO_URI
                            class="inline-flex min-h-[44px] items-center rounded-full bg-emerald-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                        >
                            "Add Repository in F-Droid ↗"
                        </a>
                        <a
                            href=FDROID_REPO_URL
                            target="_blank"
                            rel="noopener noreferrer"
                            class="inline-flex min-h-[44px] items-center rounded-full border border-white/15 px-4 py-2 text-xs font-semibold text-slate-200 transition hover:bg-white/5"
                        >
                            "Browse Repo Index"
                        </a>
                    </div>
                    <div class="flex flex-col items-start gap-4 pt-2 sm:flex-row sm:items-center">
                        <img
                            src="/fdroid-repo-qr.svg"
                            alt="QR code to add the Dabney.moe F-Droid repository"
                            class="h-40 w-40 shrink-0 rounded-xl bg-white p-1.5 shadow-md"
                        />
                        <div class="space-y-2 text-xs text-slate-300 min-w-0">
                            <p class="font-semibold text-white">"Scan from Desktop with F-Droid"</p>
                            <p>
                                "Open F-Droid → Settings → Repositories → "
                                <strong class="text-white">"+"</strong>
                                " and scan this QR code (or paste the repo URL below)."
                            </p>
                            <div class="space-y-1">
                                <p class="text-[11px] uppercase tracking-wider text-slate-400">"Repository URL"</p>
                                <code class="block break-all rounded-lg border border-white/10 bg-slate-950 px-2.5 py-1.5 font-mono text-[11px] text-cyan-200">
                                    {FDROID_REPO_URL}
                                </code>
                            </div>
                        </div>
                    </div>
                    <div class="space-y-1 pt-1">
                        <p class="text-[11px] uppercase tracking-wider text-slate-400">
                            "F-Droid Repository Signing Fingerprint (SHA-256)"
                        </p>
                        <code class="block break-all rounded-lg border border-white/10 bg-slate-950 px-2.5 py-1.5 font-mono text-[11px] text-emerald-200">
                            {FDROID_REPO_FINGERPRINT}
                        </code>
                    </div>
                </section>

                // Card 2: Direct Signed APK + Obtainium
                <section class="rounded-2xl border border-cyan-400/30 bg-slate-900/60 p-6 space-y-5">
                    <div class="space-y-3">
                        <div class="flex items-center justify-between gap-2">
                            <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                                "Direct Download · Universal Signed APK"
                            </p>
                            <span class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-2.5 py-0.5 font-mono text-[10px] text-cyan-200">
                                "24 MB · API 26+"
                            </span>
                        </div>
                        <h2 class="text-xl font-bold text-white">
                            {format!("Download Signed APK (v{ANDROID_APP_VERSION})")}
                        </h2>
                        <p class="text-sm text-slate-300">
                            "Signed universal release APK supporting 64-bit ARM phones ("
                            <code class="text-cyan-200">"arm64-v8a"</code>
                            ") and Android emulators ("
                            <code class="text-cyan-200">"x86_64"</code>
                            "), verified 16 KB page-size aligned for Android 15–17."
                        </p>
                        <div class="flex flex-wrap gap-3">
                            <a
                                href=DIRECT_APK_URL
                                class="inline-flex min-h-[44px] items-center rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                            >
                                "Download APK (Codeberg) ↓"
                            </a>
                            <a
                                href=GITHUB_RELEASE_URL
                                target="_blank"
                                rel="noopener noreferrer"
                                class="inline-flex min-h-[44px] items-center rounded-full border border-white/15 px-4 py-2 text-xs font-semibold text-slate-200 transition hover:bg-white/5"
                            >
                                "GitHub Release Mirror ↗"
                            </a>
                        </div>
                        <div class="space-y-2 pt-1 text-xs">
                            <div>
                                <p class="text-[11px] uppercase tracking-wider text-slate-400">"APK SHA-256 Checksum"</p>
                                <code class="mt-0.5 block break-all rounded-lg border border-white/10 bg-slate-950 px-2.5 py-1.5 font-mono text-[11px] text-slate-200">
                                    {APK_SHA256}
                                </code>
                            </div>
                            <div>
                                <p class="text-[11px] uppercase tracking-wider text-slate-400">"APK Signing Certificate SHA-256"</p>
                                <code class="mt-0.5 block break-all rounded-lg border border-white/10 bg-slate-950 px-2.5 py-1.5 font-mono text-[11px] text-slate-300">
                                    {SIGNING_CERT_SHA256}
                                </code>
                            </div>
                        </div>
                    </div>

                    <div class="border-t border-white/10 pt-4 space-y-2">
                        <p class="text-xs font-semibold uppercase tracking-widest text-purple-300">
                            "Obtainium Auto-Updates"
                        </p>
                        <p class="text-xs text-slate-300">
                            "Using Obtainium? Download the pre-configured JSON profile or paste the F-Droid repository URL directly inside Obtainium → Add App."
                        </p>
                        <a
                            href="/obtainium-dabney-demo.json"
                            download="obtainium-dabney-demo.json"
                            class="inline-flex min-h-[38px] items-center rounded-full border border-purple-400/35 bg-purple-500/15 px-4 py-1.5 text-xs font-semibold text-purple-200 transition hover:bg-purple-500/25"
                        >
                            "Download Obtainium Config (.json)"
                        </a>
                    </div>
                </section>
            </div>

            // Store & Transparency Matrix
            <section class="rounded-2xl border border-white/10 bg-slate-900/40 p-6 space-y-4">
                <h2 class="text-lg font-semibold text-white">"Distribution Channels & Zero-PHI Audit"</h2>
                <div class="grid gap-4 sm:grid-cols-3 text-xs">
                    <div class="rounded-xl border border-emerald-400/25 bg-slate-950/70 p-4 space-y-1.5">
                        <p class="font-semibold text-emerald-300">"Dabney.moe F-Droid Repo"</p>
                        <p class="text-slate-300">
                            "Live now on Codeberg HTTPS with signed v1/v2 indexes, Fastlane screenshots, and automatic updates."
                        </p>
                    </div>
                    <div class="rounded-xl border border-cyan-400/25 bg-slate-950/70 p-4 space-y-1.5">
                        <p class="font-semibold text-cyan-300">"IzzyOnDroid & Official F-Droid"</p>
                        <p class="text-slate-300">
                            "IzzyOnDroid inclusion requested ("
                            <a href=IZZYONDROID_ISSUE_URL target="_blank" rel="noopener noreferrer" class="text-cyan-400 underline">
                                "#966"
                            </a>
                            "). Official F-Droid recipe passed 9/9 CI checks ("
                            <a href=FDROID_MR_URL target="_blank" rel="noopener noreferrer" class="text-cyan-400 underline">
                                "!59140"
                            </a>
                            ") and is queued for F-Droid's 3-month maturity window."
                        </p>
                    </div>
                    <div class="rounded-xl border border-white/10 bg-slate-950/70 p-4 space-y-1.5">
                        <p class="font-semibold text-slate-200">"Zero-PHI & Permission Guarantee"</p>
                        <p class="text-slate-300">
                            "100% synthetic sample records. Zero Health Connect, camera, microphone, location, or contacts permissions. Zero analytics or ads. "
                            <A href="/privacy" attr:class="text-cyan-400 underline">"Read privacy policy"</A>
                            "."
                        </p>
                    </div>
                </div>
            </section>

            // Try In-Browser Instead
            <section class="rounded-2xl border border-white/10 bg-slate-900/40 p-6 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                <div>
                    <h2 class="text-base font-semibold text-white">"Want to preview right now without installing?"</h2>
                    <p class="mt-1 text-xs text-slate-400">
                        "Every screen in the Android app runs identically in your browser from the same Rust/WASM codebase."
                    </p>
                </div>
                <div class="flex flex-wrap gap-2.5">
                    <A
                        href="/clinical/patient"
                        attr:class="inline-flex items-center rounded-full bg-emerald-500 px-4 py-2 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                    >
                        "Patient Demo →"
                    </A>
                    <A
                        href="/clinical/physician"
                        attr:class="inline-flex items-center rounded-full bg-cyan-500 px-4 py-2 text-xs font-semibold text-slate-950 transition hover:bg-cyan-400"
                    >
                        "Physician Demo →"
                    </A>
                    <a
                        href=CODEBERG_SOURCE_URL
                        target="_blank"
                        rel="noopener noreferrer"
                        class="inline-flex items-center rounded-full border border-white/15 px-4 py-2 text-xs font-semibold text-slate-200 transition hover:bg-white/5"
                    >
                        "Source Code (Codeberg) ↗"
                    </a>
                </div>
            </section>
        </div>

        <Footer />
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use leptos_meta::provide_meta_context;
    use leptos_router::components::Router;
    use leptos_router::location::RequestUrl;

    use crate::consent::provide_consent_context;

    #[test]
    fn android_download_page_renders_fdroid_fingerprint_and_apk_sha256() {
        let owner = Owner::new();
        let html = owner.with(|| {
            provide_meta_context();
            provide_consent_context();
            provide_context(RequestUrl::new("/android"));
            view! {
                <Router>
                    <AndroidDownloadPage />
                </Router>
            }
            .to_html()
        });
        drop(owner);
        assert!(html.contains(ANDROID_APP_PACKAGE));
        assert!(html.contains(FDROID_REPO_URL));
        assert!(html.contains(FDROID_REPO_FINGERPRINT));
        assert!(html.contains(APK_SHA256));
        assert!(html.contains(SIGNING_CERT_SHA256));
        assert!(html.contains("/fdroid-repo-qr.svg"));
        assert!(html.contains("/obtainium-dabney-demo.json"));
        assert!(html.contains("Zero PHI"));
    }
}
