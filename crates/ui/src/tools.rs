//! Client-side financial tools (portfolio CSV stays in the browser).

use leptos::prelude::*;
use leptos_router::components::A;

use crate::portfolio::{
    analyze_buy, analyze_portfolio, analyze_rebalance, parse_deposit, parse_withdrawal,
    parse_withholding_pct, AccountType, BuyReport, PortfolioImpact, RebalanceReport, SellReport,
    TaxWithholding, TradeSide,
};
use crate::portfolio_chart::PortfolioImpactCharts;
use crate::sections::Footer;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalculatorMode {
    Sell,
    Buy,
    Rebalance,
}

fn fmt_usd(amount: f64) -> String {
    let sign = if amount < 0.0 { "-" } else { "" };
    let abs = amount.abs();
    let dollars = abs.floor() as i64;
    let cents = ((abs - dollars as f64) * 100.0).round() as i64;
    let dollars_str = dollars
        .to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join(","))
        .unwrap_or_else(|_| dollars.to_string());
    format!("{sign}${dollars_str}.{cents:02}")
}

#[component]
pub fn ToolsMenu() -> impl IntoView {
    let open = RwSignal::new(false);
    let toggle = move |_| open.update(|value| *value = !*value);
    let close = move |_| open.set(false);

    view! {
        <div class="relative">
            <button
                type="button"
                class="flex items-center gap-1 transition hover:text-white"
                aria-expanded=move || open.get()
                aria-haspopup="true"
                on:click=toggle
            >
                "Tools"
                <span class="text-[10px] opacity-70">{move || if open.get() { "▲" } else { "▼" }}</span>
            </button>
            <Show when=move || open.get()>
                <div
                    class="absolute right-0 z-50 mt-2 min-w-56 rounded-xl border border-white/10 bg-slate-900 py-2 shadow-xl"
                    role="menu"
                >
                    <A href="/tools/investment-account" on:click=close>
                        <span class="block px-4 py-2 text-slate-200 transition hover:bg-white/5 hover:text-white" role="menuitem">
                            "Investment Account Calculator"
                        </span>
                    </A>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn InvestmentAccountPage() -> impl IntoView {
    use crate::sections::NavBar;

    // Type-erase each child into `AnyView` for the same reason as `HomePage`:
    // the combined view tuple of NavBar + the calculator section + Footer is
    // deep enough (after ToolsMenu closures and the many Show branches inside
    // InvestmentAccountCalculator) to overflow rustc's type-layout recursion
    // limit in the release SSR binary.
    vec![
        view! { <NavBar /> }.into_any(),
        view! {
            <section class="border-b border-white/5 px-6 py-20">
                <div class="mx-auto max-w-4xl">
                    <p class="mb-2 text-sm font-semibold uppercase tracking-widest text-cyan-400">
                        "Tools"
                    </p>
                    <h1 class="mb-3 text-3xl font-bold tracking-tight text-white sm:text-4xl">
                        "Investment Account Calculator"
                    </h1>
                    <p class="mb-10 max-w-2xl text-lg leading-relaxed text-slate-300">
                        "Upload a Raymond James portfolio CSV. Choose sell, buy, or quarterly rebalance—"
                        "everything runs in your browser and your file never leaves this device."
                    </p>
                    <InvestmentAccountCalculator />
                </div>
            </section>
        }
        .into_any(),
        view! { <Footer /> }.into_any(),
    ]
}

/// Backward-compatible alias for the sell-only page export.
#[component]
pub fn IraSellPage() -> impl IntoView {
    view! { <InvestmentAccountPage /> }
}

#[component]
pub fn InvestmentAccountCalculator() -> impl IntoView {
    let mode = RwSignal::new(CalculatorMode::Sell);
    let account_type = RwSignal::new(AccountType::TraditionalIra);
    let withholding_enabled = RwSignal::new(true);
    let federal_pct = RwSignal::new("20".to_string());
    let state_pct = RwSignal::new("7".to_string());
    let csv_text = RwSignal::new(None::<String>);
    let file_name = RwSignal::new(None::<String>);
    let amount = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let sell_report = RwSignal::new(None::<SellReport>);
    let buy_report = RwSignal::new(None::<BuyReport>);
    let rebalance_report = RwSignal::new(None::<RebalanceReport>);

    let recompute = move || {
        error.set(None);
        sell_report.set(None);
        buy_report.set(None);
        rebalance_report.set(None);

        let Some(text) = csv_text.get() else {
            return;
        };

        match mode.get() {
            CalculatorMode::Sell => {
                let value = match parse_withdrawal(&amount.get()) {
                    Ok(v) => v,
                    Err(message) => {
                        if !amount.get().trim().is_empty() {
                            error.set(Some(message));
                        }
                        return;
                    }
                };

                let acct = account_type.get();
                let (net_withdrawal, withholding) = if acct.is_ira() && withholding_enabled.get() {
                    let federal = match parse_withholding_pct(&federal_pct.get(), "federal") {
                        Ok(v) => v,
                        Err(message) => {
                            error.set(Some(message));
                            return;
                        }
                    };
                    let state = match parse_withholding_pct(&state_pct.get(), "state") {
                        Ok(v) => v,
                        Err(message) => {
                            error.set(Some(message));
                            return;
                        }
                    };
                    let rates = TaxWithholding {
                        federal_pct: federal,
                        state_pct: state,
                    };
                    if rates.total_pct() >= 100.0 {
                        error.set(Some(
                            "Combined federal and state withholding must be less than 100%.".into(),
                        ));
                        return;
                    }
                    (Some(value), Some(rates))
                } else {
                    (None, None)
                };

                match analyze_portfolio(&text, acct, value, net_withdrawal, withholding) {
                    Ok(result) => sell_report.set(Some(result)),
                    Err(message) => error.set(Some(message)),
                }
            }
            CalculatorMode::Buy => {
                let value = match parse_deposit(&amount.get()) {
                    Ok(v) => v,
                    Err(message) => {
                        if !amount.get().trim().is_empty() {
                            error.set(Some(message));
                        }
                        return;
                    }
                };
                match analyze_buy(&text, value) {
                    Ok(result) => buy_report.set(Some(result)),
                    Err(message) => error.set(Some(message)),
                }
            }
            CalculatorMode::Rebalance => match analyze_rebalance(&text) {
                Ok(result) => rebalance_report.set(Some(result)),
                Err(message) => error.set(Some(message)),
            },
        }
    };

    Effect::new(move |_| {
        let _ = csv_text.get();
        let _ = amount.get();
        let _ = mode.get();
        let _ = account_type.get();
        let _ = withholding_enabled.get();
        let _ = federal_pct.get();
        let _ = state_pct.get();
        recompute();
    });

    let set_mode = move |next: CalculatorMode| {
        move |_| {
            mode.set(next);
            amount.set(String::new());
        }
    };

    let on_amount_input = move |ev: leptos::ev::Event| {
        amount.set(event_target_value(&ev));
    };

    let on_federal_input = move |ev: leptos::ev::Event| {
        federal_pct.set(event_target_value(&ev));
    };

    let on_state_input = move |ev: leptos::ev::Event| {
        state_pct.set(event_target_value(&ev));
    };

    let apply_ny_preset = move |_| {
        federal_pct.set("20".into());
        state_pct.set("7".into());
    };

    #[cfg(any(feature = "hydrate", feature = "csr"))]
    let on_file_change = move |ev: leptos::ev::Event| {
        error.set(None);
        let input = event_target::<web_sys::HtmlInputElement>(&ev);
        let Some(file_list) = input.files() else {
            return;
        };
        let Some(file) = file_list.get(0) else {
            csv_text.set(None);
            file_name.set(None);
            return;
        };
        file_name.set(Some(file.name()));
        read_csv_file(file, csv_text, error);
    };

    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    let on_file_change = move |_ev: leptos::ev::Event| {};

    view! {
        <div class="space-y-8">
            <div class="flex flex-wrap gap-2">
                <ModeTab label="Sell / Withdraw" active=move || mode.get() == CalculatorMode::Sell on_click=set_mode(CalculatorMode::Sell) />
                <ModeTab label="Buy / Deposit" active=move || mode.get() == CalculatorMode::Buy on_click=set_mode(CalculatorMode::Buy) />
                <ModeTab label="Rebalance" active=move || mode.get() == CalculatorMode::Rebalance on_click=set_mode(CalculatorMode::Rebalance) />
            </div>

            <div class="grid gap-6 rounded-2xl border border-white/10 bg-white/[0.02] p-6 sm:grid-cols-2">
                <div>
                    <label class="mb-2 block text-sm font-medium text-slate-200" for="account-type">
                        "Account type"
                    </label>
                    <select
                        id="account-type"
                        class="w-full rounded-lg border border-white/10 bg-slate-950 px-3 py-2 text-slate-100 focus:border-cyan-400/50 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                        prop:value=move || match account_type.get() {
                            AccountType::Taxable => "taxable",
                            AccountType::RothIra => "roth",
                            AccountType::TraditionalIra => "traditional",
                        }
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            account_type.set(match value.as_str() {
                                "taxable" => AccountType::Taxable,
                                "roth" => AccountType::RothIra,
                                _ => AccountType::TraditionalIra,
                            });
                        }
                    >
                        <option value="traditional">"Traditional IRA"</option>
                        <option value="roth">"Roth IRA"</option>
                        <option value="taxable">"Taxable brokerage"</option>
                    </select>
                    <p class="mt-2 text-xs text-slate-500">
                        {move || match account_type.get() {
                            AccountType::Taxable =>
                                "No tax withholding on sales—proceeds go directly to cash.",
                            AccountType::TraditionalIra =>
                                "Distributions may have federal and state tax withheld at source.",
                            AccountType::RothIra =>
                                "Qualified Roth distributions are typically tax-free; withholding optional for non-qualified.",
                        }}
                    </p>
                </div>

                <Show when=move || mode.get() == CalculatorMode::Sell && account_type.get().is_ira()>
                    <div class="space-y-3">
                        <label class="flex items-center gap-2 text-sm font-medium text-slate-200">
                            <input
                                type="checkbox"
                                class="rounded border-white/20 bg-slate-950 text-cyan-400 focus:ring-cyan-400/50"
                                prop:checked=move || withholding_enabled.get()
                                on:change=move |ev| {
                                    withholding_enabled.set(event_target_checked(&ev));
                                }
                            />
                            "Account for tax withholding"
                        </label>
                        <Show when=move || withholding_enabled.get()>
                            <div class="grid gap-3 sm:grid-cols-2">
                                <div>
                                    <label class="mb-1 block text-xs text-slate-400" for="federal-pct">
                                        "Federal withholding %"
                                    </label>
                                    <input
                                        id="federal-pct"
                                        type="text"
                                        inputmode="decimal"
                                        prop:value=move || federal_pct.get()
                                        class="w-full rounded-lg border border-white/10 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-cyan-400/50 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                                        on:input=on_federal_input
                                    />
                                </div>
                                <div>
                                    <label class="mb-1 block text-xs text-slate-400" for="state-pct">
                                        "State withholding %"
                                    </label>
                                    <input
                                        id="state-pct"
                                        type="text"
                                        inputmode="decimal"
                                        prop:value=move || state_pct.get()
                                        class="w-full rounded-lg border border-white/10 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-cyan-400/50 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                                        on:input=on_state_input
                                    />
                                </div>
                            </div>
                            <button
                                type="button"
                                class="text-xs text-cyan-400 transition hover:text-cyan-300"
                                on:click=apply_ny_preset
                            >
                                "Use New York defaults (20% federal, 7% state)"
                            </button>
                        </Show>
                    </div>
                </Show>

                <div class="sm:col-span-2 sm:max-w-md">
                    <label class="mb-2 block text-sm font-medium text-slate-200" for="portfolio-csv">
                        "Portfolio CSV"
                    </label>
                    <input
                        id="portfolio-csv"
                        type="file"
                        accept=".csv,text/csv"
                        class="block w-full cursor-pointer rounded-lg border border-white/10 bg-slate-950 px-3 py-2 text-sm text-slate-300 file:mr-3 file:rounded-md file:border-0 file:bg-cyan-500/20 file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-cyan-200 hover:file:bg-cyan-500/30"
                        on:change=on_file_change
                    />
                    <p class="mt-2 text-xs text-slate-500">
                        {move || {
                            file_name
                                .get()
                                .map(|name| format!("Loaded: {name}"))
                                .unwrap_or_else(|| "Raymond James export with SYMBOL/CUSIP columns.".to_string())
                        }}
                    </p>
                </div>

                <Show when=move || mode.get() != CalculatorMode::Rebalance>
                    <div>
                        <label class="mb-2 block text-sm font-medium text-slate-200" for="amount">
                            {move || match mode.get() {
                                CalculatorMode::Sell if account_type.get().is_ira() && withholding_enabled.get() =>
                                    "Target net withdrawal",
                                CalculatorMode::Sell => "Target withdrawal",
                                CalculatorMode::Buy => "Deposit amount",
                                CalculatorMode::Rebalance => "Amount",
                            }}
                        </label>
                        <input
                            id="amount"
                            type="text"
                            inputmode="decimal"
                            placeholder="10000"
                            prop:value=move || amount.get()
                            class="w-full rounded-lg border border-white/10 bg-slate-950 px-3 py-2 text-slate-100 placeholder:text-slate-600 focus:border-cyan-400/50 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                            on:input=on_amount_input
                        />
                        <p class="mt-2 text-xs text-slate-500">
                            {move || match mode.get() {
                                CalculatorMode::Sell if account_type.get().is_ira() && withholding_enabled.get() =>
                                    "Cash you want after federal and state withholding. Gross sell amount is computed automatically.",
                                CalculatorMode::Sell => "Whole-share sells that preserve fund weights.",
                                CalculatorMode::Buy => "Whole-share buys that preserve fund weights.",
                                CalculatorMode::Rebalance => "",
                            }}
                        </p>
                    </div>
                </Show>

                <Show when=move || mode.get() == CalculatorMode::Rebalance>
                    <div class="flex items-center text-sm leading-relaxed text-slate-400">
                        "Deploys idle cash (RJBDP) into funds proportionally so each fund matches its "
                        "target share of the total portfolio. No amount needed—the CSV cash balance is used."
                    </div>
                </Show>
            </div>

            <Show when=move || error.get().is_some()>
                <div class="rounded-xl border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-200">
                    {move || error.get().unwrap_or_default()}
                </div>
            </Show>

            <Show when=move || sell_report.get().is_some()>
                {move || sell_report.get().map(|data| view! { <SellReportView report=data /> })}
            </Show>
            <Show when=move || buy_report.get().is_some()>
                {move || buy_report.get().map(|data| view! { <BuyReportView report=data /> })}
            </Show>
            <Show when=move || rebalance_report.get().is_some()>
                {move || rebalance_report.get().map(|data| view! { <RebalanceReportView report=data /> })}
            </Show>
        </div>
    }
}

