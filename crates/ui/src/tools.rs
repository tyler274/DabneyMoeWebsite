//! Client-side financial tools (portfolio CSV stays in the browser).

use leptos::prelude::*;
use leptos_router::components::A;

use crate::ira_sell::{analyze_portfolio, parse_withdrawal, SellReport};
use crate::sections::Footer;

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
                    class="absolute right-0 z-50 mt-2 min-w-52 rounded-xl border border-white/10 bg-slate-900 py-2 shadow-xl"
                    role="menu"
                >
                    <A href="/tools/investment-account-sell" on:click=close>
                        <span class="block px-4 py-2 text-slate-200 transition hover:bg-white/5 hover:text-white" role="menuitem">
                            "Investment Account Sell Calculator"
                        </span>
                    </A>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn IraSellPage() -> impl IntoView {
    view! {
        <IraSellShell />
    }
}

#[component]
fn IraSellShell() -> impl IntoView {
    use crate::sections::NavBar;

    view! {
        <NavBar />
        <section class="border-b border-white/5 px-6 py-20">
            <div class="mx-auto max-w-4xl">
                <p class="mb-2 text-sm font-semibold uppercase tracking-widest text-cyan-400">
                    "Tools"
                </p>
                <h1 class="mb-3 text-3xl font-bold tracking-tight text-white sm:text-4xl">
                    "Investment Account Ratio-Preserving Sell Calculator"
                </h1>
                <p class="mb-10 max-w-2xl text-lg leading-relaxed text-slate-300">
                    "Upload a Raymond James portfolio CSV and enter a withdrawal amount. "
                    "The calculator runs entirely in your browser—your file never leaves this device."
                </p>
                <IraSellCalculator />
            </div>
        </section>
        <Footer />
    }
}

#[component]
pub fn IraSellCalculator() -> impl IntoView {
    let csv_text = RwSignal::new(None::<String>);
    let file_name = RwSignal::new(None::<String>);
    let withdrawal = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let report = RwSignal::new(None::<SellReport>);

    let recompute = move || {
        error.set(None);
        report.set(None);

        let Some(text) = csv_text.get() else {
            return;
        };

        let amount = match parse_withdrawal(&withdrawal.get()) {
            Ok(value) => value,
            Err(message) => {
                if !withdrawal.get().trim().is_empty() {
                    error.set(Some(message));
                }
                return;
            }
        };

        match analyze_portfolio(&text, amount) {
            Ok(result) => report.set(Some(result)),
            Err(message) => error.set(Some(message)),
        }
    };

    Effect::new(move |_| {
        let _ = csv_text.get();
        let _ = withdrawal.get();
        recompute();
    });

    let on_withdrawal_input = move |ev: leptos::ev::Event| {
        withdrawal.set(event_target_value(&ev));
    };

    #[cfg(any(feature = "hydrate", feature = "csr"))]
    let on_file_change = {
        move |ev: leptos::ev::Event| {
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
        }
    };

    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    let on_file_change = move |_ev: leptos::ev::Event| {};

    view! {
        <div class="space-y-8">
            <div class="grid gap-6 rounded-2xl border border-white/10 bg-white/[0.02] p-6 sm:grid-cols-2">
                <div>
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
                <div>
                    <label class="mb-2 block text-sm font-medium text-slate-200" for="withdrawal">
                        "Target withdrawal"
                    </label>
                    <input
                        id="withdrawal"
                        type="text"
                        inputmode="decimal"
                        placeholder="10000"
                        prop:value=move || withdrawal.get()
                        class="w-full rounded-lg border border-white/10 bg-slate-950 px-3 py-2 text-slate-100 placeholder:text-slate-600 focus:border-cyan-400/50 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                        on:input=on_withdrawal_input
                    />
                    <p class="mt-2 text-xs text-slate-500">"Whole-share sells that preserve fund weights."</p>
                </div>
            </div>

            <Show when=move || error.get().is_some()>
                <div class="rounded-xl border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-200">
                    {move || error.get().unwrap_or_default()}
                </div>
            </Show>

            <Show when=move || report.get().is_some()>
                {move || {
                    report
                        .get()
                        .map(|data| view! { <IraSellReportView report=data /> })
                }}
            </Show>
        </div>
    }
}

