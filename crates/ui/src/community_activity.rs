//! Public projection only: source documents and subject identifiers stay server-side.
use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Dashboard {
    pub valid_for_seconds: u64,
    pub charts: Vec<Chart>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Chart {
    pub title: String,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Row {
    pub at: i64,
    pub description: String,
    pub count: Option<u64>,
}

#[server(prefix = "/api/community-activity", endpoint = "snapshot")]
pub async fn activity_data() -> Result<Option<Dashboard>, ServerFnError> {
    tokio::task::spawn_blocking(crate::activity_source::load)
        .await
        .map_err(|_| ServerFnError::new("Activity source unavailable"))
}

#[component]
pub fn CommunityActivityPage() -> impl IntoView {
    let data = Resource::new(|| (), |_| activity_data());
    view! {
        <Title text="Community activity | dabney.moe" />
        <Meta name="robots" content="noindex, nofollow, noarchive" />
        <article class="mx-auto max-w-5xl px-6 py-16">
            <a href="/">"Home"</a>
            <h1 class="text-4xl font-bold my-6">"Community activity"</h1>
            <p>"This page shows authorized community activity metadata, not messages or conversations. Live activity is unavailable until a listener and current authorization are available. Test data is never shown as live activity."</p>
            <p class="my-4">"Discord-reported presence is delayed by at least one hour. The dashboard shows a rolling seven-day window; this does not expire published Git LFS history or downloaded copies. Missing observations are unknown, not evidence that someone is offline."</p>
            <details class="my-4">
                <summary class="cursor-pointer">"How publication and privacy work"</summary>
                <p class="my-4">"Offline or invisible does not prove absence. Counts cover completed UTC hours only. No listener is confirmed active."</p>
                <p>"Current authorization is checked separately on every request. Publication stops when authorization is unavailable or over 60 seconds old, or the snapshot is over five minutes old. Withdrawal can still be visible within that bounded interval; this is not instantaneous revocation. Refresh to load new data. The display timer is only a convenience: suspended tabs, disabled JavaScript, and downloaded pages cannot be revoked. Already copied pages cannot be recalled. Aggregates exclude the operator; affected hours must be removed entirely on withdrawal, not republished with subtracted counts."</p>
            </details>
            <p class="my-4"><a class="underline" href="/community-policy">"Community privacy and participation policy"</a></p>
            <Suspense fallback=|| view! { <p role="status">"Loading; observation state unknown."</p> }>
                {move || data.get().map(|result| match result {
                    Ok(Some(dashboard)) => view! { <DashboardView dashboard /> }.into_any(),
                    Ok(None) | Err(_) => view! { <p role="status">"No publishable data. Source or current authorization is missing, stale, invalid, or unavailable; activity is unknown."</p> }.into_any(),
                })}
            </Suspense>
        </article>
    }
}

#[component]
fn DashboardView(dashboard: Dashboard) -> impl IntoView {
    let expired = RwSignal::new(false);
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    Effect::new(move |_| {
        // Relative duration avoids trusting the visitor's wall clock. This is a
        // display aid only, not revocation of a downloaded response; timers may
        // be delayed by suspended tabs and by transport/hydration latency.
        set_timeout(
            move || expired.set(true),
            std::time::Duration::from_secs(dashboard.valid_for_seconds),
        );
    });
    let charts = dashboard.charts;
    view! {
        <Show when=move || !expired.get() fallback=|| view! { <p role="status">"Publication lease expired. Refresh to recheck authorization; activity is unknown."</p> }>
            {charts.clone().into_iter().map(|chart| view! { <ActivityChart chart /> }).collect_view()}
        </Show>
    }
}

pub(crate) fn utc_time(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown time".into())
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    #[test]
    fn synthetic_chart_has_accessible_values_and_unknown_gaps() {
        let owner = Owner::new();
        owner.with(|| {
            let chart = Chart {
                title: "Synthetic activity".into(),
                rows: vec![
                    Row {
                        at: 1_800_000_000,
                        description: "20 synthetic messages".into(),
                        count: Some(20),
                    },
                    Row {
                        at: 1_800_003_600,
                        description: "Unknown observation".into(),
                        count: None,
                    },
                ],
            };
            let html = view! { <ActivityChart chart /> }.to_html();
            for text in [
                "role=\"img\"",
                "aria-label=\"Synthetic activity",
                "<caption>",
                "scope=\"col\"",
                "scope=\"row\"",
                "20 synthetic messages",
                "Unknown observation",
                "Missing hours are unknown, not zero",
                "UTC",
            ] {
                assert!(html.contains(text), "{text}: {html}");
            }
        });
    }
}

#[component]
fn ActivityChart(chart: Chart) -> impl IntoView {
    let max = chart
        .rows
        .iter()
        .filter_map(|r| r.count)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let first = chart.rows.iter().map(|r| r.at).min().unwrap_or(0);
    let last = chart.rows.iter().map(|r| r.at).max().unwrap_or(first);
    let position = |at: i64| 5.0 + (at - first) as f64 * 590.0 / (last - first).max(1) as f64;
    let bars = chart
        .rows
        .iter()
        .filter_map(|row| {
            row.count.map(|count| {
                let x = position(row.at);
                let height = count as f64 / max * 100.0;
                view! { <rect x=x y=110.0-height width=3 height=height fill="currentColor" /> }
            })
        })
        .collect_view();
    let events = chart
        .rows
        .iter()
        .filter(|row| row.count.is_none())
        .map(|row| {
            let x = position(row.at);
            view! { <circle cx=x cy=60 r=4 fill="currentColor" /> }
        })
        .collect_view();
    view! {
        <section class="my-8">
            <h2 class="text-2xl font-semibold">{chart.title.clone()}</h2>
            <svg viewBox="0 0 600 120" role="img" aria-label=format!("{}. Values and unknown gaps are in the table below.", chart.title) class="w-full text-cyan-300">
                <title>{chart.title.clone()}</title>
                {bars}
                {events}
            </svg>
            <div class="overflow-x-auto">
                <table class="w-full text-left">
                    <caption>{format!("{} — text alternative. Horizontal position is UTC time; bar height is message count and dots mark observations. Missing hours are unknown, not zero.", chart.title)}</caption>
                    <thead><tr><th scope="col">"UTC timestamp"</th><th scope="col">"Observation"</th></tr></thead>
                    <tbody>{chart.rows.into_iter().map(|row| view! {
                        <tr><th scope="row">{utc_time(row.at)}</th><td>{row.description}</td></tr>
                    }).collect_view()}</tbody>
                </table>
            </div>
        </section>
    }
}