#[component]
fn ModeTab<F, C>(label: &'static str, active: F, on_click: C) -> impl IntoView
where
    F: Fn() -> bool + Send + Sync + 'static,
    C: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
{
    view! {
        <button
            type="button"
            class=move || {
                if active() {
                    "rounded-full bg-cyan-500 px-4 py-1.5 text-sm font-medium text-slate-950 transition"
                } else {
                    "rounded-full border border-white/10 px-4 py-1.5 text-sm font-medium text-slate-300 transition hover:border-white/30 hover:text-white"
                }
            }
            on:click=on_click
        >
            {label}
        </button>
    }
}

#[component]
fn SellReportView(report: SellReport) -> impl IntoView {
    let total_proceeds = report.total_proceeds();
    let shortfall = report.shortfall();
    let impact = PortfolioImpact::from_sell(&report);
    let withholding = report.withholding;
    let account_label = report.account_type.label();

    view! {
        <div class="space-y-6">
            <p class="text-sm text-slate-400">
                "Account: "
                <span class="font-medium text-slate-200">{account_label}</span>
            </p>

            <div class="grid gap-4 rounded-2xl border border-white/10 bg-white/[0.02] p-6 sm:grid-cols-2 lg:grid-cols-4">
                <SummaryStat label="Total portfolio" value=fmt_usd(report.total_portfolio()) />
                <SummaryStat label="Cash / RJBDP" value=fmt_usd(report.cash_value) />
                <SummaryStat label="Invested funds" value=fmt_usd(report.total_fund_value()) />
                <SummaryStat
                    label=if withholding.is_some() { "Gross distribution" } else { "Target withdrawal" }
                    value=fmt_usd(report.withdrawal)
                />
            </div>

            <Show when=move || withholding.is_some()>
                {move || {
                    let w = withholding.unwrap();
                    view! {
                        <div class="grid gap-4 rounded-2xl border border-amber-400/20 bg-amber-400/5 p-6 sm:grid-cols-2 lg:grid-cols-4">
                            <SummaryStat label="Federal withheld" value=fmt_usd(w.federal) />
                            <SummaryStat label="State withheld" value=fmt_usd(w.state) />
                            <SummaryStat label="Net to you" value=fmt_usd(w.net) />
                            <SummaryStat label="Gross sell proceeds" value=fmt_usd(total_proceeds) />
                        </div>
                    }
                }}
            </Show>

            <PortfolioImpactCharts impact=impact />

            <PlanTable
                headers=&["Fund", "Sym", "Weight", "Sell", "@ Price", "Proceeds", "Δ Weight"]
                rows=report
                    .plan
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        view! {
                            <tr>
                                <td class="px-4 py-3">{line.holding.description.clone()}</td>
                                <td class="px-4 py-3 font-medium text-cyan-300">{line.holding.symbol.clone()}</td>
                                <td class="px-4 py-3 text-right">{format!("{:.2}%", report.weight_before(index))}</td>
                                <td class="px-4 py-3 text-right">{line.shares_to_sell}</td>
                                <td class="px-4 py-3 text-right">{fmt_usd(line.holding.price)}</td>
                                <td class="px-4 py-3 text-right">{fmt_usd(line.proceeds)}</td>
                                <td class="px-4 py-3 text-right">{format!("{:+.2}%", report.weight_delta(index))}</td>
                            </tr>
                        }
                    })
                    .collect_view()
            />

            <TradeInstructions>
                {report
                    .plan
                    .iter()
                    .map(|line| {
                        if line.shares_to_sell > 0 {
                            view! {
                                <li>
                                    "SELL "
                                    <span class="text-white">{line.shares_to_sell}</span>
                                    " shares "
                                    <span class="text-cyan-300">{line.holding.symbol.clone()}</span>
                                    " (~" {fmt_usd(line.proceeds)} ")"
                                </li>
                            }
                            .into_any()
                        } else {
                            view! {
                                <li class="text-slate-500">
                                    "SKIP 0 shares "
                                    <span class="text-slate-400">{line.holding.symbol.clone()}</span>
                                    " (allocation too small)"
                                </li>
                            }
                            .into_any()
                        }
                    })
                    .collect_view()}
            </TradeInstructions>

            <FooterRow label="Total proceeds" value=fmt_usd(total_proceeds) />
            <Show when=move || { shortfall.abs() > 0.005 }>
                <FooterRow label="Rounding shortfall (keep as cash)" value=fmt_usd(shortfall) muted=true />
            </Show>
        </div>
    }
}

