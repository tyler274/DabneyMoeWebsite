//! SVG charts for portfolio impact visualization.

use leptos::prelude::*;

use crate::portfolio::PortfolioImpact;

const CYAN: &str = "#22d3ee";
const AMBER: &str = "#fbbf24";
const SLATE: &str = "#64748b";
const SLATE_LIGHT: &str = "#94a3b8";

#[component]
pub fn PortfolioImpactCharts(impact: PortfolioImpact) -> impl IntoView {
    view! {
        <div class="space-y-6">
            <PortfolioValueChart impact=impact.clone() />
            <FundWeightChart impact=impact />
        </div>
    }
}

#[component]
fn PortfolioValueChart(impact: PortfolioImpact) -> impl IntoView {
    let max_value = impact
        .total_before
        .max(impact.total_after)
        .max(impact.cash_before)
        .max(impact.cash_after);
    let scale = if max_value > 0.0 {
        200.0 / max_value
    } else {
        0.0
    };

    let before_h = impact.total_before * scale;
    let after_h = impact.total_after * scale;
    let before_y = 240.0 - before_h;
    let after_y = 240.0 - after_h;
    let delta = impact.total_after - impact.total_before;
    let delta_label = format!("{:+}", format_usd_compact(delta));
    let delta_color = if delta >= 0.0 { CYAN } else { AMBER };

    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <h2 class="mb-1 text-lg font-semibold text-white">"Portfolio value"</h2>
            <p class="mb-6 text-sm text-slate-400">
                "Total portfolio (funds + cash) before and after the planned trades."
            </p>
            <svg viewBox="0 0 360 280" class="mx-auto w-full max-w-md" role="img" aria-label="Portfolio value before and after">
                <text x="90" y="24" text-anchor="middle" fill=SLATE_LIGHT font-size="12">"Before"</text>
                <text x="270" y="24" text-anchor="middle" fill=SLATE_LIGHT font-size="12">"After"</text>

                <rect x="40" y=before_y width="100" height=before_h rx="4" fill=SLATE />
                <rect x="220" y=after_y width="100" height=after_h rx="4" fill=CYAN />

                <text x="90" y="255" text-anchor="middle" fill=SLATE_LIGHT font-size="11">
                    {format_usd_compact(impact.total_before)}
                </text>
                <text x="270" y="255" text-anchor="middle" fill=SLATE_LIGHT font-size="11">
                    {format_usd_compact(impact.total_after)}
                </text>

                <text x="180" y="275" text-anchor="middle" fill=delta_color font-size="12">
                    {delta_label}
                </text>
            </svg>

            <div class="mt-4 flex flex-wrap justify-center gap-6 text-xs text-slate-400">
                <span class="flex items-center gap-2">
                    <span class="inline-block h-3 w-3 rounded-sm bg-slate-500"></span>
                    "Before"
                </span>
                <span class="flex items-center gap-2">
                    <span class="inline-block h-3 w-3 rounded-sm bg-cyan-400"></span>
                    "After"
                </span>
            </div>
        </div>
    }
}

#[component]
fn FundWeightChart(impact: PortfolioImpact) -> impl IntoView {
    let row_height = 28.0;
    let chart_height = impact.funds.len() as f64 * row_height + 40.0;
    let bar_max_width = 220.0;

    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <h2 class="mb-1 text-lg font-semibold text-white">"Fund weights"</h2>
            <p class="mb-6 text-sm text-slate-400">
                "Each fund's share of the portfolio before (gray) and after (cyan) trades."
            </p>
            <div class="overflow-x-auto">
                <svg
                    viewBox=format!("0 0 420 {chart_height}")
                    class="w-full min-w-[360px]"
                    role="img"
                    aria-label="Fund weight comparison"
                >
                    {impact
                        .funds
                        .iter()
                        .enumerate()
                        .map(|(index, fund)| {
                            let y = 30.0 + index as f64 * row_height;
                            let before_w = fund.weight_before / 100.0 * bar_max_width;
                            let after_w = fund.weight_after / 100.0 * bar_max_width;
                            let label = if fund.symbol.len() > 8 {
                                fund.symbol.clone()
                            } else {
                                fund.symbol.clone()
                            };
                            view! {
                                <g>
                                    <text x="0" y=y + 10.0 fill=SLATE_LIGHT font-size="11">{label}</text>
                                    <rect
                                        x="70"
                                        y=y
                                        width=before_w
                                        height="10"
                                        rx="2"
                                        fill=SLATE
                                    />
                                    <rect
                                        x="70"
                                        y=y + 14.0
                                        width=after_w
                                        height="10"
                                        rx="2"
                                        fill=CYAN
                                    />
                                    <text
                                        x=70.0 + bar_max_width + 8.0
                                        y=y + 16.0
                                        fill=SLATE_LIGHT
                                        font-size="10"
                                    >
                                        {format!(
                                            "{:.1}% → {:.1}%",
                                            fund.weight_before,
                                            fund.weight_after
                                        )}
                                    </text>
                                </g>
                            }
                        })
                        .collect_view()}
                </svg>
            </div>
            <div class="mt-4 flex flex-wrap justify-center gap-6 text-xs text-slate-400">
                <span class="flex items-center gap-2">
                    <span class="inline-block h-2 w-6 rounded-sm bg-slate-500"></span>
                    "Before"
                </span>
                <span class="flex items-center gap-2">
                    <span class="inline-block h-2 w-6 rounded-sm bg-cyan-400"></span>
                    "After"
                </span>
            </div>
        </div>
    }
}

fn format_usd_compact(amount: f64) -> String {
    let sign = if amount < 0.0 { "-" } else { "" };
    let abs = amount.abs();
    if abs >= 1_000_000.0 {
        format!("{sign}${:.1}M", abs / 1_000_000.0)
    } else if abs >= 10_000.0 {
        format!("{sign}${:.0}k", abs / 1_000.0)
    } else {
        format!("{sign}${abs:.0}")
    }
}
