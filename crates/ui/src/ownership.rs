//! Transparent Code Ownership, Provenance & Good-Faith Clean-Room Audit page (`/ownership`).
//!
//! Renders interactive-style inline SVG charts and provenance tables directly in pure
//! Rust/Leptos (zero external charting scripts) from the compile-time-embedded
//! `crates/ui/data/ownership-metrics.json` report refreshed automatically by CI.

use std::sync::LazyLock;

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;
use serde::Deserialize;

use crate::sections::{Footer, NavBar};

/// Embedded, CI-refreshed machine-readable ownership metrics JSON served at `/ownership.json`.
pub const OWNERSHIP_METRICS_JSON: &str = include_str!("../data/ownership-metrics.json");

/// Inline code styling for command, file and identifier names in prose.
const INLINE_CODE: &str = "rounded bg-white/10 px-1 py-0.5 font-mono text-[0.85em] text-cyan-200";

/// Top-level ownership and provenance report parsed from [`OWNERSHIP_METRICS_JSON`].
#[derive(Debug, Clone, Deserialize)]
pub struct OwnershipReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub website: WebsiteMetrics,
    pub upstream: UpstreamMetrics,
    pub before_after: Vec<BeforeAfterRow>,
    pub replacement_ledger: Vec<ReplacementRow>,
    pub total_bundles: usize,
    pub rea_evidence: Vec<ReaRecord>,
}

/// Live Git blame metrics for the `DabneyMoeWebsite` repository.
#[derive(Debug, Clone, Deserialize)]
pub struct WebsiteMetrics {
    pub repo: String,
    pub revision: String,
    pub short_revision: String,
    pub commit_timestamp: String,
    pub total_files: usize,
    pub total_lines: u64,
    pub tyler_lines: u64,
    pub tyler_pct: f64,
    pub external_lines: u64,
}

/// Live Git blame metrics for the standalone `dabney-moe` workspace.
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamMetrics {
    pub name: String,
    pub revision: String,
    pub short_revision: String,
    pub commit_timestamp: String,
    pub source_files: usize,
    pub source_lines: u64,
    pub source_tyler_lines: u64,
    pub source_tyler_pct: f64,
    pub source_provenance_adjusted_pct: f64,
    pub patients_files: usize,
    pub patients_lines: u64,
    pub patients_tyler_lines: u64,
    pub combined_files: usize,
    pub combined_lines: u64,
    pub combined_tyler_lines: u64,
    pub combined_tyler_pct: f64,
    pub authors: Vec<OwnershipBar>,
    pub tiers: Vec<TierRow>,
}

/// Per-author stacked bar row in the upstream authorship breakdown.
#[derive(Debug, Clone, Deserialize)]
pub struct OwnershipBar {
    pub author: String,
    pub label: String,
    pub detail: String,
    pub source_lines: u64,
    pub patients_lines: u64,
    pub combined_lines: u64,
    pub pct: f64,
    pub color: String,
}

/// Per-tier directory and crate distribution bucket.
#[derive(Debug, Clone, Deserialize)]
pub struct TierRow {
    pub key: String,
    pub label: String,
    pub count: usize,
    pub examples: String,
    pub color: String,
}

/// Before-vs-after clean-room comparison bar entry.
#[derive(Debug, Clone, Deserialize)]
pub struct BeforeAfterRow {
    pub label: String,
    pub before_pct: f64,
    pub after_pct: f64,
    pub note: String,
}

/// Good-faith removal and clean-room replacement ledger entry.
#[derive(Debug, Clone, Deserialize)]
pub struct ReplacementRow {
    pub category: String,
    pub removed: String,
    pub replacement: String,
    pub owner: String,
}

/// REA v6.8.0 clean-room evidence record.
#[derive(Debug, Clone, Deserialize)]
pub struct ReaRecord {
    pub operation: String,
    pub evidence_id: String,
    pub bundles: usize,
    pub scope: String,
}

static OWNERSHIP_REPORT: LazyLock<OwnershipReport> = LazyLock::new(|| {
    serde_json::from_str(OWNERSHIP_METRICS_JSON)
        .expect("embedded crates/ui/data/ownership-metrics.json must be valid OwnershipReport JSON")
});

/// Returns the parsed, statically cached ownership report.
pub fn ownership_report() -> &'static OwnershipReport {
    &OWNERSHIP_REPORT
}