#[component]
fn BuyReportView(report: BuyReport) -> impl IntoView {
    let total_cost = report.total_cost();
    let leftover = report.leftover();
    let impact = PortfolioImpact::from_buy(&report);

    view! {
        <div class="space-y-6">
            <div class="grid gap-4 rounded-2xl border border-white/10 bg-white/[0.02] p-6 sm:grid-cols-2 lg:grid-cols-4">
                <SummaryStat label="Total portfolio" value=fmt_usd(report.total_portfolio()) />
                <SummaryStat label="Cash / RJBDP" value=fmt_usd(report.cash_value) />
                <SummaryStat label="Invested funds" value=fmt_usd(report.total_fund_value()) />
                <SummaryStat label="Deposit amount" value=fmt_usd(report.deposit) />
            </div>

            <PortfolioImpactCharts impact=impact />

            <PlanTable
                headers=&["Fund", "Sym", "Weight", "Buy", "@ Price", "Cost", "Δ Weight"]
                rows=report
                    .plan
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        view! {
                            <tr>
                                <td class="px-4 py-3">{line.holding.description.clone()}</td>
                                <td class="px-4 py-3 font-medium text-cyan-300">{line.holding.symbol.clone()}</td>
                                <td class="px-4 py-3 text-right">{format!("{:.2}%", report.weight_before(index))}</td>
                                <td class="px-4 py-3 text-right">{line.shares_to_buy}</td>
                                <td class="px-4 py-3 text-right">{fmt_usd(line.holding.price)}</td>
                                <td class="px-4 py-3 text-right">{fmt_usd(line.cost)}</td>
                                <td class="px-4 py-3 text-right">{format!("{:+.2}%", report.weight_delta(index))}</td>
                            </tr>
                        }
                    })
                    .collect_view()
            />

            <TradeInstructions>
                {report
                    .plan
                    .iter()
                    .map(|line| {
                        if line.shares_to_buy > 0 {
                            view! {
                                <li>
                                    "BUY "
                                    <span class="text-white">{line.shares_to_buy}</span>
                                    " shares "
                                    <span class="text-cyan-300">{line.holding.symbol.clone()}</span>
                                    " (~" {fmt_usd(line.cost)} ")"
                                </li>
                            }
                            .into_any()
                        } else {
                            view! {
                                <li class="text-slate-500">
                                    "SKIP 0 shares "
                                    <span class="text-slate-400">{line.holding.symbol.clone()}</span>
                                    " (allocation too small)"
                                </li>
                            }
                            .into_any()
                        }
                    })
                    .collect_view()}
            </TradeInstructions>

            <FooterRow label="Total cost" value=fmt_usd(total_cost) />
            <Show when=move || { leftover.abs() > 0.005 }>
                <FooterRow label="Uninvested remainder (keep as cash)" value=fmt_usd(leftover) muted=true />
            </Show>
        </div>
    }
}