#[component]
fn IraSellReportView(report: SellReport) -> impl IntoView {
    let total_proceeds = report.total_proceeds();
    let shortfall = report.shortfall();

    view! {
        <div class="space-y-6">
            <div class="grid gap-4 rounded-2xl border border-white/10 bg-white/[0.02] p-6 sm:grid-cols-2 lg:grid-cols-4">
                <SummaryStat label="Total portfolio" value=fmt_usd(report.total_portfolio()) />
                <SummaryStat label="Cash / RJBDP" value=fmt_usd(report.cash_value) />
                <SummaryStat label="Invested funds" value=fmt_usd(report.total_fund_value()) />
                <SummaryStat label="Target withdrawal" value=fmt_usd(report.withdrawal) />
            </div>

            <div class="overflow-x-auto rounded-2xl border border-white/10">
                <table class="min-w-full text-left text-sm">
                    <thead class="border-b border-white/10 bg-white/[0.03] text-xs uppercase tracking-wide text-slate-400">
                        <tr>
                            <th class="px-4 py-3">"Fund"</th>
                            <th class="px-4 py-3">"Sym"</th>
                            <th class="px-4 py-3 text-right">"Weight"</th>
                            <th class="px-4 py-3 text-right">"Sell"</th>
                            <th class="px-4 py-3 text-right">"@ Price"</th>
                            <th class="px-4 py-3 text-right">"Proceeds"</th>
                            <th class="px-4 py-3 text-right">"Δ Weight"</th>
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-white/5 text-slate-200">
                        {report
                            .plan
                            .iter()
                            .enumerate()
                            .map(|(index, line)| {
                                let weight_before = report.weight_before(index);
                                let delta = report.weight_delta(index);
                                view! {
                                    <tr>
                                        <td class="px-4 py-3">{line.holding.description.clone()}</td>
                                        <td class="px-4 py-3 font-medium text-cyan-300">{line.holding.symbol.clone()}</td>
                                        <td class="px-4 py-3 text-right">{format!("{weight_before:.2}%")}</td>
                                        <td class="px-4 py-3 text-right">{line.shares_to_sell}</td>
                                        <td class="px-4 py-3 text-right">{fmt_usd(line.holding.price)}</td>
                                        <td class="px-4 py-3 text-right">{fmt_usd(line.proceeds)}</td>
                                        <td class="px-4 py-3 text-right">{format!("{:+.2}%", delta)}</td>
                                    </tr>
                                }
                            })
                            .collect_view()}
                    </tbody>
                    <tfoot class="border-t border-white/10 bg-white/[0.02] text-slate-300">
                        <tr>
                            <td class="px-4 py-3 font-medium" colspan="5">"Total proceeds"</td>
                            <td class="px-4 py-3 text-right font-medium">{fmt_usd(total_proceeds)}</td>
                            <td></td>
                        </tr>
                        <Show when=move || { shortfall.abs() > 0.005 }>
                            <tr>
                                <td class="px-4 py-3 text-slate-400" colspan="5">"Rounding shortfall (keep as cash)"</td>
                                <td class="px-4 py-3 text-right text-slate-400">{fmt_usd(shortfall)}</td>
                                <td></td>
                            </tr>
                        </Show>
                    </tfoot>
                </table>
            </div>

            <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <h2 class="mb-4 text-lg font-semibold text-white">"Trade instructions"</h2>
                <ul class="space-y-2 font-mono text-sm text-slate-300">
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
                                        " (~"
                                        {fmt_usd(line.proceeds)}
                                        ")"
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
                </ul>
            </div>
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

    let onload = Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(move |ev: web_sys::ProgressEvent| {
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