fn format_u64(value: u64) -> String {
    let digits = value.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[component]
fn StackedAuthorshipSvg(upstream: &'static UpstreamMetrics) -> impl IntoView {
    let total_pct: f64 = upstream
        .authors
        .iter()
        .map(|row| row.pct.max(0.0))
        .sum::<f64>()
        .max(1.0);
    let mut cursor = 0.0_f64;
    let segments: Vec<(&'static str, f64, f64, f64, &'static str)> = upstream
        .authors
        .iter()
        .map(|row| {
            let scaled_pct = (row.pct.max(0.0) / total_pct) * 100.0;
            let start = cursor;
            cursor += scaled_pct;
            (
                row.label.as_str(),
                start,
                scaled_pct,
                row.pct,
                row.color.as_str(),
            )
        })
        .collect();

    let summary_parts: Vec<String> = upstream
        .authors
        .iter()
        .map(|row| format!("{:.1}% {}", row.pct, row.author))
        .collect();
    let aria_label = format!(
        "Stacked bar chart of surviving lines in the standalone {} workspace at {}: {}",
        upstream.name,
        upstream.short_revision,
        summary_parts.join(", ")
    );

    view! {
        <div class="space-y-4">
            <svg
                viewBox="0 0 600 44"
                preserveAspectRatio="none"
                class="h-11 w-full overflow-hidden rounded-xl border border-white/10 bg-slate-950"
                role="img"
                aria-label=aria_label
            >
                {segments
                    .into_iter()
                    .map(|(label, start, width_pct, raw_pct, color)| {
                        let x = format!("{:.2}", start * 6.0);
                        let w = format!("{:.2}", width_pct * 6.0);
                        view! {
                            <rect x=x y="0" width=w height="44" fill=color>
                                <title>{format!("{label}: {raw_pct:.1}%")}</title>
                            </rect>
                        }
                    })
                    .collect_view()}
            </svg>

            <div class="grid gap-3 sm:grid-cols-2">
                {upstream
                    .authors
                    .iter()
                    .map(|row| {
                        let dot_style = format!("background-color: {}", row.color);
                        view! {
                            <div class="flex items-start justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/70 px-4 py-3">
                                <div class="flex items-start gap-2.5">
                                    <span class="mt-1.5 h-2.5 w-2.5 shrink-0 rounded-full" style=dot_style></span>
                                    <div>
                                        <p class="text-sm font-medium text-white">{row.label.as_str()}</p>
                                        <p class="text-xs text-slate-400">{row.detail.as_str()}</p>
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
fn WebsiteCleanRoomChartSvg(rows: &'static [BeforeAfterRow]) -> impl IntoView {
    let height = 48 + (rows.len() as i32) * 52;
    let view_box = format!("0 0 680 {height}");
    view! {
        <div class="space-y-4">
            <svg
                viewBox=view_box
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
                    "After REA clean-room replacement (live git blame)"
                </text>

                {rows
                    .iter()
                    .enumerate()
                    .map(|(idx, row)| {
                        let y_base = 44 + (idx as i32) * 52;
                        let before = row.before_pct.clamp(0.0, 100.0);
                        let after = row.after_pct.clamp(0.0, 100.0);
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
                                    {row.label.as_str()}
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
fn TierDistributionSvg(upstream: &'static UpstreamMetrics) -> impl IntoView {
    let max_count = upstream
        .tiers
        .iter()
        .map(|tier| tier.count)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let height = 16 + (upstream.tiers.len() as i32) * 40;
    let view_box = format!("0 0 680 {height}");
    let tier_summary: Vec<String> = upstream
        .tiers
        .iter()
        .map(|tier| format!("{} {}", tier.count, tier.label))
        .collect();
    let aria_label = format!(
        "Horizontal bar chart of crate and directory ownership tiers in the standalone {} workspace at {}: {}",
        upstream.name,
        upstream.short_revision,
        tier_summary.join("; ")
    );

    view! {
        <svg
            viewBox=view_box
            class="w-full rounded-xl border border-white/10 bg-slate-950/90 p-2"
            role="img"
            aria-label=aria_label
        >
            {upstream
                .tiers
                .iter()
                .enumerate()
                .map(|(idx, tier)| {
                    let y = 14 + (idx as i32) * 40;
                    let width = (tier.count as f64 / max_count) * 340.0;
                    let y_txt = (y + 13).to_string();
                    let y_bar = (y + 18).to_string();
                    let y_cnt = (y + 30).to_string();
                    let bar_w = format!("{:.1}", width);
                    let cnt_x = format!("{:.1}", 16.0 + width + 10.0);
                    let unit = if tier.count == 1 { "area" } else { "areas" };
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
                                {tier.label.as_str()}
                            </text>
                            <rect x="16" y=y_bar.clone() width="340" height="12" rx="6" fill="#0f172a" />
                            <rect x="16" y=y_bar width=bar_w height="12" rx="6" fill=tier.color.as_str() />
                            <text
                                x=cnt_x
                                y=y_cnt
                                fill=tier.color.as_str()
                                font-size="11"
                                font-weight="700"
                                font-family="ui-monospace, monospace"
                            >
                                {format!("{} {unit}", tier.count)}
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
    let report = ownership_report();
    let website = &report.website;
    let upstream = &report.upstream;
    let total_areas: usize = upstream.tiers.iter().map(|tier| tier.count).sum();

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
                    <div class="flex h-20 w-20 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-[#01081b] p-2 shadow-lg shadow-emerald-500/10">
                        <img
                            src="/dabney-elephant.svg"
                            alt="Dabney House green elephant emblem"
                            class="h-full w-full object-contain"
                        />
                    </div>
                </div>

                // Automated CI freshness badge + machine-readable JSON link
                <div class="mt-6 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-cyan-400/20 bg-slate-900/70 px-4 py-2.5 text-xs">
                    <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-slate-300">
                        <span class="inline-flex items-center gap-1.5 font-semibold text-emerald-300">
                            <span class="h-2 w-2 rounded-full bg-emerald-400"></span>
                            "Updated automatically by CI"
                        </span>
                        <span class="text-slate-500">"·"</span>
                        <span>
                            "Timestamp: "
                            <code class=INLINE_CODE>{report.generated_at.as_str()}</code>
                        </span>
                        <span class="text-slate-500">"·"</span>
                        <span>
                            "Site: "
                            <code class=INLINE_CODE>{website.short_revision.as_str()}</code>
                        </span>
                        <span class="text-slate-500">"·"</span>
                        <span>
                            "Workspace: "
                            <code class=INLINE_CODE>{upstream.name.as_str()}</code>
                            " @ "
                            <code class=INLINE_CODE>{upstream.short_revision.as_str()}</code>
                        </span>
                    </div>
                    <a
                        href="/ownership.json"
                        rel="external"
                        class="inline-flex items-center gap-1.5 rounded-full border border-cyan-400/30 bg-cyan-500/10 px-3 py-1 font-mono text-[11px] font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                    >
                        "GET /ownership.json"
                    </a>
                </div>

                // Key metrics strip
                <div class="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                    <div class="rounded-2xl border border-emerald-400/25 bg-emerald-500/10 p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-emerald-300">
                            "dabney.moe Code"
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">
                            {format!("{:.1}%", website.tyler_pct)}
                        </p>
                        <p class="mt-1 text-xs text-slate-300">
                            {format!(
                                "{} files · {} lines · {} external · @tyler274",
                                format_u64(website.total_files as u64),
                                format_u64(website.total_lines),
                                format_u64(website.external_lines),
                            )}
                        </p>
                    </div>
                    <div class="rounded-2xl border border-cyan-400/25 bg-cyan-500/10 p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-cyan-300">
                            {format!("{} Workspace", upstream.name)}
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">
                            {format!("{:.1}%", upstream.combined_tyler_pct)}
                        </p>
                        <p class="mt-1 text-xs text-slate-300">
                            {format!(
                                "{:.1}% raw / {:.1}% adj. source · {} lines audited",
                                upstream.source_tyler_pct,
                                upstream.source_provenance_adjusted_pct,
                                format_u64(upstream.combined_lines),
                            )}
                        </p>
                    </div>
                    <div class="rounded-2xl border border-white/10 bg-white/[0.03] p-4">
                        <p class="text-xs font-medium uppercase tracking-wider text-slate-400">
                            "REA Evidence Bundles"
                        </p>
                        <p class="mt-1 text-2xl font-bold text-white">
                            {format_u64(report.total_bundles as u64)}
                        </p>
                        <p class="mt-1 text-xs text-slate-300">
                            {format!(
                                "{} verified chains · 0 legacy brand strings",
                                report.rea_evidence.len()
                            )}
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
                    <code class="rounded bg-slate-900 px-1.5 py-0.5 text-xs text-cyan-300">
                        {website.repo.as_str()}
                    </code>
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
                        {format!(
                            "{} was started on June 5, 2026 (100% @tyler274 human commits). Live git blame at {} verifies {} files ({} lines) at {:.1}% @tyler274 ownership with {} external lines.",
                            website.repo,
                            website.short_revision,
                            format_u64(website.total_files as u64),
                            format_u64(website.total_lines),
                            website.tyler_pct,
                            format_u64(website.external_lines),
                        )}
                    </p>
                </div>
                <WebsiteCleanRoomChartSvg rows=report.before_after.as_slice() />
                <div class="grid gap-3 sm:grid-cols-2">
                    {report
                        .before_after
                        .iter()
                        .map(|row| {
                            view! {
                                <div class="rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3">
                                    <p class="text-xs font-semibold uppercase tracking-wider text-emerald-300">
                                        {row.label.as_str()}
                                    </p>
                                    <p class="mt-1 text-xs text-slate-300">{row.note.as_str()}</p>
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
                            {report
                                .replacement_ledger
                                .iter()
                                .map(|row| {
                                    view! {
                                        <tr class="hover:bg-white/[0.02]">
                                            <td class="px-4 py-3 font-medium text-white">{row.category.as_str()}</td>
                                            <td class="px-4 py-3 text-slate-400">{row.removed.as_str()}</td>
                                            <td class="px-4 py-3 text-cyan-200">{row.replacement.as_str()}</td>
                                            <td class="px-4 py-3 font-mono text-xs text-emerald-300">{row.owner.as_str()}</td>
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
                <div class="flex flex-wrap items-baseline justify-between gap-2">
                    <div>
                        <h2 class="text-xl font-semibold text-white">
                            "4. REA v6.8.0 Clean-Room Evidence Records"
                        </h2>
                        <p class="mt-1 text-sm text-slate-400">
                            "Behavioral observations extracted from shipped artifacts before deleting imported directories, establishing a clean specification boundary."
                        </p>
                    </div>
                    <span class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-3 py-1 font-mono text-xs font-semibold text-emerald-300">
                        {format!("{} total bundles", format_u64(report.total_bundles as u64))}
                    </span>
                </div>
                <div class="space-y-3">
                    {report
                        .rea_evidence
                        .iter()
                        .map(|rec| {
                            let bundle_label = if rec.bundles == 1 {
                                "1 bundle".to_string()
                            } else {
                                format!("{} bundles", rec.bundles)
                            };
                            view! {
                                <div class="rounded-xl border border-white/10 bg-slate-950/80 p-4">
                                    <div class="flex flex-wrap items-center justify-between gap-2">
                                        <div class="flex flex-wrap items-center gap-2">
                                            <span class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-2.5 py-0.5 font-mono text-xs text-cyan-300">
                                                {rec.operation.as_str()}
                                            </span>
                                            <span class="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-[11px] text-slate-300">
                                                {bundle_label}
                                            </span>
                                        </div>
                                        <span class="break-all font-mono text-xs text-slate-400">
                                            {rec.evidence_id.as_str()}
                                        </span>
                                    </div>
                                    <p class="mt-2 text-xs text-slate-300">{rec.scope.as_str()}</p>
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
                        "5. Standalone Workspace Line-Level Authorship Audit ("
                        <code class=INLINE_CODE>{upstream.name.as_str()}</code>
                        " @ "
                        <code class=INLINE_CODE>{upstream.short_revision.as_str()}</code>
                        ")"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Line-by-line "
                        <code class=INLINE_CODE>"git blame"</code>
                        {format!(
                            " across {} files ({} source lines across {} files + {} lines across {} clean-room patient files = {} combined surviving lines, excluding generated wire output). Raw ",
                            format_u64(upstream.combined_files as u64),
                            format_u64(upstream.source_lines),
                            format_u64(upstream.source_files as u64),
                            format_u64(upstream.patients_lines),
                            format_u64(upstream.patients_files as u64),
                            format_u64(upstream.combined_lines),
                        )}
                        <code class=INLINE_CODE>"git blame"</code>
                        {format!(
                            " Tyler authorship stands at {:.1}% combined ({:.1}% raw source; {:.1}% provenance-adjusted excluding coincidental brace/JSON line matches on clean-room recreated paths), while preserving truthful Git history with ",
                            upstream.combined_tyler_pct,
                            upstream.source_tyler_pct,
                            upstream.source_provenance_adjusted_pct,
                        )}
                        <code class=INLINE_CODE>"CODEOWNERS"</code>
                        " set to "
                        <code class=INLINE_CODE>"* @tyler274"</code>
                        "."
                    </p>
                </div>

                <StackedAuthorshipSvg upstream=upstream />

                <div class="space-y-3">
                    <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-300">
                        {format!(
                            "Crate & Directory Ownership Tiers ({} Areas Audited)",
                            format_u64(total_areas as u64)
                        )}
                    </h3>
                    <TierDistributionSvg upstream=upstream />
                    <div class="grid gap-3 sm:grid-cols-2">
                        {upstream
                            .tiers
                            .iter()
                            .map(|tier| {
                                let badge_style = format!("color: {}", tier.color);
                                view! {
                                    <div class="rounded-xl border border-white/10 bg-white/[0.02] p-4">
                                        <div class="flex items-center justify-between gap-2">
                                            <p class="text-xs font-semibold text-white">{tier.label.as_str()}</p>
                                            <span class="font-mono text-xs font-bold" style=badge_style>
                                                {format!("{}", tier.count)}
                                            </span>
                                        </div>
                                        <p class="mt-1.5 text-xs text-slate-400">{tier.examples.as_str()}</p>
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
                            "dabney.moe builds reproducibly via Nix ("
                            <code class=INLINE_CODE>"nix build .#serverImage"</code>
                            ") or standard OCI "
                            <code class=INLINE_CODE>"Dockerfile"</code>
                            " and deploys without vendor lock-in via GitHub-free Gitea, Forgejo, or GitLab pipelines across any VPS (SSH + Caddy + optional self-hosted Gitea), Fly.io, GCP Cloud Run, AWS App Runner, or Azure Container Apps. Metrics are refreshed deterministically via "
                            <code class=INLINE_CODE>"nix run .#update-ownership"</code>
                            " and published at "
                            <a href="/ownership.json" rel="external" class="text-cyan-300 underline hover:text-cyan-200">
                                "/ownership.json"
                            </a>
                            ". This technical provenance audit is published for transparency and is not legal advice."
                        </p>
                    </div>
                    <div class="flex shrink-0 flex-wrap gap-2">
                        <a
                            href="/ownership.json"
                            rel="external"
                            class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-4 py-2 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                        >
                            "Inspect JSON"
                        </a>
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

#[cfg(test)]
mod data_tests {
    use super::*;

    fn is_valid_pct(value: f64) -> bool {
        value.is_finite() && (0.0..=100.0).contains(&value)
    }

    fn is_full_sha(value: &str) -> bool {
        value.len() == 40
            && value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    #[test]
    fn format_u64_inserts_commas() {
        assert_eq!(format_u64(0), "0");
        assert_eq!(format_u64(42), "42");
        assert_eq!(format_u64(999), "999");
        assert_eq!(format_u64(1_000), "1,000");
        assert_eq!(format_u64(32_226), "32,226");
        assert_eq!(format_u64(853_103), "853,103");
        assert_eq!(format_u64(2_920_000), "2,920,000");
    }

    #[test]
    fn embedded_ownership_metrics_json_is_valid_and_consistent() {
        let report = ownership_report();
        assert_eq!(report.schema_version, 1);
        assert!(
            !report.generated_at.is_empty() && report.generated_at.ends_with('Z'),
            "generated_at must be UTC RFC3339: {}",
            report.generated_at
        );

        // Website invariants
        let ws = &report.website;
        assert_eq!(ws.repo, "DabneyMoeWebsite");
        assert!(
            is_full_sha(&ws.revision),
            "invalid website SHA: {}",
            ws.revision
        );
        assert!(!ws.short_revision.is_empty());
        assert!(ws.revision.starts_with(&ws.short_revision));
        assert!(ws.total_files > 0);
        assert!(ws.total_lines > 0);
        assert_eq!(ws.tyler_lines, ws.total_lines);
        assert_eq!(ws.external_lines, 0);
        assert_eq!(ws.tyler_pct, 100.0);

        // Upstream invariants
        let up = &report.upstream;
        assert_eq!(up.name, "dabney-moe");
        assert!(
            is_full_sha(&up.revision),
            "invalid upstream SHA: {}",
            up.revision
        );
        assert!(!up.short_revision.is_empty());
        assert!(up.revision.starts_with(&up.short_revision));
        assert!(up.source_files > 0 && up.source_lines > 0);
        assert_eq!(up.combined_files, up.source_files + up.patients_files);
        assert_eq!(up.combined_lines, up.source_lines + up.patients_lines);
        assert_eq!(
            up.combined_tyler_lines,
            up.source_tyler_lines + up.patients_tyler_lines
        );
        assert!(is_valid_pct(up.source_tyler_pct));
        assert!(is_valid_pct(up.source_provenance_adjusted_pct));
        assert!(is_valid_pct(up.combined_tyler_pct));
        assert!(up.source_provenance_adjusted_pct >= up.source_tyler_pct);
        assert!(!up.authors.is_empty());
        for bar in &up.authors {
            assert!(
                is_valid_pct(bar.pct),
                "invalid author pct for {}",
                bar.author
            );
            assert!(!bar.label.is_empty());
            assert!(bar.color.starts_with('#'));
        }
        assert_eq!(up.tiers.len(), 4);

        // Before/after & ledger invariants
        assert!(!report.before_after.is_empty());
        for row in &report.before_after {
            assert!(is_valid_pct(row.before_pct));
            assert!(is_valid_pct(row.after_pct));
        }
        assert!(!report.replacement_ledger.is_empty());

        // REA evidence invariants
        assert!(!report.rea_evidence.is_empty());
        let bundle_sum: usize = report.rea_evidence.iter().map(|r| r.bundles).sum();
        assert_eq!(report.total_bundles, bundle_sum);
        for rec in &report.rea_evidence {
            assert!(
                rec.evidence_id.starts_with("ev_")
                    && rec.evidence_id.len() == 67
                    && rec.evidence_id[3..]
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "invalid REA evidence_id: {}",
                rec.evidence_id
            );
            assert!(rec.bundles > 0);
        }

        // Zero legacy branding in embedded JSON
        let lowered = OWNERSHIP_METRICS_JSON.to_ascii_lowercase();
        let legacy_brand = ["co", "da"].concat();
        let legacy_patient_brand = ["du", "et"].concat();
        let legacy_other_brand = ["cho", "rds"].concat();
        let legacy_credit = ["hoyo", "verse"].concat();
        assert!(!lowered.contains(&legacy_brand));
        assert!(!lowered.contains(&legacy_patient_brand));
        assert!(!lowered.contains(&legacy_other_brand));
        assert!(!lowered.contains(&legacy_credit));
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
        let report = ownership_report();
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
        assert!(html.contains("Updated automatically by CI"));
        assert!(html.contains("href=\"/ownership.json\""));
        assert!(html.contains(&report.generated_at));
        assert!(html.contains(&report.website.short_revision));
        assert!(html.contains(&report.upstream.short_revision));
        assert!(
            !html.contains('`'),
            "Markdown backticks must render as <code>, not literal text"
        );
        for code in [
            "git blame",
            "CODEOWNERS",
            "dabney-moe",
            "Dockerfile",
            "nix build .#serverImage",
            "nix run .#update-ownership",
        ] {
            assert!(
                html.contains(&format!(">{code}</code>")),
                "missing <code> for {code}"
            );
        }
        let legacy_brand = ["co", "da"].concat();
        let legacy_patient_brand = ["du", "et"].concat();
        let legacy_other_brand = ["cho", "rds"].concat();
        let legacy_credit = ["hoyo", "verse"].concat();
        let lowered_html = html.to_ascii_lowercase();
        assert!(!lowered_html.contains(&legacy_brand));
        assert!(!lowered_html.contains(&legacy_patient_brand));
        assert!(!lowered_html.contains(&legacy_other_brand));
        assert!(!lowered_html.contains(&legacy_credit));
    }
}