#[component]
fn RebalanceReportView(report: RebalanceReport) -> impl IntoView {
    let cash_leftover = report.cash_leftover();
    let impact = PortfolioImpact::from_rebalance(&report);

    view! {
        <div class="space-y-6">
            <div class="grid gap-4 rounded-2xl border border-white/10 bg-white/[0.02] p-6 sm:grid-cols-2 lg:grid-cols-4">
                <SummaryStat label="Total portfolio" value=fmt_usd(report.total_portfolio()) />
                <SummaryStat label="Cash to deploy" value=fmt_usd(report.cash_value) />
                <SummaryStat label="Invested funds" value=fmt_usd(report.total_fund_value()) />
                <SummaryStat label="Total buys" value=fmt_usd(report.total_buys()) />
            </div>

            <PortfolioImpactCharts impact=impact />

            <PlanTable
                headers=&["Fund", "Sym", "Weight", "Action", "Shares", "Amount", "Target %"]
                rows=report
                    .plan
                    .iter()
                    .map(|line| {
                        let target_pct = if report.total_portfolio() > 0.0 {
                            line.target_value / report.total_portfolio() * 100.0
                        } else {
                            0.0
                        };
                        let action = match line.side {
                            TradeSide::Buy => "BUY",
                            TradeSide::Sell => "SELL",
                            TradeSide::Hold => "HOLD",
                        };
                        view! {
                            <tr>
                                <td class="px-4 py-3">{line.holding.description.clone()}</td>
                                <td class="px-4 py-3 font-medium text-cyan-300">{line.holding.symbol.clone()}</td>
                                <td class="px-4 py-3 text-right">{format!("{:.2}%", line.weight_before)}</td>
                                <td class="px-4 py-3">{action}</td>
                                <td class="px-4 py-3 text-right">{line.shares}</td>
                                <td class="px-4 py-3 text-right">{fmt_usd(line.amount)}</td>
                                <td class="px-4 py-3 text-right">{format!("{target_pct:.2}%")}</td>
                            </tr>
                        }
                    })
                    .collect_view()
            />

            <TradeInstructions>
                {report
                    .plan
                    .iter()
                    .map(|line| match line.side {
                        TradeSide::Buy => {
                            view! {
                                <li>
                                    "BUY "
                                    <span class="text-white">{line.shares}</span>
                                    " shares "
                                    <span class="text-cyan-300">{line.holding.symbol.clone()}</span>
                                    " (~" {fmt_usd(line.amount)} ")"
                                </li>
                            }
                            .into_any()
                        }
                        TradeSide::Sell => {
                            view! {
                                <li>
                                    "SELL "
                                    <span class="text-white">{line.shares}</span>
                                    " shares "
                                    <span class="text-cyan-300">{line.holding.symbol.clone()}</span>
                                    " (~" {fmt_usd(line.amount)} ")"
                                </li>
                            }
                            .into_any()
                        }
                        TradeSide::Hold => {
                            view! {
                                <li class="text-slate-500">
                                    "HOLD "
                                    <span class="text-slate-400">{line.holding.symbol.clone()}</span>
                                    " (at target weight)"
                                </li>
                            }
                            .into_any()
                        }
                    })
                    .collect_view()}
            </TradeInstructions>

            <FooterRow label="Cash remaining after rebalance" value=fmt_usd(cash_leftover) muted=true />
        </div>
    }
}

