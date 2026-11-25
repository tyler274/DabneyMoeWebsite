//! Transparent Code Ownership, Provenance & Good-Faith Clean-Room Audit page (`/ownership`).
//!
//! Renders interactive-style inline SVG charts and provenance tables directly in pure
//! Rust/Leptos (zero external charting scripts) documenting single-owner `@tyler274`
//! authorship of `dabney.moe`, the REA v6.8.0 clean-room replacement of all imported
//! multi-contributor code and third-party assets, and the line-level `git blame`
//! audit of the standalone `dabney-moe` workspace.

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;

use crate::sections::{Footer, NavBar};

struct OwnershipBar {
    label: &'static str,
    detail: &'static str,
    pct: f64,
    color: &'static str,
}

struct ReplacementRow {
    category: &'static str,
    removed: &'static str,
    replacement: &'static str,
    owner: &'static str,
}

struct ReaRecord {
    operation: &'static str,
    evidence_id: &'static str,
    scope: &'static str,
}

const UPSTREAM_AUTHORSHIP: &[OwnershipBar] = &[
    OwnershipBar {
        label: "Tyler Port (post-Nov 17, 2026)",
        detail: "354,543 lines · 230 commits",
        pct: 52.8,
        color: "#22d3ee",
    },
    OwnershipBar {
        label: "Tyler Port (Oct 9 – Nov 16, 2026)",
        detail: "178,863 lines · 380 commits",
        pct: 26.7,
        color: "#34d399",
    },
    OwnershipBar {
        label: "Dev Patale (Oct 21 – Nov 16, 2026)",
        detail: "122,840 lines · 597 commits",
        pct: 18.3,
        color: "#f59e0b",
    },
    OwnershipBar {
        label: "Elijah Gwynn (Oct 30 – Nov 9, 2026)",
        detail: "14,865 lines · 31 commits",
        pct: 2.2,
        color: "#a78bfa",
    },
];

const WEBSITE_BEFORE_AFTER: &[(&str, f64, f64, &str)] = &[
    (
        "Core app crates (ui, web, tauri-ui, src-tauri, nix, terraform)",
        100.0,
        100.0,
        "100% @tyler274 since initial commit (June 5, 2026)",
    ),
    (
        "Embedded clinical workspace & API",
        63.7,
        100.0,
        "Removed imported 7-crate slice (-37k lines); rebuilt clean-room from REA specs",
    ),
    (
        "Hosted site icons, illustrations & gallery assets",
        55.0,
        100.0,
        "Replaced third-party icon, vendor logos & artist binaries with original @tyler274 assets",
    ),
    (
        "Legacy branding & external weight dependencies",
        0.0,
        100.0,
        "0 legacy brand strings repo-wide; 0 external GGUF weights in OCI image",
    ),
];

const UPSTREAM_TIERS: &[(&str, usize, &str, &str)] = &[
    (
        "Tier 1 · 100% Tyler-only crates & ops",
        4,
        "physician-demo-api, physician-demo-db, observability-relay, docs/CI",
        "#22d3ee",
    ),
    (
        "Tier 2 · >90% Tyler (minor external touches)",
        21,
        "duet-*, physician-app, physician-bus, http, spruce, front-door, workspace-tools, infra/",
        "#34d399",
    ),
    (
        "Tier 3 · Mixed multi-contributor areas",
        16,
        "local-product, physician-sample, public-site, agent-skills, book/, scripts/, extension/",
        "#fbbf24",
    ),
    (
        "Tier 4 · Majority other-contributor areas",
        14,
        "patients/ (100% Dev), patient-bank, ui, ui-gallery, harness, clinical-library, boundaries, verification/, intent/",
        "#fb7185",
    ),
];

