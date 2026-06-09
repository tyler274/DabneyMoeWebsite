//! SVG charts for portfolio impact visualization.

use leptos::prelude::*;

use crate::portfolio::PortfolioImpact;

const CYAN: &str = "#22d3ee";
const AMBER: &str = "#fbbf24";
const SLATE: &str = "#64748b";
const SLATE_LIGHT: &str = "#94a3b8";
const EMERALD: &str = "#34d399";
const ROSE: &str = "#fb7185";

#[component]
pub fn PortfolioImpactCharts(impact: PortfolioImpact) -> impl IntoView {
    view! {
        <div class="space-y-6">
            <PortfolioCompositionChart impact=impact.clone() />
            <FundValueChart impact=impact.clone() />
            <FundWeightChart impact=impact />
        </div>
    }
}

#[component]
fn PortfolioCompositionChart(impact: PortfolioImpact) -> impl IntoView {
    let max_total = impact.total_before.max(impact.total_after);
    let scale = if max_total > 0.0 {
        200.0 / max_total
    } else {
        0.0
    };

    let before_fund_h = impact.fund_value_before * scale;
    let before_cash_h = impact.cash_before * scale;
    let after_fund_h = impact.fund_value_after * scale;
    let after_cash_h = impact.cash_after * scale;

    let before_fund_y = 240.0 - before_fund_h - before_cash_h;
    let before_cash_y = 240.0 - before_cash_h;
    let after_fund_y = 240.0 - after_fund_h - after_cash_h;
    let after_cash_y = 240.0 - after_cash_h;

    let fund_delta = impact.fund_value_after - impact.fund_value_before;
    let cash_delta = impact.cash_after - impact.cash_before;

    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <h2 class="mb-1 text-lg font-semibold text-white">"Portfolio composition"</h2>
            <p class="mb-6 text-sm text-slate-400">
                "Invested funds (cyan) and cash (amber) before and after the planned trades."
            </p>
            <svg viewBox="0 0 360 300" class="mx-auto w-full max-w-md" role="img" aria-label="Portfolio composition before and after">
                <text x="90" y="24" text-anchor="middle" fill=SLATE_LIGHT font-size="12">"Before"</text>
                <text x="270" y="24" text-anchor="middle" fill=SLATE_LIGHT font-size="12">"After"</text>

                <rect x="40" y=before_fund_y width="100" height=before_fund_h rx="4" fill=CYAN />
                <rect x="40" y=before_cash_y width="100" height=before_cash_h rx="4" fill=AMBER />
                <rect x="220" y=after_fund_y width="100" height=after_fund_h rx="4" fill=CYAN />
                <rect x="220" y=after_cash_y width="100" height=after_cash_h rx="4" fill=AMBER />

                <text x="90" y="255" text-anchor="middle" fill=SLATE_LIGHT font-size="11">
                    {format_usd_compact(impact.total_before)}
                </text>
                <text x="270" y="255" text-anchor="middle" fill=SLATE_LIGHT font-size="11">
                    {format_usd_compact(impact.total_after)}
                </text>

                <text x="180" y="275" text-anchor="middle" fill=SLATE_LIGHT font-size="11">
                    {format!(
                        "Funds {:+} · Cash {:+}",
                        format_usd_compact(fund_delta),
                        format_usd_compact(cash_delta)
                    )}
                </text>
            </svg>

            <div class="mt-4 flex flex-wrap justify-center gap-6 text-xs text-slate-400">
                <span class="flex items-center gap-2">
                    <span class="inline-block h-3 w-3 rounded-sm bg-cyan-400"></span>
                    "Invested funds"
                </span>
                <span class="flex items-center gap-2">
                    <span class="inline-block h-3 w-3 rounded-sm bg-amber-400"></span>
                    "Cash"
                </span>
            </div>
        </div>
    }
}

#[component]
fn FundValueChart(impact: PortfolioImpact) -> impl IntoView {
    let row_height = 32.0;
    let chart_height = impact.funds.len() as f64 * row_height + 40.0;
    let max_value = impact
        .funds
        .iter()
        .flat_map(|f| [f.value_before, f.value_after])
        .fold(0.0_f64, f64::max);
    let bar_max_width = 200.0;
    let scale = if max_value > 0.0 {
        bar_max_width / max_value
    } else {
        0.0
    };

    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <h2 class="mb-1 text-lg font-semibold text-white">"Fund values"</h2>
            <p class="mb-6 text-sm text-slate-400">
                "Dollar value per fund before (gray) and after (cyan) trades."
            </p>
            <div class="overflow-x-auto">
                <svg
                    viewBox=format!("0 0 440 {chart_height}")
                    class="w-full min-w-[380px]"
                    role="img"
                    aria-label="Fund dollar value comparison"
                >
                    {impact
                        .funds
                        .iter()
                        .enumerate()
                        .map(|(index, fund)| {
                            let y = 30.0 + index as f64 * row_height;
                            let before_w = fund.value_before * scale;
                            let after_w = fund.value_after * scale;
                            let delta = fund.value_after - fund.value_before;
                            let delta_color = if delta >= 0.0 { EMERALD } else { ROSE };
                            view! {
                                <g>
                                    <text x="0" y=y + 10.0 fill=SLATE_LIGHT font-size="11">{fund.symbol.clone()}</text>
                                    <rect x="70" y=y width=before_w height="10" rx="2" fill=SLATE />
                                    <rect x="70" y=y + 14.0 width=after_w height="10" rx="2" fill=CYAN />
                                    <text
                                        x=70.0 + bar_max_width + 8.0
                                        y=y + 16.0
                                        fill=delta_color
                                        font-size="10"
                                    >
                                        {format!(
                                            "{} → {} ({:+})",
                                            format_usd_compact(fund.value_before),
                                            format_usd_compact(fund.value_after),
                                            format_usd_compact(delta)
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

#[component]
fn FundWeightChart(impact: PortfolioImpact) -> impl IntoView {
    let row_height = 28.0;
    let chart_height = impact.funds.len() as f64 * row_height + 40.0;
    let bar_max_width = 220.0;

    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <h2 class="mb-1 text-lg font-semibold text-white">"Fund weights"</h2>
            <p class="mb-6 text-sm text-slate-400">
                "Each fund's share of the total portfolio before (gray) and after (cyan) trades."
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
                            let delta = fund.weight_after - fund.weight_before;
                            let delta_color = if delta >= 0.0 { EMERALD } else { ROSE };
                            view! {
                                <g>
                                    <text x="0" y=y + 10.0 fill=SLATE_LIGHT font-size="11">{fund.symbol.clone()}</text>
                                    <rect x="70" y=y width=before_w height="10" rx="2" fill=SLATE />
                                    <rect x="70" y=y + 14.0 width=after_w height="10" rx="2" fill=CYAN />
                                    <text
                                        x=70.0 + bar_max_width + 8.0
                                        y=y + 16.0
                                        fill=delta_color
                                        font-size="10"
                                    >
                                        {format!(
                                            "{:.1}% → {:.1}% ({:+.1}%)",
                                            fund.weight_before,
                                            fund.weight_after,
                                            delta
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