#[component]
fn PlanTable(headers: &'static [&'static str], rows: impl IntoView) -> impl IntoView {
    view! {
        <div class="overflow-x-auto rounded-2xl border border-white/10">
            <table class="min-w-full text-left text-sm">
                <thead class="border-b border-white/10 bg-white/[0.03] text-xs uppercase tracking-wide text-slate-400">
                    <tr>
                        {headers
                            .iter()
                            .map(|h| view! { <th class="px-4 py-3">{*h}</th> })
                            .collect_view()}
                    </tr>
                </thead>
                <tbody class="divide-y divide-white/5 text-slate-200">{rows}</tbody>
            </table>
        </div>
    }
}

#[component]
fn TradeInstructions(children: Children) -> impl IntoView {
    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <h2 class="mb-4 text-lg font-semibold text-white">"Trade instructions"</h2>
            <ul class="space-y-2 font-mono text-sm text-slate-300">{children()}</ul>
        </div>
    }
}

#[component]
fn FooterRow(
    label: &'static str,
    value: String,
    #[prop(default = false)] muted: bool,
) -> impl IntoView {
    let class = if muted {
        "rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3 text-sm text-slate-400 flex justify-between"
    } else {
        "rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3 text-sm text-slate-300 flex justify-between font-medium"
    };
    view! {
        <div class=class>
            <span>{label}</span>
            <span>{value}</span>
        </div>
    }
}