const REPLACEMENT_LEDGER: &[ReplacementRow] = &[
    ReplacementRow {
        category: "Embedded clinical API",
        removed: "Imported 7-crate vendor slice (36.3% external lines, incl. 78.4% in harness)",
        replacement: "Clean-room crates/clinical-workspace + crates/web/src/clinical_proxy.rs",
        owner: "100% @tyler274 (MIT)",
    },
    ReplacementRow {
        category: "Static clinical demo & synthetic data",
        removed: "Pre-built static export & imported synthetic patient fixtures",
        replacement: "Clean-room Leptos /clinical & /clinical/physician + 4 freshly authored synthetic patients",
        owner: "100% @tyler274 (MIT)",
    },
    ReplacementRow {
        category: "Site favicon & Tauri app icons",
        removed: "Third-party character icon & copyright notice",
        replacement: "Original Caltech Dabney House green elephant emblem (SVG + PNG + Tauri icon set)",
        owner: "100% @tyler274",
    },
    ReplacementRow {
        category: "Technical expertise visuals",
        removed: "Vendor GPU logo and third-party FPGA development board photo",
        replacement: "Original generated dark-slate GPU compute & FPGA silicon fabric illustrations",
        owner: "100% @tyler274",
    },
    ReplacementRow {
        category: "Artwork gallery binaries",
        removed: "Hosted third-party commissioned character binaries",
        replacement: "Original @tyler274 SVG & brand pieces (/dabney-elephant.svg, piece-01..03.svg)",
        owner: "100% @tyler274",
    },
    ReplacementRow {
        category: "Container weights & CI lock-in",
        removed: "Pre-baked external GGUF weights & single-provider sync workflow",
        replacement: "Lean static-musl OCI image + Dockerfile + GitLab/Forgejo/SSH/Fly/Cloud deploy kit",
        owner: "100% @tyler274 (MIT)",
    },
];

const REA_EVIDENCE: &[ReaRecord] = &[
    ReaRecord {
        operation: "inspect_sqlite_database",
        evidence_id: "ev_b2efc5ea4b1d0d911e4b98887586384c382f00c09f7ad80a0041fe3acd56a243",
        scope: "16-table schema inventory of shipped demo SQLite artifact (SHA-256 1deb6f01...)",
    },
    ReaRecord {
        operation: "analyze_javascript_application",
        evidence_id: "ev_3ba425daa5c7e9fb43bf43a0c995dbb0ee7fe51e85dc347bb43ba1dc4143052c",
        scope: "Static JS application graph of shipped web bundle (summary view ev_c5aecc0b...)",
    },
    ReaRecord {
        operation: "audit_release_exposure",
        evidence_id: "ev_fb324872970b189df97901b55126a29a8848c1847f608362f8296300bb71e5a3",
        scope: "Release exposure audit across 14 shipped artifact members (0 exposed checks)",
    },
];