#[component]
fn SummaryStat(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div>
            <p class="text-xs uppercase tracking-wide text-slate-500">{label}</p>
            <p class="mt-1 text-lg font-semibold text-white">{value}</p>
        </div>
    }
}

/// Backward-compatible export used by earlier routes.
#[component]
pub fn IraSellCalculator() -> impl IntoView {
    view! { <InvestmentAccountCalculator /> }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn read_csv_file(
    file: web_sys::File,
    csv_text: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let reader = match web_sys::FileReader::new() {
        Ok(reader) => reader,
        Err(_) => {
            error.set(Some("Could not read the selected file.".into()));
            return;
        }
    };

    let onload =
        Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(move |ev: web_sys::ProgressEvent| {
            let Some(target) = ev.target() else {
                error.set(Some("Could not read the selected file.".into()));
                return;
            };
            let Ok(reader) = target.dyn_into::<web_sys::FileReader>() else {
                error.set(Some("Could not read the selected file.".into()));
                return;
            };

            match reader.result() {
                Ok(value) => {
                    if let Some(text) = value.as_string() {
                        csv_text.set(Some(text));
                    } else {
                        error.set(Some("Could not decode the CSV as text.".into()));
                    }
                }
                Err(_) => error.set(Some("Could not read the selected file.".into())),
            }
        });

    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();

    if reader.read_as_text(&file).is_err() {
        error.set(Some("Could not read the selected file.".into()));
    }
}