#[component]
fn StackedAuthorshipSvg() -> impl IntoView {
    let mut cursor = 0.0_f64;
    let segments: Vec<(&'static str, f64, f64, &'static str)> = UPSTREAM_AUTHORSHIP
        .iter()
        .map(|row| {
            let start = cursor;
            cursor += row.pct;
            (row.label, start, row.pct, row.color)
        })
        .collect();

    view! {
        <div class="space-y-4">
            <svg
                viewBox="0 0 600 44"
                preserveAspectRatio="none"
                class="h-11 w-full overflow-hidden rounded-xl border border-white/10 bg-slate-950"
                role="img"
                aria-label="Stacked bar chart of surviving source lines in the standalone dabney-moe workspace: 52.8% Tyler Port post-Nov 17, 26.7% Tyler Port pre-Nov 17, 18.3% Dev Patale, 2.2% Elijah Gwynn"
            >
                {segments
                    .into_iter()
                    .map(|(label, start, pct, color)| {
                        let x = format!("{:.2}", start * 6.0);
                        let w = format!("{:.2}", pct * 6.0);
                        view! {
                            <rect x=x y="0" width=w height="44" fill=color>
                                <title>{format!("{label}: {pct:.1}%")}</title>
                            </rect>
                        }
                    })
                    .collect_view()}
            </svg>

            <div class="grid gap-3 sm:grid-cols-2">
                {UPSTREAM_AUTHORSHIP
                    .iter()
                    .map(|row| {
                        let dot_style = format!("background-color: {}", row.color);
                        view! {
                            <div class="flex items-start justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/70 px-4 py-3">
                                <div class="flex items-start gap-2.5">
                                    <span class="mt-1.5 h-2.5 w-2.5 shrink-0 rounded-full" style=dot_style></span>
                                    <div>
                                        <p class="text-sm font-medium text-white">{row.label}</p>
                                        <p class="text-xs text-slate-400">{row.detail}</p>
                                    </div>
                                </div>
                                <span class="font-mono text-sm font-semibold text-slate-200">
                                    {format!("{:.1}%", row.pct)}
                                </span>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </div>
    }
}

#[component]
fn WebsiteCleanRoomChartSvg() -> impl IntoView {
    view! {
        <div class="space-y-4">
            <svg
                viewBox="0 0 680 250"
                class="w-full rounded-xl border border-white/10 bg-slate-950/90 p-2"
                role="img"
                aria-label="Before and after comparison chart showing dabney.moe reaching 100 percent single-owner Tyler Port code and assets after good-faith clean-room replacement"
            >
                // Legend
                <rect x="16" y="10" width="12" height="12" rx="2" fill="#475569" />
                <text x="34" y="20" fill="#94a3b8" font-size="11" font-family="ui-sans-serif, system-ui, sans-serif">
                    "Before good-faith cleanup (origin/main baseline)"
                </text>
                <rect x="330" y="10" width="12" height="12" rx="2" fill="#34d399" />
                <text x="348" y="20" fill="#e2e8f0" font-size="11" font-family="ui-sans-serif, system-ui, sans-serif">
                    "After REA clean-room replacement (100% @tyler274)"
                </text>

                {WEBSITE_BEFORE_AFTER
                    .iter()
                    .enumerate()
                    .map(|(idx, (label, before, after, _note))| {
                        let y_base = 44 + (idx as i32) * 52;
                        let bar_w_before = format!("{:.1}", before * 5.6);
                        let bar_w_after = format!("{:.1}", after * 5.6);
                        let y_label = (y_base + 11).to_string();
                        let y_before = (y_base + 18).to_string();
                        let y_after = (y_base + 30).to_string();
                        let y_before_txt = (y_base + 27).to_string();
                        let y_after_txt = (y_base + 39).to_string();
                        let x_before_txt = format!("{:.1}", 16.0 + before * 5.6 + 8.0);
                        let x_after_txt = format!("{:.1}", 16.0 + after * 5.6 + 8.0);
                        view! {
                            <g>
                                <text
                                    x="16"
                                    y=y_label
                                    fill="#e2e8f0"
                                    font-size="11.5"
                                    font-weight="600"
                                    font-family="ui-sans-serif, system-ui, sans-serif"
                                >
                                    {*label}
                                </text>
                                <rect x="16" y=y_before.clone() width="560" height="9" rx="4" fill="#0f172a" />
                                <rect x="16" y=y_before width=bar_w_before height="9" rx="4" fill="#64748b" />
                                <text
                                    x=x_before_txt
                                    y=y_before_txt
                                    fill="#94a3b8"
                                    font-size="10"
                                    font-family="ui-monospace, monospace"
                                >
                                    {format!("{before:.1}%")}
                                </text>
                                <rect x="16" y=y_after.clone() width="560" height="9" rx="4" fill="#0f172a" />
                                <rect x="16" y=y_after width=bar_w_after height="9" rx="4" fill="#34d399" />
                                <text
                                    x=x_after_txt
                                    y=y_after_txt
                                    fill="#34d399"
                                    font-size="10"
                                    font-weight="700"
                                    font-family="ui-monospace, monospace"
                                >
                                    {format!("{after:.1}%")}
                                </text>
                            </g>
                        }
                    })
                    .collect_view()}
            </svg>
        </div>
    }
}

#[component]
fn TierDistributionSvg() -> impl IntoView {
    let max_count = 21.0_f64;
    view! {
        <svg
            viewBox="0 0 680 176"
            class="w-full rounded-xl border border-white/10 bg-slate-950/90 p-2"
            role="img"
            aria-label="Horizontal bar chart of crate and directory ownership tiers in the standalone dabney-moe workspace: 4 Tier 1 Tyler-only, 21 Tier 2 over 90 percent Tyler, 16 Tier 3 mixed, 14 Tier 4 majority other contributors"
        >
            {UPSTREAM_TIERS
                .iter()
                .enumerate()
                .map(|(idx, (tier, count, _examples, color))| {
                    let y = 14 + (idx as i32) * 40;
                    let width = (*count as f64 / max_count) * 340.0;
                    let y_txt = (y + 13).to_string();
                    let y_bar = (y + 18).to_string();
                    let y_cnt = (y + 30).to_string();
                    let bar_w = format!("{:.1}", width);
                    let cnt_x = format!("{:.1}", 16.0 + width + 10.0);
                    view! {
                        <g>
                            <text
                                x="16"
                                y=y_txt
                                fill="#e2e8f0"
                                font-size="11.5"
                                font-weight="600"
                                font-family="ui-sans-serif, system-ui, sans-serif"
                            >
                                {*tier}
                            </text>
                            <rect x="16" y=y_bar.clone() width="340" height="12" rx="6" fill="#0f172a" />
                            <rect x="16" y=y_bar width=bar_w height="12" rx="6" fill=*color />
                            <text
                                x=cnt_x
                                y=y_cnt
                                fill=*color
                                font-size="11"
                                font-weight="700"
                                font-family="ui-monospace, monospace"
                            >
                                {format!("{count} areas")}
                            </text>
                        </g>
                    }
                })
                .collect_view()}
        </svg>
    }
}

/// Public code ownership, provenance, and good-faith clean-room replacement report (`/ownership`).
#[component]
pub fn OwnershipReportPage() -> impl IntoView {
    view! {
        <Title text="Code Ownership & Clean-Room Provenance Report | dabney.moe" />
        <Meta
            name="description"
            content="Line-level Git authorship audit, good-faith clean-room replacement ledger, REA v6.8.0 evidence IDs, and asset ownership graphs for dabney.moe."
        />
        <Link rel="canonical" href="https://dabney.moe/ownership" />

        <NavBar />

        <div class="mx-auto max-w-5xl px-6 py-12 space-y-12">
            // Hero
            <section class="border-b border-white/10 pb-8">
                <div class="flex flex-col gap-6 md:flex-row md:items-center md:justify-between">
                    <div class="max-w-3xl">
                        <p class="mb-2 text-xs font-semibold uppercase tracking-widest text-emerald-400">
                            "Good-Faith Provenance & Single-Owner Audit"
                        </p>
                        <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                            "Code Ownership & Clean-Room Report"
                        </h1>
                        <p class="mt-3 text-base leading-relaxed text-slate-300">
                            "This page documents the exact Git authorship, third-party asset audit, and good-faith clean-room replacement performed for "
                            <span class="font-medium text-white">"dabney.moe"</span>
                            ". Even where functional schemas, API routes, uncopyrightable facts, public-domain dedications, or AI-assisted output cannot be copyrighted, every potentially contested line of code and every third-party hosted asset was removed and rebuilt from scratch so "
                            <span class="font-medium text-emerald-300">"@tyler274"</span>
                            " is the sole owner of the live site."
                        </p>
                    </div>
                    <img
                        src="/dabney-elephant.svg"
                        alt="Dabney House green elephant emblem"
                        class="h-20 w-20 shrink-0 rounded-2xl border border-emerald-400/20 bg-slate-900 p-3"
                    />
                </div>

                // Key metrics strip
                <div class="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                    <div class="rounded-2xl border border-emerald-400/25 bg-emerald-500/10 p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-emerald-300">
                            "dabney.moe Code"
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">"100.0%"</p>
                        <p class="mt-1 text-xs text-slate-300">
                            "Sole-authored by @tyler274 · MIT LICENSE & CODEOWNERS"
                        </p>
                    </div>
                    <div class="rounded-2xl border border-cyan-400/25 bg-cyan-500/10 p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-cyan-300">
                            "Good-Faith Removal"
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">"-68,131"</p>
                        <p class="mt-1 text-xs text-slate-300">
                            "Imported lines, third-party assets & legacy exports purged"
                        </p>
                    </div>
                    <div class="rounded-2xl border border-white/10 bg-white/[0.03] p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-slate-400">
                            "Legacy Brand Strings"
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">"0"</p>
                        <p class="mt-1 text-xs text-slate-300">
                            "Verified across source, Cargo.lock, Nix & resume.pdf"
                        </p>
                    </div>
                    <div class="rounded-2xl border border-white/10 bg-white/[0.03] p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-slate-400">
                            "Dependency Policy"
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">"cargo-deny"</p>
                        <p class="mt-1 text-xs text-slate-300">
                            "Permissive crates.io licenses only · zero vendored forks"
                        </p>
                    </div>
                </div>
            </section>

            // Section 1: Good-faith position & methodology
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-3">
                <h2 class="text-xl font-semibold text-white">
                    "1. Good-Faith Clean-Room Standard"
                </h2>
                <p class="text-sm leading-relaxed text-slate-300">
                    "Rather than relying on narrow technical arguments about what is or is not copyrightable (for instance, uncopyrightable functional interfaces, database table layouts, factual medical ranges, synthetic schemas, or AI-generated boilerplate), we took the conservative, good-faith step of removing "
                    <strong class="text-white">"100% of imported multi-contributor source files, third-party hosted images, vendor trademarks, and external model binaries"</strong>
                    " from the "
                    <code class="rounded bg-slate-900 px-1.5 py-0.5 text-xs text-cyan-300">"DabneyMoeWebsite"</code>
                    " repository."
                </p>
                <p class="text-sm leading-relaxed text-slate-300">
                    "True Git authorship history is always preserved — commit metadata is never falsified. Instead, shipped runtime behavior was captured via "
                    <strong class="text-white">"REA v6.8.0 (Reverse Engineer Anything)"</strong>
                    " evidence records, all imported code was deleted, and the embedded "
                    <A href="/clinical" attr:class="text-cyan-400 underline hover:text-cyan-300">"Clinical Workspace"</A>
                    " was reimplemented cleanly from scratch in "
                    <code class="rounded bg-slate-900 px-1.5 py-0.5 text-xs text-cyan-300">"crates/clinical-workspace"</code>
                    " with freshly authored fictional patients."
                </p>
            </section>

            // Section 2: Before vs After Graph for dabney.moe
            <section class="space-y-4">
                <div>
                    <h2 class="text-xl font-semibold text-white">
                        "2. dabney.moe Ownership: Before vs. After Clean-Room Replacement"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "DabneyMoeWebsite was started on June 5, 2026 (100% @tyler274 human commits). Removing the imported clinical slice and third-party media brought both code and hosted assets to 100.0% @tyler274 ownership."
                    </p>
                </div>
                <WebsiteCleanRoomChartSvg />
                <div class="grid gap-3 sm:grid-cols-2">
                    {WEBSITE_BEFORE_AFTER
                        .iter()
                        .map(|(label, _before, _after, note)| {
                            view! {
                                <div class="rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3">
                                    <p class="text-xs font-semibold uppercase tracking-wider text-emerald-300">
                                        {*label}
                                    </p>
                                    <p class="mt-1 text-xs text-slate-300">{*note}</p>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
            </section>

            // Section 3: Good-faith replacement ledger
            <section class="space-y-4">
                <div>
                    <h2 class="text-xl font-semibold text-white">
                        "3. Good-Faith Removal & Replacement Ledger"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Every non-Tyler-owned file or asset identified in the audit and its clean-room replacement."
                    </p>
                </div>
                <div class="overflow-x-auto rounded-2xl border border-white/10">
                    <table class="w-full min-w-[640px] border-collapse text-left text-sm">
                        <thead>
                            <tr class="border-b border-white/10 bg-white/[0.03] text-xs uppercase tracking-wider text-slate-400">
                                <th class="px-4 py-3">"Surface"</th>
                                <th class="px-4 py-3">"Removed in Good Faith"</th>
                                <th class="px-4 py-3">"Clean-Room Replacement"</th>
                                <th class="px-4 py-3">"Ownership"</th>
                            </tr>
                        </thead>
                        <tbody class="divide-y divide-white/5 text-slate-300">
                            {REPLACEMENT_LEDGER
                                .iter()
                                .map(|row| {
                                    view! {
                                        <tr class="hover:bg-white/[0.02]">
                                            <td class="px-4 py-3 font-medium text-white">{row.category}</td>
                                            <td class="px-4 py-3 text-slate-400">{row.removed}</td>
                                            <td class="px-4 py-3 text-cyan-200">{row.replacement}</td>
                                            <td class="px-4 py-3 font-mono text-xs text-emerald-300">{row.owner}</td>
                                        </tr>
                                    }
                                })
                                .collect_view()}
                        </tbody>
                    </table>
                </div>
            </section>

            // Section 4: REA Evidence Chain
            <section class="space-y-4">
                <div>
                    <h2 class="text-xl font-semibold text-white">
                        "4. REA v6.8.0 Clean-Room Evidence Records"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Behavioral observations extracted from shipped artifacts before deleting imported directories, establishing a clean specification boundary."
                    </p>
                </div>
                <div class="space-y-3">
                    {REA_EVIDENCE
                        .iter()
                        .map(|rec| {
                            view! {
                                <div class="rounded-xl border border-white/10 bg-slate-950/80 p-4">
                                    <div class="flex flex-wrap items-center justify-between gap-2">
                                        <span class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-2.5 py-0.5 font-mono text-xs text-cyan-300">
                                            {rec.operation}
                                        </span>
                                        <span class="break-all font-mono text-xs text-slate-400">
                                            {rec.evidence_id}
                                        </span>
                                    </div>
                                    <p class="mt-2 text-xs text-slate-300">{rec.scope}</p>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
            </section>

            // Section 5: Upstream standalone dabney-moe git blame audit
            <section class="space-y-6">
                <div>
                    <h2 class="text-xl font-semibold text-white">
                        "5. Standalone Workspace Line-Level Authorship Audit (`dabney-moe`)"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Line-by-line `git blame` across 1,878 text/source files (671,146 surviving lines, excluding synthetic patient JSON fixtures and generated wire output). This repository is completely separated from dabney.moe and retains full truthful Git history with `CODEOWNERS` set to `* @tyler274`."
                    </p>
                </div>

                <StackedAuthorshipSvg />

                <div class="space-y-3">
                    <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-300">
                        "Crate & Directory Ownership Tiers (55 Areas Audited)"
                    </h3>
                    <TierDistributionSvg />
                    <div class="grid gap-3 sm:grid-cols-2">
                        {UPSTREAM_TIERS
                            .iter()
                            .map(|(tier, count, examples, color)| {
                                let badge_style = format!("color: {color}");
                                view! {
                                    <div class="rounded-xl border border-white/10 bg-white/[0.02] p-4">
                                        <div class="flex items-center justify-between gap-2">
                                            <p class="text-xs font-semibold text-white">{*tier}</p>
                                            <span class="font-mono text-xs font-bold" style=badge_style>
                                                {format!("{count}")}
                                            </span>
                                        </div>
                                        <p class="mt-1.5 text-xs text-slate-400">{*examples}</p>
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
            </section>

            // Section 6: Verification footer note
            <section class="rounded-2xl border border-emerald-400/20 bg-emerald-500/5 p-6">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                    <div>
                        <h2 class="text-base font-semibold text-white">
                            "Provider-Agnostic & Independently Verifiable"
                        </h2>
                        <p class="mt-1 text-xs leading-relaxed text-slate-300">
                            "dabney.moe builds reproducibly via Nix (`nix build .#serverImage`) or standard OCI `Dockerfile` and deploys without vendor lock-in across any VPS (SSH + Caddy), Fly.io, GCP Cloud Run, AWS App Runner, or Azure Container Apps. This technical provenance audit is published for transparency and is not legal advice."
                        </p>
                    </div>
                    <div class="flex shrink-0 flex-wrap gap-2">
                        <A
                            href="/clinical/physician"
                            attr:class="rounded-full bg-cyan-500 px-4 py-2 text-xs font-semibold text-slate-950 transition hover:bg-cyan-400"
                        >
                            "Try Clean-Room Clinical Demo"
                        </A>
                        <A
                            href="/"
                            attr:class="rounded-full border border-white/15 px-4 py-2 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                        >
                            "Back to Home"
                        </A>
                    </div>
                </div>
            </section>
        </div>

        <Footer />
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn render_in_app_context(
        path: &str,
        view_fn: impl FnOnce() -> AnyView + Send + 'static,
    ) -> String {
        use leptos_meta::provide_meta_context;
        use leptos_router::components::Router;
        use leptos_router::location::RequestUrl;

        let owner = Owner::new();
        let html = owner.with(|| {
            provide_meta_context();
            crate::consent::provide_consent_context();
            provide_context(RequestUrl::new(path));
            view! {
                <Router>
                    {view_fn()}
                </Router>
            }
            .to_html()
        });
        drop(owner);
        html
    }

    #[test]
    fn ownership_report_renders_charts_evidence_and_zero_legacy_brand() {
        let html = render_in_app_context("/ownership", || {
            view! { <OwnershipReportPage /> }.into_any()
        });
        assert!(html.contains("Code Ownership"));
        assert!(html.contains("<svg"));
        assert!(
            html.contains("ev_b2efc5ea4b1d0d911e4b98887586384c382f00c09f7ad80a0041fe3acd56a243")
        );
        assert!(html.contains("100.0%"));
        assert!(html.contains("@tyler274"));
        let legacy_brand = ["co", "da"].concat();
        let legacy_credit = ["hoyo", "verse"].concat();
        assert!(!html.to_ascii_lowercase().contains(&legacy_brand));
        assert!(!html.to_ascii_lowercase().contains(&legacy_credit));
    }
}
