//! Client-side Recovery & Mental Health Facility Finder for `/tools/recovery` (and embedded in
//! `/mental-health`), migrated from `dei.dabney.moe` into `dabney.moe` Tools.
//!
//! Powered by checked-in per-state shards served statically from `/recovery/<ST>.json` (versioned
//! by SHA-256 in `/recovery/index.json` and cached on-device in browser Cache Storage
//! `dabney-recovery-shards-v1`). Includes non-care amenities (pools, hot tubs, food situation,
//! electronic device policy, rooms, pets, smoking/vaping) and strictly distinguished clinical staff
//! license filters (MD ≠ DO, Psychiatrist ≠ PMHNP/NP or PA).
//!
//! All search, filtering, and optional device geolocation distance sorting happen strictly on the
//! user's device; zero location or query data leaves the browser.

use std::collections::HashMap;

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;
use leptos_router::hooks::use_location;

pub use recovery_index::*;

use crate::i18n::{t, use_locale, Locale, Msg};
use crate::info_tip::InfoTip;
use crate::mental_health_support::CRISIS_RESOURCES;
use crate::sections::{Footer, NavBar};

/// Embedded state/territory directory index so SSR renders all 56 jurisdictions, facility counts,
/// enrichment counts, and shard SHA-256 digests immediately without a network round-trip.
const RECOVERY_INDEX_JSON: &str = include_str!("../../../public/recovery/index.json");

/// Cache Storage bucket holding versioned state shards (`/recovery/<ST>.json?v=<sha256>`).
#[cfg(any(feature = "hydrate", feature = "csr"))]
const SHARD_CACHE_NAME: &str = "dabney-recovery-shards-v1";

/// Parses the embedded 56-state/territory directory index.
pub fn recovery_index() -> StateIndex {
    serde_json::from_str(RECOVERY_INDEX_JSON).unwrap_or(StateIndex { states: Vec::new() })
}

/// Payload handed to the Mental Health dashboard when a patient saves a facility
/// directly into their local Stanley-Brown Personal Crisis & Support Card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedCareTeamFacility {
    pub name: String,
    pub phone: String,
    pub address: String,
}

#[cfg_attr(not(any(feature = "hydrate", feature = "csr")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShardCacheBadge {
    CachedOnDevice,
    FetchedAndCached,
    RefreshingCached,
}

impl ShardCacheBadge {
    const fn label(self) -> &'static str {
        match self {
            Self::CachedOnDevice => "Cached on device",
            Self::FetchedAndCached => "Synced & cached",
            Self::RefreshingCached => "Updating cached copy…",
        }
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
enum ShardLoadUpdate {
    StaleCached(StateCatalog),
    Fresh {
        catalog: StateCatalog,
        from_cache: bool,
    },
    Failed(String),
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn shard_versioned_url(state_code: &str, version: Option<&str>) -> String {
    match version.filter(|v| !v.is_empty()) {
        Some(v) => format!("/recovery/{state_code}.json?v={v}"),
        None => format!("/recovery/{state_code}.json"),
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn load_state_shard_cached(
    state_code: String,
    expected_sha256: Option<String>,
    on_update: impl Fn(ShardLoadUpdate) + 'static,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    wasm_bindgen_futures::spawn_local(async move {
        let Some(win) = web_sys::window() else {
            on_update(ShardLoadUpdate::Failed(
                "Browser window unavailable.".to_string(),
            ));
            return;
        };

        // 1. Check live /recovery/index.json (bypassing HTTP cache) if available, falling back to
        //    the build-embedded SHA-256 so updated deployments auto-invalidate cached shards.
        let effective_version = fetch_live_state_version(&win, &state_code)
            .await
            .or(expected_sha256);

        let target_url = shard_versioned_url(&state_code, effective_version.as_deref());
        let cache = open_shard_cache(&win).await;

        // 2. Inspect browser Cache Storage for this state.
        let mut had_stale = false;
        if let Some(cache_ref) = &cache {
            if let Some((cached_catalog, is_current)) =
                read_cached_shard(&win, cache_ref, &state_code, &target_url).await
            {
                if is_current {
                    on_update(ShardLoadUpdate::Fresh {
                        catalog: cached_catalog,
                        from_cache: true,
                    });
                    return;
                }
                had_stale = true;
                on_update(ShardLoadUpdate::StaleCached(cached_catalog));
            }
        }

        // 3. Fetch the versioned shard from origin.
        let resp_val = match JsFuture::from(win.fetch_with_str(&target_url)).await {
            Ok(v) => v,
            Err(_) => {
                if !had_stale {
                    on_update(ShardLoadUpdate::Failed(format!(
                        "Couldn't fetch the {state_code} directory right now."
                    )));
                }
                return;
            }
        };
        let Ok(resp) = resp_val.dyn_into::<web_sys::Response>() else {
            if !had_stale {
                on_update(ShardLoadUpdate::Failed(
                    "Unexpected response while loading facility directory.".to_string(),
                ));
            }
            return;
        };
        if !resp.ok() {
            if !had_stale {
                on_update(ShardLoadUpdate::Failed(format!(
                    "Directory shard for {state_code} returned status {}.",
                    resp.status()
                )));
            }
            return;
        }
        let Ok(text_promise) = resp.text() else {
            if !had_stale {
                on_update(ShardLoadUpdate::Failed(
                    "Couldn't read facility directory response.".to_string(),
                ));
            }
            return;
        };
        let Ok(body_val) = JsFuture::from(text_promise).await else {
            if !had_stale {
                on_update(ShardLoadUpdate::Failed(
                    "Couldn't decode facility directory text.".to_string(),
                ));
            }
            return;
        };
        let Some(body) = body_val.as_string() else {
            if !had_stale {
                on_update(ShardLoadUpdate::Failed(
                    "Facility directory response was empty.".to_string(),
                ));
            }
            return;
        };
        let catalog = match serde_json::from_str::<StateCatalog>(&body) {
            Ok(c) => c,
            Err(err) => {
                if !had_stale {
                    on_update(ShardLoadUpdate::Failed(format!(
                        "Couldn't parse {state_code} directory: {err}"
                    )));
                }
                return;
            }
        };

        // 4. Persist into Cache Storage and evict older cached versions of the same state.
        if let Some(cache_ref) = &cache {
            store_shard_in_cache(&win, cache_ref, &state_code, &target_url, &body).await;
        }

        on_update(ShardLoadUpdate::Fresh {
            catalog,
            from_cache: false,
        });
    });
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn fetch_live_state_version(win: &web_sys::Window, state_code: &str) -> Option<String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let init = web_sys::RequestInit::new();
    init.set_cache(web_sys::RequestCache::NoStore);
    let resp_val = JsFuture::from(win.fetch_with_str_and_init("/recovery/index.json", &init))
        .await
        .ok()?;
    let resp: web_sys::Response = resp_val.dyn_into().ok()?;
    if !resp.ok() {
        return None;
    }
    let text = JsFuture::from(resp.text().ok()?).await.ok()?.as_string()?;
    let idx: StateIndex = serde_json::from_str(&text).ok()?;
    idx.states
        .into_iter()
        .find(|s| s.state == state_code)
        .and_then(|s| s.sha256)
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn open_shard_cache(win: &web_sys::Window) -> Option<web_sys::Cache> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let caches = win.caches().ok()?;
    JsFuture::from(caches.open(SHARD_CACHE_NAME))
        .await
        .ok()?
        .dyn_into::<web_sys::Cache>()
        .ok()
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn absolute_url(win: &web_sys::Window, relative: &str) -> Option<String> {
    let href = win.location().href().ok()?;
    web_sys::Url::new_with_base(relative, &href)
        .ok()
        .map(|u| u.href())
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn read_cached_shard(
    win: &web_sys::Window,
    cache: &web_sys::Cache,
    state_code: &str,
    target_url: &str,
) -> Option<(StateCatalog, bool)> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let target_abs = absolute_url(win, target_url)?;
    let keys_arr: js_sys::Array = JsFuture::from(cache.keys()).await.ok()?.dyn_into().ok()?;
    let suffix = format!("/recovery/{state_code}.json");

    let mut matched_urls = Vec::new();
    for item in keys_arr.iter() {
        if let Ok(req) = item.dyn_into::<web_sys::Request>() {
            let url = req.url();
            if let Ok(parsed) = web_sys::Url::new(&url) {
                if parsed.pathname().ends_with(&suffix) {
                    matched_urls.push(url);
                }
            }
        }
    }

    let (chosen_url, is_current) = if matched_urls.iter().any(|u| u == &target_abs) {
        (target_abs, true)
    } else {
        let fallback = matched_urls.into_iter().next()?;
        (fallback, false)
    };

    let resp: web_sys::Response = JsFuture::from(cache.match_with_str(&chosen_url))
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    let text = JsFuture::from(resp.text().ok()?).await.ok()?.as_string()?;
    let catalog: StateCatalog = serde_json::from_str(&text).ok()?;
    Some((catalog, is_current))
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn store_shard_in_cache(
    win: &web_sys::Window,
    cache: &web_sys::Cache,
    state_code: &str,
    target_url: &str,
    body: &str,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let Ok(resp) = web_sys::Response::new_with_opt_str(Some(body)) else {
        return;
    };
    if JsFuture::from(cache.put_with_str(target_url, &resp))
        .await
        .is_err()
    {
        return;
    }

    let Some(target_abs) = absolute_url(win, target_url) else {
        return;
    };
    let Ok(keys_val) = JsFuture::from(cache.keys()).await else {
        return;
    };
    let Ok(keys_arr) = keys_val.dyn_into::<js_sys::Array>() else {
        return;
    };
    let suffix = format!("/recovery/{state_code}.json");
    for item in keys_arr.iter() {
        if let Ok(req) = item.dyn_into::<web_sys::Request>() {
            let url = req.url();
            if url != target_abs {
                if let Ok(parsed) = web_sys::Url::new(&url) {
                    if parsed.pathname().ends_with(&suffix) {
                        let _ = JsFuture::from(cache.delete_with_str(&url)).await;
                    }
                }
            }
        }
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn request_browser_location(
    on_success: impl Fn(Coordinates) + 'static,
    on_error: impl Fn(String) + 'static,
) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let Some(win) = web_sys::window() else {
        on_error("Browser window unavailable.".to_string());
        return;
    };
    let Ok(geo) = win.navigator().geolocation() else {
        on_error("Location services are unavailable in this browser.".to_string());
        return;
    };

    let on_error = std::rc::Rc::new(on_error);
    let on_error_cb = std::rc::Rc::clone(&on_error);
    let ok_cb = Closure::once(move |pos: web_sys::Position| {
        let coords = pos.coords();
        on_success(Coordinates {
            lat: coords.latitude(),
            lng: coords.longitude(),
        });
    });
    let err_cb = Closure::once(move |err: web_sys::PositionError| {
        let msg = match err.code() {
            1 => "Location permission was denied. You can still search by city, ZIP, or typed coordinates.",
            2 => "Your device couldn't determine your location right now.",
            3 => "Location lookup timed out. Try again or search by city/ZIP.",
            _ => "Location lookup failed. You can still search by city or ZIP.",
        };
        on_error_cb(msg.to_string());
    });

    if geo
        .get_current_position_with_error_callback(
            ok_cb.as_ref().unchecked_ref(),
            Some(err_cb.as_ref().unchecked_ref()),
        )
        .is_err()
    {
        on_error("Couldn't start location lookup in this browser.".to_string());
    }
    ok_cb.forget();
    err_cb.forget();
}

/// Parses `?state=CO` from a URL query string if valid.
fn state_from_query(search: &str) -> Option<String> {
    let trimmed = search.trim_start_matches('?');
    for pair in trimmed.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "state" {
                let code = v.trim().to_ascii_uppercase();
                if code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()) {
                    return Some(code);
                }
            }
        }
    }
    None
}

/// Dedicated standalone page for Recovery Search under `dabney.moe` Tools (`/tools/recovery`),
/// replacing the separate `dei.dabney.moe` site.
#[component]
pub fn RecoveryToolPage() -> impl IntoView {
    let location = use_location();
    let locale = use_locale();
    let initial_state = state_from_query(&location.search.get_untracked());

    view! {
        <Title text="Recovery & Mental Health Facility Finder | Tools | dabney.moe" />
        <Meta
            name="description"
            content="Search U.S. mental health and addiction recovery facilities privately on your device by care setting, insurance, amenities (pools, hot tubs, food, electronics policy), and exact staff license types (MD vs DO, Psychiatrist vs PMHNP/NP)."
        />
        <Link rel="canonical" href="https://dabney.moe/tools/recovery" />
        <NavBar />
        <section class="border-b border-white/5 px-4 py-12 sm:px-6 sm:py-16">
            <div class="mx-auto max-w-6xl space-y-6">
                <div class="flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                            {move || t(locale.get(), Msg::RecoveryEyebrow)}
                        </p>
                        <h1 class="mt-1 text-2xl font-bold tracking-tight text-white sm:text-4xl">
                            {move || {
                                let loc = locale.get();
                                if loc == Locale::En {
                                    "Recovery & Mental Health Facility Finder"
                                } else {
                                    t(loc, Msg::ToolRecoveryFinder)
                                }
                            }}
                        </h1>
                        <p class="mt-2 max-w-3xl text-sm leading-relaxed text-slate-300">
                            "Moved from dei.dabney.moe into dabney.moe Tools. Compare U.S. treatment facilities privately on your device, including everything aside from the care (pools, hot tubs, rooms, food situation, electronic device rules, smoking/vaping) and exact clinical staff licensure (MD vs DO, Psychiatrist vs Nurse Practitioner)."
                        </p>
                    </div>
                    <div class="flex flex-wrap items-center gap-2 text-xs">
                        <A href="/mental-health">
                            <span class="inline-flex min-h-[38px] items-center gap-1 rounded-full border border-white/15 bg-white/[0.04] px-3.5 py-1.5 font-medium text-slate-200 transition hover:border-cyan-400/40 hover:text-white">
                                <span>{move || t(locale.get(), Msg::SuiteMentalHealth)}</span>
                                <span class="inline-block rtl:rotate-180" aria-hidden="true">"→"</span>
                            </span>
                        </A>
                    </div>
                </div>

                <RecoverySearchSection
                    initial_state=initial_state.unwrap_or_else(|| "CO".to_string())
                />

                <div class="rounded-xl border border-white/10 bg-slate-900/50 p-4 text-xs leading-relaxed text-slate-400">
                    <p class="font-semibold text-slate-200">"Data Sources, Caching & Provenance"</p>
                    <p class="mt-1">
                        "Base directory listings come from SAMHSA FindTreatment.gov. Staff license distinctions (MD vs DO, Psychiatrist vs PMHNP/NP vs PA, PhD vs PsyD, LCSW, LPC, LMFT) are enriched via the CMS NPPES NPI Registry and facility team pages; non-care amenities (pools, hot tubs, food, electronic device policies, rooms, pets) come from facility pages and curated in-repo overrides ("
                        <code class="rounded bg-white/5 px-1.5 py-0.5 text-slate-200">"cargo run -p recovery-index --features cli"</code>
                        "). State shards are SHA-256 versioned and cached in your browser's Cache Storage for instant repeat loads and offline resilience. Always confirm current policies directly with the facility before admission."
                    </p>
                </div>
            </div>
        </section>
        <Footer />
    }
}

/// Interactive Recovery & Mental Health Facility Finder, usable standalone at `/tools/recovery`
/// or embedded inside `/mental-health`.
#[component]
pub fn RecoverySearchSection(
    /// Optional callback wiring a 1-click "Save to Care Team" action into the patient's
    /// local Stanley-Brown Personal Crisis & Support Card.
    #[prop(optional)]
    on_save_care_team: Option<Callback<SavedCareTeamFacility>>,
    /// When provided, defers fetching the state shard JSON until the section is visible.
    #[prop(optional)]
    active: Option<Signal<bool>>,
    /// Optional initial two-letter state code (defaults to `"CO"`).
    #[prop(optional)]
    initial_state: Option<String>,
) -> impl IntoView {
    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    let _ = active;

    let locale = use_locale();
    let index = recovery_index();
    let total_facilities: usize = index.states.iter().map(|s| s.facilities).sum();
    let total_enriched: usize = index.states.iter().map(|s| s.enriched_facilities).sum();
    let total_states = index.states.len();
    let retrieved_date = index
        .states
        .first()
        .map(|s| s.retrieved.clone())
        .unwrap_or_else(|| "2026-11-27".to_string());

    let state_versions: HashMap<String, String> = index
        .states
        .iter()
        .filter_map(|s| s.sha256.clone().map(|v| (s.state.clone(), v)))
        .collect();
    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    let _ = &state_versions;

    let default_state = initial_state
        .filter(|code| index.states.iter().any(|s| s.state == *code))
        .unwrap_or_else(|| "CO".to_string());

    let selected_state = RwSignal::new(default_state);
    let loaded_shards = RwSignal::new(HashMap::<String, StateCatalog>::new());
    let cache_badges = RwSignal::new(HashMap::<String, ShardCacheBadge>::new());
    let loading_state = RwSignal::new(Option::<String>::None);
    let load_error = RwSignal::new(Option::<String>::None);
    let reload_tick = RwSignal::new(0u32);

    // Query filter signals
    let search_text = RwSignal::new(String::new());
    let selected_care = RwSignal::new(Vec::<CareType>::new());
    let selected_settings = RwSignal::new(Vec::<Setting>::new());
    let selected_payments = RwSignal::new(Vec::<Payment>::new());
    let selected_amenities = RwSignal::new(Vec::<Amenity>::new());
    let selected_food = RwSignal::new(Vec::<FoodSituation>::new());
    let selected_dietary = RwSignal::new(Vec::<DietaryOption>::new());
    let selected_electronics = RwSignal::new(Vec::<ElectronicsPolicy>::new());
    let selected_smoking = RwSignal::new(Vec::<SmokingPolicy>::new());
    let selected_vaping = RwSignal::new(Vec::<VapingPolicy>::new());
    let selected_staff = RwSignal::new(Vec::<StaffLicense>::new());
    let staff_match_mode = RwSignal::new(StaffMatchMode::Any);
    let only_dedicated_physician = RwSignal::new(false);
    let only_shared_network = RwSignal::new(false);
    let selected_provider_id = RwSignal::new(Option::<String>::None);
    let only_enriched = RwSignal::new(false);

    // Collapsible filter drawers
    let show_care_filters = RwSignal::new(false);
    let show_lifestyle_filters = RwSignal::new(false);
    let show_staff_filters = RwSignal::new(false);
    let show_network_explorer = RwSignal::new(false);
    let show_all_payments = RwSignal::new(false);

    // Distance & geolocation signals
    let origin = RwSignal::new(Option::<Coordinates>::None);
    let coords_input = RwSignal::new(String::new());
    let coords_error = RwSignal::new(Option::<String>::None);
    let locating = RwSignal::new(false);
    let radius_km = RwSignal::new(DEFAULT_RADIUS_KM);
    let sort_mode = RwSignal::new(RecoverySort::Name);
    let page = RwSignal::new(0usize);

    // Feedback for 1-click "Save to Care Team"
    let saved_facility_name = RwSignal::new(Option::<String>::None);

    // Load selected state shard through in-memory cache -> Cache Storage -> versioned fetch
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        let versions_for_effect = state_versions.clone();
        Effect::new(move |_| {
            if let Some(vis) = active {
                if !vis.get() {
                    return;
                }
            }
            let state_code = selected_state.get();
            let _tick = reload_tick.get();
            if state_code.is_empty() {
                return;
            }
            if loaded_shards.with_untracked(|map| map.contains_key(&state_code)) {
                load_error.set(None);
                loading_state.set(None);
                return;
            }
            loading_state.set(Some(state_code.clone()));
            load_error.set(None);
            let expected_sha = versions_for_effect.get(&state_code).cloned();
            let code_done = state_code.clone();
            load_state_shard_cached(state_code, expected_sha, move |update| match update {
                ShardLoadUpdate::StaleCached(catalog) => {
                    loaded_shards.update(|map| {
                        map.insert(code_done.clone(), catalog);
                    });
                    cache_badges.update(|map| {
                        map.insert(code_done.clone(), ShardCacheBadge::RefreshingCached);
                    });
                    if loading_state.get_untracked().as_deref() == Some(code_done.as_str()) {
                        loading_state.set(None);
                    }
                }
                ShardLoadUpdate::Fresh {
                    catalog,
                    from_cache,
                } => {
                    loaded_shards.update(|map| {
                        map.insert(code_done.clone(), catalog);
                    });
                    cache_badges.update(|map| {
                        map.insert(
                            code_done.clone(),
                            if from_cache {
                                ShardCacheBadge::CachedOnDevice
                            } else {
                                ShardCacheBadge::FetchedAndCached
                            },
                        );
                    });
                    if loading_state.get_untracked().as_deref() == Some(code_done.as_str()) {
                        loading_state.set(None);
                    }
                }
                ShardLoadUpdate::Failed(err) => {
                    if selected_state.get_untracked() == code_done {
                        load_error.set(Some(err));
                        loading_state.set(None);
                    }
                }
            });
        });
    }

    let active_catalog = Memo::new(move |_| {
        let code = selected_state.get();
        loaded_shards.with(|map| map.get(&code).cloned())
    });

    let active_cache_badge = Memo::new(move |_| {
        let code = selected_state.get();
        cache_badges.with(|map| map.get(&code).copied())
    });

    let offered_facets = Memo::new(move |_| {
        active_catalog
            .get()
            .map(|cat| OfferedFacets::from_catalog(&cat))
            .unwrap_or_default()
    });

    // Settings and payments actually present in the active state's shard
    let offered_settings = Memo::new(move |_| {
        let facets = offered_facets.get();
        if facets.settings.is_empty() {
            Setting::ALL.to_vec()
        } else {
            facets.settings
        }
    });

    let offered_payments = Memo::new(move |_| {
        let Some(cat) = active_catalog.get() else {
            return Payment::PRIMARY.to_vec();
        };
        let pool = if show_all_payments.get() {
            Payment::ALL
        } else {
            Payment::PRIMARY
        };
        pool.iter()
            .copied()
            .filter(|p| cat.facilities.iter().any(|f| f.payments.contains(p)))
            .collect::<Vec<_>>()
    });

    // Live facet counts in the active state shard for amenities, food, electronics, and staff licenses
    let amenity_counts = Memo::new(move |_| {
        let mut counts = HashMap::<Amenity, usize>::new();
        if let Some(cat) = active_catalog.get() {
            for f in &cat.facilities {
                for &a in &f.lifestyle.amenities {
                    *counts.entry(a).or_default() += 1;
                }
            }
        }
        counts
    });

    let staff_counts = Memo::new(move |_| {
        let mut counts = HashMap::<StaffLicense, usize>::new();
        if let Some(cat) = active_catalog.get() {
            for f in &cat.facilities {
                for &lic in &f.staff_licenses {
                    *counts.entry(lic).or_default() += 1;
                }
            }
        }
        counts
    });

    let lifestyle_filter_count = Memo::new(move |_| {
        selected_amenities.with(Vec::len)
            + selected_food.with(Vec::len)
            + selected_dietary.with(Vec::len)
            + selected_electronics.with(Vec::len)
            + selected_smoking.with(Vec::len)
            + selected_vaping.with(Vec::len)
    });

    let network_by_id = Memo::new(move |_| {
        let mut map = HashMap::<String, NetworkProvider>::new();
        if let Some(cat) = active_catalog.get() {
            for p in cat.staff_network {
                map.insert(p.id.clone(), p);
            }
        }
        map
    });

    let top_network_providers = Memo::new(move |_| {
        active_catalog
            .get()
            .map(|cat| cat.staff_network.into_iter().take(24).collect::<Vec<_>>())
            .unwrap_or_default()
    });

    let active_filter_count = Memo::new(move |_| {
        selected_care.with(Vec::len)
            + selected_settings.with(Vec::len)
            + selected_payments.with(Vec::len)
            + lifestyle_filter_count.get()
            + selected_staff.with(Vec::len)
            + usize::from(only_dedicated_physician.get())
            + usize::from(only_shared_network.get())
            + usize::from(selected_provider_id.get().is_some())
            + usize::from(only_enriched.get())
            + usize::from(origin.get().is_some())
            + usize::from(!search_text.get().trim().is_empty())
    });

    let search_results = Memo::new(move |_| {
        let cat = active_catalog.get()?;
        let q = RecoveryQuery {
            text: search_text.get(),
            care: selected_care.get(),
            settings: selected_settings.get(),
            payments: selected_payments.get(),
            amenities: selected_amenities.get(),
            food: selected_food.get(),
            dietary: selected_dietary.get(),
            electronics: selected_electronics.get(),
            smoking: selected_smoking.get(),
            vaping: selected_vaping.get(),
            staff_licenses: selected_staff.get(),
            staff_match_mode: staff_match_mode.get(),
            only_dedicated_physician: only_dedicated_physician.get(),
            only_shared_network: only_shared_network.get(),
            provider_id: selected_provider_id.get(),
            only_enriched: only_enriched.get(),
            origin: origin.get(),
            radius_km: radius_km.get(),
            sort: sort_mode.get(),
            page: page.get(),
        };
        Some(search_catalog(&cat, &q))
    });

    let clear_all_filters = move |_| {
        search_text.set(String::new());
        selected_care.set(Vec::new());
        selected_settings.set(Vec::new());
        selected_payments.set(Vec::new());
        selected_amenities.set(Vec::new());
        selected_food.set(Vec::new());
        selected_dietary.set(Vec::new());
        selected_electronics.set(Vec::new());
        selected_smoking.set(Vec::new());
        selected_vaping.set(Vec::new());
        selected_staff.set(Vec::new());
        staff_match_mode.set(StaffMatchMode::Any);
        only_dedicated_physician.set(false);
        only_shared_network.set(false);
        selected_provider_id.set(None);
        only_enriched.set(false);
        origin.set(None);
        coords_input.set(String::new());
        coords_error.set(None);
        sort_mode.set(RecoverySort::Name);
        page.set(0);
    };

    let apply_typed_coords = move |_| {
        let raw = coords_input.get();
        if raw.trim().is_empty() {
            origin.set(None);
            coords_error.set(None);
            sort_mode.set(RecoverySort::Name);
            page.set(0);
            return;
        }
        match parse_coordinates(&raw) {
            Ok(pt) => {
                origin.set(Some(pt));
                coords_error.set(None);
                sort_mode.set(RecoverySort::Distance);
                page.set(0);
            }
            Err(err) => coords_error.set(Some(err)),
        }
    };

    let use_my_location = move |_| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            locating.set(true);
            coords_error.set(None);
            request_browser_location(
                move |pt| {
                    locating.set(false);
                    origin.set(Some(pt));
                    coords_input.set(format!("{:.4}, {:.4}", pt.lat, pt.lng));
                    sort_mode.set(RecoverySort::Distance);
                    page.set(0);
                },
                move |err| {
                    locating.set(false);
                    coords_error.set(Some(err));
                },
            );
        }
        #[cfg(not(any(feature = "hydrate", feature = "csr")))]
        {
            coords_error.set(Some(
                "Location lookup runs in your browser after page load.".to_string(),
            ));
        }
    };

    let states_list = index.states;

    view! {
        <section
            id="recovery-search"
            aria-label="Recovery and mental health facility finder"
            class="print-hide print:hidden space-y-6 rounded-2xl border border-emerald-400/25 bg-slate-900/60 p-6"
        >
            // Header
            <div class="flex flex-wrap items-start justify-between gap-4 border-b border-white/10 pb-5">
                <div class="max-w-2xl">
                    <div class="flex flex-wrap items-center gap-2">
                        <span class="rounded-full border border-emerald-400/35 bg-emerald-500/15 px-2.5 py-0.5 text-[11px] font-semibold uppercase tracking-wider text-emerald-300">
                            {move || t(locale.get(), Msg::RecoveryEyebrow)}
                        </span>
                        <span class="rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-0.5 text-[11px] text-slate-300">
                            {format!("{total_facilities} listings · {total_states} states & territories")}
                        </span>
                        {(total_enriched > 0).then(|| {
                            view! {
                                <span class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-2.5 py-0.5 text-[11px] font-medium text-cyan-200">
                                    {format!("{total_enriched} enriched with amenities / staff")}
                                </span>
                            }
                        })}
                    </div>
                    <h2 class="mt-2 flex items-center text-xl font-bold text-white sm:text-2xl">
                        {move || t(locale.get(), Msg::RecoveryTitle)}
                        <InfoTip
                            term="Recovery Search"
                            text="SAMHSA FindTreatment.gov listings enriched with non-care amenities (pools, hot tubs, rooms, food, electronic device policies) and exact clinical staff licenses (MD vs DO, Psychiatrist vs PMHNP/NP vs PA). Versioned shards are cached in browser Cache Storage; searches and GPS lookups never leave your device."
                        />
                    </h2>
                    <p class="mt-1.5 text-sm leading-relaxed text-slate-300">
                        "Filter privately on your device by care setting, insurance, living amenities (pools, hot tubs, food situation, phone/laptop policies), and exact staff licensure, without conflating MDs, DOs, psychiatrists, or nurse practitioners."
                    </p>
                </div>
                <div class="rounded-xl border border-white/10 bg-slate-950/70 px-3.5 py-2.5 text-xs text-slate-300">
                    <p class="font-semibold text-emerald-300">
                        {move || t(locale.get(), Msg::OnDeviceOnlyBadge)}
                    </p>
                    <p class="mt-0.5 text-slate-400">
                        {format!("Directory snapshot: {retrieved_date}")}
                    </p>
                    {move || {
                        active_cache_badge.get().map(|badge| {
                            view! {
                                <p class="mt-1 text-[11px] font-medium text-cyan-300">
                                    {format!("● {}", badge.label())}
                                </p>
                            }
                        })
                    }}
                </div>
            </div>

            // Always-accessible 24/7 crisis quick-call strip
            <div class="rounded-xl border border-rose-400/25 bg-rose-950/25 p-3.5 space-y-2">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <div class="space-y-0.5">
                        <p class="text-xs font-semibold text-rose-100">
                            {move || t(locale.get(), Msg::RecoveryCrisisBannerTitle)}
                        </p>
                        <p class="text-[11px] text-rose-100/80">
                            {move || t(locale.get(), Msg::RecoveryCrisis988Detail)}
                        </p>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                        {CRISIS_RESOURCES
                            .iter()
                            .map(|res| {
                                view! {
                                    <div class="flex flex-wrap items-center gap-1.5">
                                        {res.call_href.map(|href| {
                                            view! {
                                                <a
                                                    href=href
                                                    class="inline-flex min-h-[38px] items-center rounded-full border border-rose-300/35 bg-rose-500/20 px-3 py-1 text-xs font-semibold text-rose-100 transition hover:bg-rose-500/30"
                                                >
                                                    {res.call_label.unwrap_or(res.contact)}
                                                </a>
                                            }
                                        })}
                                        {res.sms_href.map(|href| {
                                            view! {
                                                <a
                                                    href=href
                                                    class="inline-flex min-h-[38px] items-center rounded-full border border-white/15 bg-slate-950/70 px-3 py-1 text-xs font-medium text-slate-100 transition hover:border-rose-300/40"
                                                >
                                                    {res.sms_label.unwrap_or(res.contact)}
                                                </a>
                                            }
                                        })}
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
                <p class="border-t border-rose-300/15 pt-2 text-[11px] text-rose-100/85">
                    {move || t(locale.get(), Msg::RecoveryCrisisLanguageNote)}
                </p>
            </div>

            // Primary controls: State + Keyword Search + Care Type toggles
            <div class="grid gap-3 md:grid-cols-12">
                <div class="md:col-span-4">
                    <label
                        for="recovery-state-select"
                        class="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-300"
                    >
                        {move || t(locale.get(), Msg::RecoveryStateLabel)}
                    </label>
                    <select
                        id="recovery-state-select"
                        on:change=move |ev| {
                            selected_state.set(event_target_value(&ev));
                            selected_settings.set(Vec::new());
                            selected_payments.set(Vec::new());
                            selected_provider_id.set(None);
                            page.set(0);
                        }
                        prop:value=move || selected_state.get()
                        class="min-h-[42px] w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-2 text-sm text-white focus:border-emerald-400 focus:outline-none"
                    >
                        {states_list
                            .into_iter()
                            .map(|entry| {
                                let code = entry.state.clone();
                                let enriched_note = if entry.enriched_facilities > 0 {
                                    format!(" · {} enriched", entry.enriched_facilities)
                                } else {
                                    String::new()
                                };
                                let label = format!(
                                    "{} - {} ({}{})",
                                    entry.state,
                                    state_full_name(&entry.state),
                                    entry.facilities,
                                    enriched_note
                                );
                                view! {
                                    <option value=code selected=move || selected_state.get() == entry.state>
                                        {label}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </div>

                <div class="md:col-span-5">
                    <label
                        for="recovery-keyword-input"
                        class="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-300"
                    >
                        "Facility Name, City, ZIP, Amenity, or License"
                    </label>
                    <input
                        id="recovery-keyword-input"
                        type="search"
                        placeholder=move || t(locale.get(), Msg::RecoverySearchPlaceholder)
                        prop:value=move || search_text.get()
                        on:input=move |ev| {
                            search_text.set(event_target_value(&ev));
                            page.set(0);
                        }
                        class="min-h-[42px] w-full rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2 text-sm text-white placeholder-slate-500 focus:border-emerald-400 focus:outline-none"
                    />
                </div>

                <div class="md:col-span-3">
                    <span class="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-slate-300">
                        {move || t(locale.get(), Msg::RecoveryCareFocus)}
                    </span>
                    <div class="flex flex-wrap gap-2" role="group" aria-label="Care focus">
                        {CareType::ALL
                            .iter()
                            .copied()
                            .map(|care| {
                                let is_on = move || selected_care.with(|v| v.contains(&care));
                                view! {
                                    <button
                                        type="button"
                                        aria-pressed=move || is_on().to_string()
                                        on:click=move |_| {
                                            toggle_vec(selected_care, care);
                                            page.set(0);
                                        }
                                        class=move || {
                                            if is_on() {
                                                "min-h-[42px] flex-1 rounded-xl border border-emerald-400/60 bg-emerald-500/20 px-3 py-1.5 text-xs font-semibold text-emerald-200 transition"
                                            } else {
                                                "min-h-[42px] flex-1 rounded-xl border border-white/15 bg-slate-950 px-3 py-1.5 text-xs font-medium text-slate-300 transition hover:border-white/30 hover:text-white"
                                            }
                                        }
                                    >
                                        {care.label()}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
            </div>

            // Location & Distance Bar + 3 Filter Drawer Toggles
            <div class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/60 px-4 py-3">
                <div class="flex flex-wrap items-center gap-2">
                    <button
                        type="button"
                        on:click=use_my_location
                        disabled=move || locating.get()
                        class="inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-cyan-400/40 bg-cyan-500/15 px-3.5 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/25 disabled:opacity-50"
                    >
                        {move || {
                            let loc = locale.get();
                            if locating.get() {
                                t(loc, Msg::RecoveryLocating)
                            } else {
                                t(loc, Msg::RecoveryUseLocation)
                            }
                        }}
                    </button>
                    <div class="flex items-center gap-1.5">
                        <label for="recovery-coords" class="sr-only">"Latitude and longitude"</label>
                        <input
                            id="recovery-coords"
                            type="text"
                            placeholder="Lat, Lng (optional)"
                            prop:value=move || coords_input.get()
                            on:input=move |ev| coords_input.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" {
                                    ev.prevent_default();
                                    apply_typed_coords(());
                                }
                            }
                            class="min-h-[38px] w-36 rounded-lg border border-white/15 bg-slate-900 px-2.5 py-1 text-xs text-white placeholder-slate-500 focus:border-cyan-400 focus:outline-none"
                        />
                        <button
                            type="button"
                            on:click=move |_| apply_typed_coords(())
                            class="min-h-[38px] rounded-lg border border-white/15 bg-white/[0.04] px-2.5 py-1 text-xs font-medium text-slate-200 transition hover:bg-white/10"
                        >
                            "Set"
                        </button>
                    </div>
                    <Show when=move || origin.get().is_some()>
                        <label for="recovery-radius" class="sr-only">"Search radius"</label>
                        <select
                            id="recovery-radius"
                            prop:value=move || radius_km.get().to_string()
                            on:change=move |ev| {
                                if let Ok(v) = event_target_value(&ev).parse::<u16>() {
                                    radius_km.set(v.clamp(MIN_RADIUS_KM, MAX_RADIUS_KM));
                                    page.set(0);
                                }
                            }
                            class="min-h-[38px] rounded-lg border border-white/15 bg-slate-900 px-2.5 py-1 text-xs text-white focus:border-cyan-400 focus:outline-none"
                        >
                            <option value="16">"Within 10 mi (16 km)"</option>
                            <option value="40">"Within 25 mi (40 km)"</option>
                            <option value="80">"Within 50 mi (80 km)"</option>
                            <option value="161">"Within 100 mi (161 km)"</option>
                            <option value="402">"Within 250 mi (402 km)"</option>
                        </select>
                        <button
                            type="button"
                            on:click=move |_| {
                                origin.set(None);
                                coords_input.set(String::new());
                                coords_error.set(None);
                                sort_mode.set(RecoverySort::Name);
                                page.set(0);
                            }
                            class="min-h-[36px] rounded-lg border border-white/10 px-2.5 py-1 text-xs text-slate-400 transition hover:text-white"
                        >
                            "Clear location"
                        </button>
                    </Show>
                </div>

                <div class="flex flex-wrap items-center gap-2">
                    <button
                        type="button"
                        aria-expanded=move || show_lifestyle_filters.get().to_string()
                        on:click=move |_| show_lifestyle_filters.update(|open| *open = !*open)
                        class=move || {
                            if lifestyle_filter_count.get() > 0 || show_lifestyle_filters.get() {
                                "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-teal-400/50 bg-teal-500/15 px-3.5 py-1.5 text-xs font-semibold text-teal-100 transition hover:bg-teal-500/25"
                            } else {
                                "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-white/15 bg-white/[0.04] px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:border-white/30 hover:text-white"
                            }
                        }
                    >
                        {move || {
                            let count = lifestyle_filter_count.get();
                            let arrow = if show_lifestyle_filters.get() { "▲" } else { "▼" };
                            if count > 0 {
                                format!("Amenities, Food & Devices ({count}) {arrow}")
                            } else {
                                format!("Amenities, Food & Devices {arrow}")
                            }
                        }}
                    </button>

                    <button
                        type="button"
                        aria-expanded=move || show_staff_filters.get().to_string()
                        on:click=move |_| show_staff_filters.update(|open| *open = !*open)
                        class=move || {
                            if selected_staff.with(|v| !v.is_empty()) || show_staff_filters.get() {
                                "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-amber-400/50 bg-amber-500/15 px-3.5 py-1.5 text-xs font-semibold text-amber-100 transition hover:bg-amber-500/25"
                            } else {
                                "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-white/15 bg-white/[0.04] px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:border-white/30 hover:text-white"
                            }
                        }
                    >
                        {move || {
                            let count = selected_staff.with(Vec::len);
                            let arrow = if show_staff_filters.get() { "▲" } else { "▼" };
                            if count > 0 {
                                format!("Staff Licenses ({count}) {arrow}")
                            } else {
                                format!("Staff Licenses {arrow}")
                            }
                        }}
                    </button>

                    <button
                        type="button"
                        aria-expanded=move || show_care_filters.get().to_string()
                        on:click=move |_| show_care_filters.update(|open| *open = !*open)
                        class=move || {
                            let count = selected_settings.with(Vec::len) + selected_payments.with(Vec::len);
                            if count > 0 || show_care_filters.get() {
                                "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-emerald-400/50 bg-emerald-500/15 px-3.5 py-1.5 text-xs font-semibold text-emerald-100 transition hover:bg-emerald-500/25"
                            } else {
                                "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-white/15 bg-white/[0.04] px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:border-white/30 hover:text-white"
                            }
                        }
                    >
                        {move || {
                            let count = selected_settings.with(Vec::len) + selected_payments.with(Vec::len);
                            let arrow = if show_care_filters.get() { "▲" } else { "▼" };
                            let label = t(locale.get(), Msg::RecoveryMoreFilters);
                            if count > 0 {
                                format!("{label} ({count}) {arrow}")
                            } else {
                                format!("{label} {arrow}")
                            }
                        }}
                    </button>

                    <Show when=move || { offered_facets.get().network_provider_count > 0 }>
                        <button
                            type="button"
                            aria-expanded=move || show_network_explorer.get().to_string()
                            on:click=move |_| show_network_explorer.update(|open| *open = !*open)
                            class=move || {
                                if selected_provider_id.get().is_some()
                                    || only_shared_network.get()
                                    || show_network_explorer.get()
                                {
                                    "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-indigo-400/55 bg-indigo-500/20 px-3.5 py-1.5 text-xs font-semibold text-indigo-100 transition hover:bg-indigo-500/30"
                                } else {
                                    "inline-flex min-h-[38px] items-center gap-1.5 rounded-full border border-white/15 bg-white/[0.04] px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:border-indigo-400/40 hover:text-white"
                                }
                            }
                        >
                            {move || {
                                let n = offered_facets.get().network_provider_count;
                                let arrow = if show_network_explorer.get() { "▲" } else { "▼" };
                                format!("Shared Staff Network ({n}) {arrow}")
                            }}
                        </button>
                    </Show>

                    <Show when=move || { offered_facets.get().enriched_count > 0 }>
                        <button
                            type="button"
                            aria-pressed=move || only_enriched.get().to_string()
                            on:click=move |_| {
                                only_enriched.update(|v| *v = !*v);
                                page.set(0);
                            }
                            class=move || {
                                if only_enriched.get() {
                                    "min-h-[38px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1.5 text-xs font-semibold text-cyan-100 transition"
                                } else {
                                    "min-h-[38px] rounded-full border border-white/15 bg-white/[0.03] px-3 py-1.5 text-xs text-slate-300 transition hover:border-cyan-400/40 hover:text-white"
                                }
                            }
                        >
                            {move || {
                                format!(
                                    "Enriched only ({})",
                                    offered_facets.get().enriched_count
                                )
                            }}
                        </button>
                    </Show>

                    <Show when=move || { active_filter_count.get() > 0 }>
                        <button
                            type="button"
                            on:click=clear_all_filters
                            class="min-h-[38px] rounded-full border border-rose-400/30 bg-rose-500/10 px-3 py-1.5 text-xs font-medium text-rose-200 transition hover:bg-rose-500/20"
                        >
                            {move || t(locale.get(), Msg::RecoveryClearFilters)}
                        </button>
                    </Show>
                </div>
            </div>

            {move || {
                coords_error.get().map(|err| {
                    view! {
                        <p
                            role="alert"
                            class="rounded-xl border border-amber-400/30 bg-amber-500/10 px-3.5 py-2 text-xs text-amber-100"
                        >
                            {err}
                        </p>
                    }
                })
            }}

            // Drawer 1: Amenities, Food, Electronic Devices & Policies (Everything aside from the care)
            <Show when=move || show_lifestyle_filters.get()>
                <div class="space-y-4 rounded-xl border border-teal-400/25 bg-slate-950/75 p-4">
                    <div class="flex flex-wrap items-baseline justify-between gap-2">
                        <p class="text-xs font-semibold uppercase tracking-wider text-teal-300">
                            "Non-Care Amenities & Living Setup"
                        </p>
                        <p class="text-[11px] text-slate-400">
                            "Amenities & dietary require all selected · Food, devices & smoking match any selected"
                        </p>
                    </div>

                    // Amenities grouped by category
                    <div class="grid gap-4 md:grid-cols-2">
                        {AmenityCategory::ALL
                            .iter()
                            .copied()
                            .map(|cat| {
                                let items: Vec<Amenity> = Amenity::ALL
                                    .iter()
                                    .copied()
                                    .filter(|a| a.category() == cat)
                                    .collect();
                                view! {
                                    <div>
                                        <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                            {cat.label()}
                                        </p>
                                        <div class="flex flex-wrap gap-1.5">
                                            {items
                                                .into_iter()
                                                .map(|amenity| {
                                                    let is_on = move || selected_amenities.with(|v| v.contains(&amenity));
                                                    view! {
                                                        <button
                                                            type="button"
                                                            aria-pressed=move || is_on().to_string()
                                                            on:click=move |_| {
                                                                toggle_vec(selected_amenities, amenity);
                                                                page.set(0);
                                                            }
                                                            class=move || {
                                                                if is_on() {
                                                                    "min-h-[32px] rounded-full border border-teal-400/60 bg-teal-500/20 px-2.5 py-1 text-xs font-semibold text-teal-100 transition"
                                                                } else {
                                                                    "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                                }
                                                            }
                                                        >
                                                            {move || {
                                                                let count = amenity_counts
                                                                    .with(|m| m.get(&amenity).copied().unwrap_or(0));
                                                                if count > 0 {
                                                                    format!("{} ({count})", amenity.label())
                                                                } else {
                                                                    amenity.label().to_string()
                                                                }
                                                            }}
                                                        </button>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>

                    // Food & Dietary
                    <div class="grid gap-4 border-t border-white/10 pt-3 md:grid-cols-2">
                        <div>
                            <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                "Food & Dining Situation"
                            </p>
                            <div class="flex flex-wrap gap-1.5">
                                {FoodSituation::ALL
                                    .iter()
                                    .copied()
                                    .map(|food| {
                                        let is_on = move || selected_food.with(|v| v.contains(&food));
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || is_on().to_string()
                                                on:click=move |_| {
                                                    toggle_vec(selected_food, food);
                                                    page.set(0);
                                                }
                                                class=move || {
                                                    if is_on() {
                                                        "min-h-[32px] rounded-full border border-emerald-400/60 bg-emerald-500/20 px-2.5 py-1 text-xs font-semibold text-emerald-100 transition"
                                                    } else {
                                                        "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                    }
                                                }
                                            >
                                                {food.label()}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                        <div>
                            <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                "Dietary Accommodations"
                            </p>
                            <div class="flex flex-wrap gap-1.5">
                                {DietaryOption::ALL
                                    .iter()
                                    .copied()
                                    .map(|diet| {
                                        let is_on = move || selected_dietary.with(|v| v.contains(&diet));
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || is_on().to_string()
                                                on:click=move |_| {
                                                    toggle_vec(selected_dietary, diet);
                                                    page.set(0);
                                                }
                                                class=move || {
                                                    if is_on() {
                                                        "min-h-[32px] rounded-full border border-emerald-400/60 bg-emerald-500/20 px-2.5 py-1 text-xs font-semibold text-emerald-100 transition"
                                                    } else {
                                                        "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                    }
                                                }
                                            >
                                                {diet.label()}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                    </div>

                    // Electronic Devices + Smoking / Vaping
                    <div class="grid gap-4 border-t border-white/10 pt-3 md:grid-cols-2">
                        <div>
                            <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                "Electronic Device Policy (Phones & Laptops)"
                            </p>
                            <div class="flex flex-wrap gap-1.5">
                                {ElectronicsPolicy::ALL
                                    .iter()
                                    .copied()
                                    .map(|pol| {
                                        let is_on = move || selected_electronics.with(|v| v.contains(&pol));
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || is_on().to_string()
                                                on:click=move |_| {
                                                    toggle_vec(selected_electronics, pol);
                                                    page.set(0);
                                                }
                                                class=move || {
                                                    if is_on() {
                                                        "min-h-[32px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-2.5 py-1 text-xs font-semibold text-cyan-100 transition"
                                                    } else {
                                                        "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                    }
                                                }
                                            >
                                                {pol.label()}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                        <div class="space-y-2.5">
                            <div>
                                <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                    "Smoking Policy"
                                </p>
                                <div class="flex flex-wrap gap-1.5">
                                    {SmokingPolicy::ALL
                                        .iter()
                                        .copied()
                                        .map(|pol| {
                                            let is_on = move || selected_smoking.with(|v| v.contains(&pol));
                                            view! {
                                                <button
                                                    type="button"
                                                    aria-pressed=move || is_on().to_string()
                                                    on:click=move |_| {
                                                        toggle_vec(selected_smoking, pol);
                                                        page.set(0);
                                                    }
                                                    class=move || {
                                                        if is_on() {
                                                            "min-h-[32px] rounded-full border border-amber-400/60 bg-amber-500/20 px-2.5 py-1 text-xs font-semibold text-amber-100 transition"
                                                        } else {
                                                            "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                        }
                                                    }
                                                >
                                                    {pol.label()}
                                                </button>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            </div>
                            <div>
                                <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                    "Vaping Policy"
                                </p>
                                <div class="flex flex-wrap gap-1.5">
                                    {VapingPolicy::ALL
                                        .iter()
                                        .copied()
                                        .map(|pol| {
                                            let is_on = move || selected_vaping.with(|v| v.contains(&pol));
                                            view! {
                                                <button
                                                    type="button"
                                                    aria-pressed=move || is_on().to_string()
                                                    on:click=move |_| {
                                                        toggle_vec(selected_vaping, pol);
                                                        page.set(0);
                                                    }
                                                    class=move || {
                                                        if is_on() {
                                                            "min-h-[32px] rounded-full border border-amber-400/60 bg-amber-500/20 px-2.5 py-1 text-xs font-semibold text-amber-100 transition"
                                                        } else {
                                                            "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                        }
                                                    }
                                                >
                                                    {pol.label()}
                                                </button>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>

            // Drawer 2: Clinical Staff License Types (MD != DO, Psychiatrist != NP/PA)
            <Show when=move || show_staff_filters.get()>
                <div class="space-y-4 rounded-xl border border-amber-400/25 bg-slate-950/75 p-4">
                    <div class="flex flex-wrap items-center justify-between gap-2">
                        <div>
                            <p class="text-xs font-semibold uppercase tracking-wider text-amber-300">
                                "Clinical Staff License Types"
                            </p>
                            <p class="mt-0.5 text-[11px] text-slate-400">
                                "Exact licensure only: MD ≠ DO, and a Psychiatrist (MD/DO) ≠ a Psychiatric Nurse Practitioner (PMHNP), NP, or PA."
                            </p>
                        </div>
                        <div class="flex items-center gap-1.5 text-xs" role="group" aria-label="Staff license match mode">
                            <span class="text-slate-400">"Match:"</span>
                            <button
                                type="button"
                                aria-pressed=move || (staff_match_mode.get() == StaffMatchMode::Any).to_string()
                                on:click=move |_| {
                                    staff_match_mode.set(StaffMatchMode::Any);
                                    page.set(0);
                                }
                                class=move || {
                                    if staff_match_mode.get() == StaffMatchMode::Any {
                                        "rounded-full border border-amber-400/60 bg-amber-500/20 px-2.5 py-1 font-semibold text-amber-100"
                                    } else {
                                        "rounded-full border border-white/10 px-2.5 py-1 text-slate-400 hover:text-white"
                                    }
                                }
                            >
                                "Any selected"
                            </button>
                            <button
                                type="button"
                                aria-pressed=move || (staff_match_mode.get() == StaffMatchMode::All).to_string()
                                on:click=move |_| {
                                    staff_match_mode.set(StaffMatchMode::All);
                                    page.set(0);
                                }
                                class=move || {
                                    if staff_match_mode.get() == StaffMatchMode::All {
                                        "rounded-full border border-amber-400/60 bg-amber-500/20 px-2.5 py-1 font-semibold text-amber-100"
                                    } else {
                                        "rounded-full border border-white/10 px-2.5 py-1 text-slate-400 hover:text-white"
                                    }
                                }
                            >
                                "Require all"
                            </button>
                        </div>
                    </div>

                    // Dedicated vs Shared care-model quick filters
                    <div class="flex flex-wrap items-center gap-2 border-b border-white/10 pb-3">
                        <span class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                            "Care Model:"
                        </span>
                        <button
                            type="button"
                            aria-pressed=move || only_dedicated_physician.get().to_string()
                            on:click=move |_| {
                                only_dedicated_physician.update(|v| *v = !*v);
                                page.set(0);
                            }
                            class=move || {
                                if only_dedicated_physician.get() {
                                    "min-h-[32px] rounded-full border border-emerald-400/60 bg-emerald-500/20 px-3 py-1 text-xs font-semibold text-emerald-100 transition"
                                } else {
                                    "min-h-[32px] rounded-full border border-white/15 bg-white/[0.03] px-3 py-1 text-xs text-slate-300 transition hover:border-emerald-400/40 hover:text-white"
                                }
                            }
                        >
                            {move || {
                                let n = offered_facets.get().dedicated_physician_count;
                                if n > 0 {
                                    format!("Dedicated single-site MD/DO on-site ({n})")
                                } else {
                                    "Dedicated single-site MD/DO on-site".to_string()
                                }
                            }}
                        </button>
                        <button
                            type="button"
                            aria-pressed=move || only_shared_network.get().to_string()
                            on:click=move |_| {
                                only_shared_network.update(|v| *v = !*v);
                                page.set(0);
                            }
                            class=move || {
                                if only_shared_network.get() {
                                    "min-h-[32px] rounded-full border border-indigo-400/60 bg-indigo-500/20 px-3 py-1 text-xs font-semibold text-indigo-100 transition"
                                } else {
                                    "min-h-[32px] rounded-full border border-white/15 bg-white/[0.03] px-3 py-1 text-xs text-slate-300 transition hover:border-indigo-400/40 hover:text-white"
                                }
                            }
                        >
                            {move || {
                                let n = offered_facets.get().shared_network_facility_count;
                                if n > 0 {
                                    format!("Shares staff across facilities ({n})")
                                } else {
                                    "Shares staff across facilities".to_string()
                                }
                            }}
                        </button>
                    </div>

                    <div class="space-y-3">
                        {StaffLicenseTier::ALL
                            .iter()
                            .copied()
                            .map(|tier| {
                                let licenses: Vec<StaffLicense> = StaffLicense::ALL
                                    .iter()
                                    .copied()
                                    .filter(|l| l.tier() == tier)
                                    .collect();
                                view! {
                                    <div>
                                        <p class="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                            {tier.label()}
                                        </p>
                                        <div class="flex flex-wrap gap-1.5">
                                            {licenses
                                                .into_iter()
                                                .map(|lic| {
                                                    let is_on = move || selected_staff.with(|v| v.contains(&lic));
                                                    view! {
                                                        <button
                                                            type="button"
                                                            aria-pressed=move || is_on().to_string()
                                                            on:click=move |_| {
                                                                toggle_vec(selected_staff, lic);
                                                                page.set(0);
                                                            }
                                                            class=move || {
                                                                if is_on() {
                                                                    "min-h-[32px] rounded-full border border-amber-400/60 bg-amber-500/20 px-2.5 py-1 text-xs font-semibold text-amber-100 transition"
                                                                } else {
                                                                    "min-h-[32px] rounded-full border border-white/10 bg-white/[0.03] px-2.5 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                                }
                                                            }
                                                        >
                                                            {move || {
                                                                let count = staff_counts
                                                                    .with(|m| m.get(&lic).copied().unwrap_or(0));
                                                                if count > 0 {
                                                                    format!("{} ({count})", lic.label())
                                                                } else {
                                                                    lic.label().to_string()
                                                                }
                                                            }}
                                                        </button>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
            </Show>

            // Drawer 2b: Cross-Facility Shared Staff & Visiting Provider Network Explorer
            <Show when=move || show_network_explorer.get()>
                <div class="space-y-3 rounded-xl border border-indigo-400/30 bg-slate-950/80 p-4">
                    <div class="flex flex-wrap items-baseline justify-between gap-2">
                        <div>
                            <p class="text-xs font-semibold uppercase tracking-wider text-indigo-300">
                                "Shared Staff & Visiting Provider Network"
                            </p>
                            <p class="mt-0.5 text-[11px] text-slate-400">
                                "Many clinics share visiting prescribers (e.g. traveling PMHNPs or psychiatrists) and local wellness specialists (e.g. yoga instructors). Select a provider to compare every facility they serve."
                            </p>
                        </div>
                        <Show when=move || selected_provider_id.get().is_some()>
                            <button
                                type="button"
                                on:click=move |_| {
                                    selected_provider_id.set(None);
                                    page.set(0);
                                }
                                class="text-xs font-medium text-indigo-200 underline-offset-2 hover:underline"
                            >
                                "Clear provider filter"
                            </button>
                        </Show>
                    </div>
                    <div class="flex flex-wrap gap-1.5">
                        {move || {
                            top_network_providers
                                .get()
                                .into_iter()
                                .map(|prov| {
                                    let pid_pressed = prov.id.clone();
                                    let pid_class = prov.id.clone();
                                    let pid_click = prov.id.clone();
                                    let org_part = prov
                                        .organization
                                        .as_deref()
                                        .map(|o| format!(" · {o}"))
                                        .unwrap_or_default();
                                    let label = format!(
                                        "{}{} · {} ({} sites)",
                                        prov.name,
                                        org_part,
                                        prov.role.short_label(),
                                        prov.facility_ids.len()
                                    );
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || {
                                                (selected_provider_id.get().as_deref()
                                                    == Some(pid_pressed.as_str()))
                                                    .to_string()
                                            }
                                            on:click=move |_| {
                                                selected_provider_id.update(|cur| {
                                                    if cur.as_deref() == Some(pid_click.as_str()) {
                                                        *cur = None;
                                                    } else {
                                                        *cur = Some(pid_click.clone());
                                                    }
                                                });
                                                page.set(0);
                                            }
                                            class=move || {
                                                if selected_provider_id.get().as_deref()
                                                    == Some(pid_class.as_str())
                                                {
                                                    "min-h-[32px] rounded-full border border-indigo-400/70 bg-indigo-500/25 px-3 py-1 text-xs font-semibold text-indigo-100 transition"
                                                } else {
                                                    "min-h-[32px] rounded-full border border-white/15 bg-white/[0.03] px-3 py-1 text-xs text-slate-200 transition hover:border-indigo-400/45 hover:text-white"
                                                }
                                            }
                                        >
                                            {label}
                                        </button>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                </div>
            </Show>

            // Active shared-provider filter banner
            {move || {
                let pid = selected_provider_id.get()?;
                let prov = network_by_id.with(|m| m.get(&pid).cloned())?;
                let org_text = prov
                    .organization
                    .as_deref()
                    .map(|o| format!(" ({o})"))
                    .unwrap_or_default();
                let summary = format!(
                    "{}{org_text} · {} · {} · {} facilities",
                    prov.name,
                    prov.role.short_label(),
                    prov.arrangement.label(),
                    prov.facility_ids.len()
                );
                let notes = prov.notes.clone();
                Some(view! {
                    <div
                        role="status"
                        class="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-indigo-400/40 bg-indigo-500/15 px-4 py-2.5 text-xs text-indigo-100"
                    >
                        <div>
                            <p>
                                <strong>"Shared provider filter: "</strong>
                                {summary}
                            </p>
                            {notes.map(|n| view! { <p class="mt-0.5 text-[11px] text-indigo-200/80">{n}</p> })}
                        </div>
                        <button
                            type="button"
                            on:click=move |_| {
                                selected_provider_id.set(None);
                                page.set(0);
                            }
                            class="min-h-[32px] rounded-full border border-indigo-300/40 bg-indigo-500/20 px-3 py-1 text-xs font-semibold text-white transition hover:bg-indigo-500/35"
                        >
                            "Show all facilities"
                        </button>
                    </div>
                })
            }}

            // Drawer 3: Care Settings & Payment / Insurance
            <Show when=move || show_care_filters.get()>
                <div class="space-y-4 rounded-xl border border-white/10 bg-slate-950/60 p-4">
                    <div>
                        <p class="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-400">
                            "Care Setting (matches any selected)"
                        </p>
                        <div class="flex flex-wrap gap-1.5">
                            {move || {
                                offered_settings
                                    .get()
                                    .into_iter()
                                    .map(|setting| {
                                        let is_on = move || selected_settings.with(|v| v.contains(&setting));
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || is_on().to_string()
                                                on:click=move |_| {
                                                    toggle_vec(selected_settings, setting);
                                                    page.set(0);
                                                }
                                                class=move || {
                                                    if is_on() {
                                                        "min-h-[34px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1 text-xs font-semibold text-cyan-100 transition"
                                                    } else {
                                                        "min-h-[34px] rounded-full border border-white/10 bg-white/[0.03] px-3 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                    }
                                                }
                                            >
                                                {setting.label()}
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>

                    <div class="border-t border-white/10 pt-3">
                        <div class="mb-2 flex items-center justify-between gap-2">
                            <p class="text-xs font-semibold uppercase tracking-wider text-slate-400">
                                "Insurance & Payment Assistance (matches any selected)"
                            </p>
                            <button
                                type="button"
                                on:click=move |_| show_all_payments.update(|v| *v = !*v)
                                class="text-xs text-cyan-300 underline-offset-2 hover:underline"
                            >
                                {move || if show_all_payments.get() { "Show common only" } else { "Show all grant & state funds" }}
                            </button>
                        </div>
                        <div class="flex flex-wrap gap-1.5">
                            {move || {
                                offered_payments
                                    .get()
                                    .into_iter()
                                    .map(|payment| {
                                        let is_on = move || selected_payments.with(|v| v.contains(&payment));
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || is_on().to_string()
                                                on:click=move |_| {
                                                    toggle_vec(selected_payments, payment);
                                                    page.set(0);
                                                }
                                                class=move || {
                                                    if is_on() {
                                                        "min-h-[34px] rounded-full border border-emerald-400/60 bg-emerald-500/20 px-3 py-1 text-xs font-semibold text-emerald-100 transition"
                                                    } else {
                                                        "min-h-[34px] rounded-full border border-white/10 bg-white/[0.03] px-3 py-1 text-xs text-slate-300 transition hover:border-white/25 hover:text-white"
                                                    }
                                                }
                                            >
                                                {payment.label()}
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>
                </div>
            </Show>

            // Saved-to-care-team confirmation banner
            {move || {
                saved_facility_name.get().map(|name| {
                    view! {
                        <div
                            role="status"
                            aria-live="polite"
                            class="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-emerald-400/35 bg-emerald-500/15 px-4 py-2.5 text-xs text-emerald-100"
                        >
                            <span>
                                <strong>
                                    {move || format!("{}: ", t(locale.get(), Msg::RecoverySavedCareTeam))}
                                </strong>
                                {name}
                                " (open Care & Brief tab to review or print)."
                            </span>
                            <button
                                type="button"
                                on:click=move |_| saved_facility_name.set(None)
                                class="text-emerald-200 underline-offset-2 hover:underline"
                            >
                                "Dismiss"
                            </button>
                        </div>
                    }
                })
            }}

            // Results area
            {move || {
                if let Some(err) = load_error.get() {
                    return view! {
                        <div
                            role="alert"
                            class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-rose-400/30 bg-rose-950/30 p-4 text-sm text-rose-100"
                        >
                            <span>{err}</span>
                            <button
                                type="button"
                                on:click=move |_| reload_tick.update(|n| *n = n.wrapping_add(1))
                                class="min-h-[38px] rounded-full border border-rose-300/40 bg-rose-500/20 px-4 py-1.5 text-xs font-semibold text-white transition hover:bg-rose-500/30"
                            >
                                "Try again"
                            </button>
                        </div>
                    }
                    .into_any();
                }

                if let Some(code) = loading_state.get() {
                    return view! {
                        <div
                            role="status"
                            aria-live="polite"
                            class="space-y-3 rounded-xl border border-white/10 bg-slate-950/50 p-6 text-sm text-slate-300"
                        >
                            <p class="font-medium text-white">
                                {format!("Loading {} ({}) facility directory…", state_full_name(&code), code)}
                            </p>
                            <div class="grid gap-3 md:grid-cols-2">
                                <div class="h-28 animate-pulse rounded-xl bg-white/[0.04]"></div>
                                <div class="h-28 animate-pulse rounded-xl bg-white/[0.04]"></div>
                            </div>
                        </div>
                    }
                    .into_any();
                }

                let Some(results) = search_results.get() else {
                    let code = selected_state.get();
                    return view! {
                        <div class="rounded-xl border border-white/10 bg-slate-950/50 p-5 text-sm text-slate-300">
                            {format!(
                                "Select a state above to search mental health and recovery programs in {} ({code}).",
                                state_full_name(&code)
                            )}
                        </div>
                    }
                    .into_any();
                };

                let code = selected_state.get();
                let total = results.total;
                let cur_page = results.page;
                let total_pages = results.page_count;

                view! {
                    <div class="space-y-4">
                        // Summary & sort bar
                        <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-slate-300">
                            <p role="status" aria-live="polite">
                                <strong class="text-white">{total}</strong>
                                {format!(
                                    " matching {} in {} ({code})",
                                    if total == 1 { "facility" } else { "facilities" },
                                    state_full_name(&code)
                                )}
                                {if total_pages > 1 {
                                    format!(" · Page {} of {}", cur_page + 1, total_pages)
                                } else {
                                    String::new()
                                }}
                            </p>
                            <div class="flex items-center gap-1.5">
                                <button
                                    type="button"
                                    aria-pressed=move || (sort_mode.get() == RecoverySort::Name).to_string()
                                    on:click=move |_| {
                                        sort_mode.set(RecoverySort::Name);
                                        page.set(0);
                                    }
                                    class=move || {
                                        if sort_mode.get() == RecoverySort::Name {
                                            "rounded-full border border-emerald-400/50 bg-emerald-500/15 px-3 py-1 font-semibold text-emerald-200"
                                        } else {
                                            "rounded-full border border-white/10 px-3 py-1 text-slate-400 hover:text-white"
                                        }
                                    }
                                >
                                    {move || t(locale.get(), Msg::RecoverySortName)}
                                </button>
                                <button
                                    type="button"
                                    disabled=move || origin.get().is_none()
                                    aria-pressed=move || (sort_mode.get() == RecoverySort::Distance).to_string()
                                    on:click=move |_| {
                                        sort_mode.set(RecoverySort::Distance);
                                        page.set(0);
                                    }
                                    class=move || {
                                        if sort_mode.get() == RecoverySort::Distance && origin.get().is_some() {
                                            "rounded-full border border-cyan-400/50 bg-cyan-500/15 px-3 py-1 font-semibold text-cyan-200"
                                        } else {
                                            "rounded-full border border-white/10 px-3 py-1 text-slate-400 hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
                                        }
                                    }
                                >
                                    {move || t(locale.get(), Msg::RecoverySortDistance)}
                                </button>
                            </div>
                        </div>

                        {if results.hits.is_empty() {
                            view! {
                                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-6 text-center text-sm text-slate-300">
                                    <p class="font-medium text-white">"No facilities matched these filters."</p>
                                    <p class="mt-1 text-xs text-slate-400">
                                        "Try clearing an amenity or staff license filter, widening your radius, or switching states."
                                    </p>
                                    <button
                                        type="button"
                                        on:click=clear_all_filters
                                        class="mt-3 min-h-[38px] rounded-full border border-emerald-400/40 bg-emerald-500/15 px-4 py-1.5 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/25"
                                    >
                                        {move || t(locale.get(), Msg::RecoveryClearFilters)}
                                    </button>
                                </div>
                            }
                            .into_any()
                        } else {
                            view! {
                                <div class="grid gap-4 md:grid-cols-2">
                                    {results
                                        .hits
                                        .into_iter()
                                        .map(|hit| {
                                            let shared_providers: Vec<NetworkProvider> = hit
                                                .facility
                                                .staffing_profile
                                                .as_ref()
                                                .map(|prof| {
                                                    network_by_id.with(|map| {
                                                        prof.network_provider_ids
                                                            .iter()
                                                            .filter_map(|id| map.get(id).cloned())
                                                            .collect()
                                                    })
                                                })
                                                .unwrap_or_default();
                                            render_facility_card(
                                                hit,
                                                shared_providers,
                                                selected_provider_id,
                                                page,
                                                on_save_care_team,
                                                saved_facility_name,
                                            )
                                        })
                                        .collect_view()}
                                </div>

                                // Pagination
                                {if total_pages > 1 {
                                    let is_first_page = cur_page == 0;
                                    let is_last_page = cur_page + 1 >= total_pages;
                                    view! {
                                        <div class="flex items-center justify-between border-t border-white/10 pt-4 text-xs">
                                            <button
                                                type="button"
                                                disabled=is_first_page
                                                on:click=move |_| page.update(|p| *p = p.saturating_sub(1))
                                                class="min-h-[40px] rounded-full border border-white/15 px-4 py-1.5 font-medium text-slate-200 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-40"
                                            >
                                                {move || t(locale.get(), Msg::RecoveryPrevPage)}
                                            </button>
                                            <span class="text-slate-400">
                                                {format!("Page {} of {}", cur_page + 1, total_pages)}
                                            </span>
                                            <button
                                                type="button"
                                                disabled=is_last_page
                                                on:click=move |_| page.update(|p| *p = p.saturating_add(1))
                                                class="min-h-[40px] rounded-full border border-white/15 px-4 py-1.5 font-medium text-slate-200 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-40"
                                            >
                                                {move || t(locale.get(), Msg::RecoveryNextPage)}
                                            </button>
                                        </div>
                                    }
                                    .into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }}
                            }
                            .into_any()
                        }}
                    </div>
                }
                .into_any()
            }}
        </section>
    }
}

fn toggle_vec<T: PartialEq + Copy + Send + Sync + 'static>(sig: RwSignal<Vec<T>>, item: T) {
    sig.update(|list| {
        if let Some(pos) = list.iter().position(|x| *x == item) {
            list.remove(pos);
        } else {
            list.push(item);
        }
    });
}

fn staff_badge_class(lic: StaffLicense) -> &'static str {
    match lic.tier() {
        StaffLicenseTier::Physicians => {
            "rounded-full border border-amber-400/45 bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-100"
        }
        StaffLicenseTier::AdvancedPractice => {
            "rounded-full border border-cyan-400/40 bg-cyan-500/15 px-2 py-0.5 text-[10px] font-medium text-cyan-100"
        }
        StaffLicenseTier::Psychologists => {
            "rounded-full border border-violet-400/40 bg-violet-500/15 px-2 py-0.5 text-[10px] font-medium text-violet-100"
        }
        StaffLicenseTier::TherapistsAndSocialWorkers => {
            "rounded-full border border-sky-400/35 bg-sky-500/10 px-2 py-0.5 text-[10px] text-sky-100"
        }
        StaffLicenseTier::NursingAndAddictionSupport => {
            "rounded-full border border-white/15 bg-white/[0.05] px-2 py-0.5 text-[10px] text-slate-200"
        }
    }
}

fn render_facility_card(
    hit: RecoveryHit,
    shared_providers: Vec<NetworkProvider>,
    selected_provider_id: RwSignal<Option<String>>,
    page: RwSignal<usize>,
    on_save_care_team: Option<Callback<SavedCareTeamFacility>>,
    saved_facility_name: RwSignal<Option<String>>,
) -> impl IntoView {
    let locale = use_locale();
    let f = hit.facility;
    let dist = hit.distance_km.map(format_distance);
    let addr_line = f.address.display_line();
    let tel = f.tel_href();
    let phone_display = f.phone.clone();
    let website = f.safe_website().map(str::to_string);
    let maps = f.maps_href();
    let care_badges = f.care.clone();
    let setting_badges: Vec<&'static str> = f.settings.iter().map(|s| s.label()).collect();
    let payment_badges: Vec<&'static str> = f.payments.iter().map(|p| p.label()).collect();
    let group_badges: Vec<&'static str> = f.groups.iter().map(|g| g.label()).collect();
    let staff_licenses = f.staff_licenses.clone();
    let staffing_profile = f.staffing_profile.clone();
    let amenities = f.lifestyle.amenities.clone();
    let food = f.lifestyle.food;
    let dietary = f.lifestyle.dietary.clone();
    let electronics = f.lifestyle.electronics;
    let smoking = f.lifestyle.smoking;
    let vaping = f.lifestyle.vaping;
    let lifestyle_notes = f.lifestyle.notes.clone();
    let enrichment = f.enrichment.clone();
    let has_living_details = !amenities.is_empty()
        || food.is_some()
        || !dietary.is_empty()
        || electronics.is_some()
        || smoking.is_some()
        || vaping.is_some();

    let save_name = match &f.site_name {
        Some(site) if !site.trim().is_empty() => format!("{} ({})", f.name, site),
        _ => f.name.clone(),
    };
    let save_phone = f.phone.clone().unwrap_or_default();
    let save_addr = addr_line.clone();

    view! {
        <article class="flex flex-col justify-between gap-3 rounded-xl border border-white/10 bg-slate-950/75 p-4 transition hover:border-white/20">
            <div class="space-y-2.5">
                <div class="flex flex-wrap items-start justify-between gap-2">
                    <div>
                        <h3 class="text-sm font-semibold text-white">{f.name.clone()}</h3>
                        {f.site_name.clone().filter(|s| !s.trim().is_empty()).map(|site| {
                            view! { <p class="text-xs text-slate-400">{site}</p> }
                        })}
                    </div>
                    <div class="flex flex-wrap items-center gap-1.5">
                        {enrichment.as_ref().map(|meta| {
                            let sources = meta.sources.clone();
                            view! {
                                {sources.into_iter().map(|src| {
                                    let cls = match src {
                                        DataSource::Verified => "rounded-full border border-emerald-400/50 bg-emerald-500/20 px-2 py-0.5 text-[10px] font-semibold text-emerald-200",
                                        DataSource::Nppes => "rounded-full border border-amber-400/35 bg-amber-500/10 px-2 py-0.5 text-[10px] text-amber-200",
                                        DataSource::Website => "rounded-full border border-teal-400/35 bg-teal-500/10 px-2 py-0.5 text-[10px] text-teal-200",
                                        DataSource::Llm => "rounded-full border border-indigo-400/40 bg-indigo-500/15 px-2 py-0.5 text-[10px] text-indigo-200",
                                        DataSource::Samhsa => "rounded-full border border-white/10 bg-white/[0.04] px-2 py-0.5 text-[10px] text-slate-400",
                                    };
                                    view! { <span class=cls>{src.label()}</span> }
                                }).collect_view()}
                            }
                        })}
                        {dist.map(|d| {
                            view! {
                                <span class="shrink-0 rounded-full border border-cyan-400/35 bg-cyan-500/15 px-2.5 py-0.5 text-[11px] font-semibold text-cyan-200">
                                    {d}
                                </span>
                            }
                        })}
                    </div>
                </div>

                // Care type & setting badges
                <div class="flex flex-wrap gap-1.5">
                    {care_badges
                        .into_iter()
                        .map(|c| {
                            let cls = match c {
                                CareType::MentalHealth => "rounded-full border border-violet-400/35 bg-violet-500/15 px-2 py-0.5 text-[11px] font-medium text-violet-200",
                                CareType::SubstanceUse => "rounded-full border border-emerald-400/35 bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-200",
                            };
                            view! { <span class=cls>{c.label()}</span> }
                        })
                        .collect_view()}
                    {setting_badges
                        .iter()
                        .take(3)
                        .copied()
                        .map(|label| {
                            view! {
                                <span class="rounded-full border border-white/10 bg-white/[0.04] px-2 py-0.5 text-[11px] text-slate-300">
                                    {label}
                                </span>
                            }
                        })
                        .collect_view()}
                </div>

                // Staff Licenses & Dedicated vs Shared/Visiting Network (MD != DO, Psychiatrist != NP)
                {(!staff_licenses.is_empty() || staffing_profile.is_some()).then(|| {
                    let nppes_count = enrichment.as_ref().and_then(|m| m.nppes_provider_count);
                    let prof = staffing_profile.clone();
                    let top_shared: Vec<NetworkProvider> =
                        shared_providers.into_iter().take(4).collect();
                    view! {
                        <div class="space-y-1.5 rounded-lg border border-amber-400/20 bg-amber-950/15 p-2">
                            <div class="flex flex-wrap items-center justify-between gap-2">
                                <span class="text-[10px] font-semibold uppercase tracking-wider text-amber-200/90">
                                    "Staff Licenses & Care Model"
                                </span>
                                {nppes_count.map(|n| {
                                    view! {
                                        <span class="text-[10px] text-slate-400">
                                            {format!("{n} NPPES matched")}
                                        </span>
                                    }
                                })}
                            </div>
                            {prof.as_ref().map(|p| {
                                let has_model = p.dedicated_physicians > 0
                                    || p.dedicated_clinicians > 0
                                    || p.shared_prescribers > 0
                                    || p.shared_with_facilities > 0
                                    || p.visiting_specialists > 0;
                                has_model.then(|| {
                                    let ded_phys = p.dedicated_physicians;
                                    let ded_clin = p.dedicated_clinicians;
                                    let sh_presc = p.shared_prescribers;
                                    let sh_peers = p.shared_with_facilities;
                                    let visiting = p.visiting_specialists;
                                    view! {
                                        <div class="flex flex-wrap gap-1">
                                            {(ded_phys > 0).then(|| {
                                                view! {
                                                    <span class="rounded border border-emerald-400/40 bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-200">
                                                        {format!("{ded_phys} dedicated MD/DO")}
                                                    </span>
                                                }
                                            })}
                                            {(ded_clin > 0 && ded_phys == 0).then(|| {
                                                view! {
                                                    <span class="rounded border border-emerald-400/30 bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-200">
                                                        {format!("{ded_clin} dedicated staff")}
                                                    </span>
                                                }
                                            })}
                                            {(sh_presc > 0).then(|| {
                                                view! {
                                                    <span class="rounded border border-amber-400/40 bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-100">
                                                        {format!("{sh_presc} shared prescriber{}", if sh_presc == 1 { "" } else { "s" })}
                                                    </span>
                                                }
                                            })}
                                            {(visiting > 0).then(|| {
                                                view! {
                                                    <span class="rounded border border-indigo-400/40 bg-indigo-500/15 px-1.5 py-0.5 text-[10px] font-medium text-indigo-200">
                                                        {format!("{visiting} visiting specialist{}", if visiting == 1 { "" } else { "s" })}
                                                    </span>
                                                }
                                            })}
                                            {(sh_peers > 0).then(|| {
                                                view! {
                                                    <span class="rounded border border-indigo-400/30 bg-indigo-500/10 px-1.5 py-0.5 text-[10px] text-indigo-200">
                                                        {format!("Shares staff with {sh_peers} peer {}", if sh_peers == 1 { "site" } else { "sites" })}
                                                    </span>
                                                }
                                            })}
                                        </div>
                                    }
                                })
                            })}
                            {(!staff_licenses.is_empty()).then(|| {
                                view! {
                                    <div class="flex flex-wrap gap-1">
                                        {staff_licenses
                                            .into_iter()
                                            .map(|lic| {
                                                view! {
                                                    <span class=staff_badge_class(lic)>
                                                        {lic.short_label()}
                                                    </span>
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                }
                            })}
                            {(!top_shared.is_empty()).then(|| {
                                view! {
                                    <div class="border-t border-white/5 pt-1.5">
                                        <span class="mb-1 block text-[10px] text-slate-400">
                                            "Shared / visiting providers (click to compare sites):"
                                        </span>
                                        <div class="flex flex-wrap gap-1">
                                            {top_shared
                                                .into_iter()
                                                .map(|sp| {
                                                    let pid = sp.id.clone();
                                                    let chip_label = format!(
                                                        "{} · {} ({} sites)",
                                                        sp.name,
                                                        sp.role.short_label(),
                                                        sp.facility_ids.len()
                                                    );
                                                    view! {
                                                        <button
                                                            type="button"
                                                            on:click=move |_| {
                                                                selected_provider_id.set(Some(pid.clone()));
                                                                page.set(0);
                                                            }
                                                            class="rounded-full border border-indigo-400/35 bg-indigo-500/15 px-2 py-0.5 text-[10px] font-medium text-indigo-100 transition hover:bg-indigo-500/30"
                                                        >
                                                            {chip_label}
                                                        </button>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </div>
                                }
                            })}
                        </div>
                    }
                })}

                // Non-care Amenities, Food & Electronic Device Policies
                {has_living_details.then(|| {
                    view! {
                        <div class="rounded-lg border border-teal-400/20 bg-teal-950/15 p-2">
                            <span class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-teal-200/90">
                                "Living, Amenities & Policies"
                            </span>
                            <div class="flex flex-wrap gap-1">
                                {food.map(|f_sit| {
                                    view! {
                                        <span class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-2 py-0.5 text-[10px] font-medium text-emerald-100">
                                            {f_sit.short_label()}
                                        </span>
                                    }
                                })}
                                {electronics.map(|el| {
                                    view! {
                                        <span class="rounded-full border border-cyan-400/40 bg-cyan-500/15 px-2 py-0.5 text-[10px] font-medium text-cyan-100">
                                            {el.short_label()}
                                        </span>
                                    }
                                })}
                                {amenities
                                    .into_iter()
                                    .map(|a| {
                                        view! {
                                            <span class="rounded-full border border-teal-400/35 bg-teal-500/15 px-2 py-0.5 text-[10px] text-teal-100">
                                                {a.label()}
                                            </span>
                                        }
                                    })
                                    .collect_view()}
                                {dietary
                                    .into_iter()
                                    .map(|d| {
                                        view! {
                                            <span class="rounded-full border border-emerald-300/25 bg-emerald-500/10 px-2 py-0.5 text-[10px] text-emerald-200">
                                                {d.label()}
                                            </span>
                                        }
                                    })
                                    .collect_view()}
                                {smoking.map(|s| {
                                    view! {
                                        <span class="rounded-full border border-white/15 bg-white/[0.04] px-2 py-0.5 text-[10px] text-slate-300">
                                            {s.label()}
                                        </span>
                                    }
                                })}
                                {vaping.map(|v| {
                                    view! {
                                        <span class="rounded-full border border-white/15 bg-white/[0.04] px-2 py-0.5 text-[10px] text-slate-300">
                                            {v.label()}
                                        </span>
                                    }
                                })}
                            </div>
                        </div>
                    }
                })}

                // Optional curated/verified notes
                {lifestyle_notes.filter(|n| !n.trim().is_empty()).map(|notes| {
                    view! {
                        <p class="text-[11px] leading-relaxed text-slate-300">{notes}</p>
                    }
                })}

                // Address
                <p class="text-xs text-slate-300">{addr_line}</p>

                // Payment summary (compact)
                {(!payment_badges.is_empty()).then(|| {
                    let shown: Vec<&str> = payment_badges.iter().take(5).copied().collect();
                    let extra = payment_badges.len().saturating_sub(shown.len());
                    let summary = if extra > 0 {
                        format!("Payment: {} (+{} more)", shown.join(" · "), extra)
                    } else {
                        format!("Payment: {}", shown.join(" · "))
                    };
                    view! { <p class="text-[11px] leading-relaxed text-slate-400">{summary}</p> }
                })}

                // Specialized programs + evidence links (collapsible)
                {(!group_badges.is_empty()).then(|| {
                    let count = group_badges.len();
                    view! {
                        <details class="text-[11px] text-slate-400">
                            <summary class="cursor-pointer select-none text-slate-300 hover:text-white">
                                {format!("Specialized programs ({count})")}
                            </summary>
                            <div class="mt-1.5 flex flex-wrap gap-1">
                                {group_badges
                                    .into_iter()
                                    .map(|g| {
                                        view! {
                                            <span class="rounded border border-white/10 bg-white/[0.03] px-1.5 py-0.5 text-[10px] text-slate-300">
                                                {g}
                                            </span>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </details>
                    }
                })}
            </div>

            // Action bar: Call + Maps + Website + Save to Care Team
            <div class="flex flex-wrap items-center gap-2 border-t border-white/10 pt-3">
                {tel.and_then(|href| {
                    phone_display.map(|label| {
                        view! {
                            <a
                                href=href
                                class="inline-flex min-h-[38px] items-center gap-1 rounded-lg border border-emerald-400/40 bg-emerald-500/15 px-3 py-1.5 text-xs font-semibold text-emerald-200 transition hover:bg-emerald-500/25"
                            >
                                <span>{move || t(locale.get(), Msg::RecoveryCallNow)}</span>
                                <span dir="ltr">{label}</span>
                            </a>
                        }
                    })
                })}
                <a
                    href=maps
                    target="_blank"
                    rel="noopener noreferrer"
                    class="inline-flex min-h-[38px] items-center rounded-lg border border-white/15 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:border-white/30 hover:text-white"
                >
                    {move || t(locale.get(), Msg::RecoveryDirections)}
                </a>
                {website.map(|url| {
                    view! {
                        <a
                            href=url
                            target="_blank"
                            rel="noopener noreferrer"
                            class="inline-flex min-h-[38px] items-center rounded-lg border border-white/15 bg-white/[0.03] px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:border-white/30 hover:text-white"
                        >
                            {move || t(locale.get(), Msg::RecoveryWebsite)}
                        </a>
                    }
                })}
                {on_save_care_team.map(|cb| {
                    let saved_payload = SavedCareTeamFacility {
                        name: save_name.clone(),
                        phone: save_phone.clone(),
                        address: save_addr.clone(),
                    };
                    let check_name = save_name.clone();
                    let check_name_cls = save_name.clone();
                    view! {
                        <button
                            type="button"
                            on:click=move |_| {
                                cb.run(saved_payload.clone());
                                saved_facility_name.set(Some(saved_payload.name.clone()));
                            }
                            class=move || {
                                let is_saved = saved_facility_name
                                    .with(|cur| cur.as_deref() == Some(check_name_cls.as_str()));
                                if is_saved {
                                    "ms-auto inline-flex min-h-[38px] items-center rounded-lg border border-emerald-400/45 bg-emerald-500/15 px-3 py-1.5 text-xs font-semibold text-emerald-200 transition"
                                } else {
                                    "ms-auto inline-flex min-h-[38px] items-center rounded-lg border border-cyan-400/35 bg-cyan-500/10 px-3 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                                }
                            }
                        >
                            {move || {
                                let loc = locale.get();
                                let is_saved = saved_facility_name
                                    .with(|cur| cur.as_deref() == Some(check_name.as_str()));
                                if is_saved {
                                    t(loc, Msg::RecoverySavedCareTeam)
                                } else {
                                    t(loc, Msg::RecoverySaveCareTeam)
                                }
                            }}
                        </button>
                    }
                })}
            </div>
        </article>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_recovery_index_loads_all_jurisdictions() {
        let index = recovery_index();
        assert!(
            index.states.len() >= 50,
            "expected >= 50 jurisdictions, found {}",
            index.states.len()
        );
        let co = index
            .states
            .iter()
            .find(|s| s.state == "CO")
            .expect("Colorado entry present");
        assert!(co.facilities > 100);
    }

    #[test]
    fn real_colorado_shard_parses_and_filters_accurately() {
        let raw = include_str!("../../../public/recovery/CO.json");
        let catalog: StateCatalog = serde_json::from_str(raw).expect("CO.json parses cleanly");
        assert_eq!(catalog.state, "CO");
        assert!(catalog.facilities.len() > 200);

        let results = search_catalog(
            &catalog,
            &RecoveryQuery {
                text: "Denver".to_string(),
                care: vec![CareType::MentalHealth],
                settings: vec![Setting::Outpatient],
                payments: vec![Payment::Medicaid],
                radius_km: DEFAULT_RADIUS_KM,
                ..RecoveryQuery::default()
            },
        );
        assert!(results.total > 0);
        assert!(results.hits.len() <= PAGE_SIZE);
        for hit in &results.hits {
            assert!(hit.facility.care.contains(&CareType::MentalHealth));
            assert!(hit.facility.payments.contains(&Payment::Medicaid));
        }

        let near = search_catalog(
            &catalog,
            &RecoveryQuery {
                origin: Some(Coordinates {
                    lat: 39.7392,
                    lng: -104.9903,
                }),
                radius_km: 25,
                sort: RecoverySort::Distance,
                ..RecoveryQuery::default()
            },
        );
        assert!(near.total > 0);
        let distances: Vec<f64> = near.hits.iter().filter_map(|h| h.distance_km).collect();
        assert!(distances.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn coordinate_parser_validates_bounds_and_format() {
        let ok = parse_coordinates("39.7392, -104.9903").expect("valid coordinates");
        assert!((ok.lat - 39.7392).abs() < 1e-6);
        assert!((ok.lng - (-104.9903)).abs() < 1e-6);
        assert!(parse_coordinates("95.0, -104.0").is_err());
        assert!(parse_coordinates("not-a-point").is_err());
    }

    #[test]
    fn recovery_search_renders_cleanly_on_ssr() {
        let html = view! { <RecoverySearchSection /> }.to_html();
        assert!(html.contains("Find Mental Health &amp; Recovery Care Near You"));
        assert!(html.contains("Colorado"));
        assert!(html.contains("tel:988"));
        assert!(html.contains("Amenities, Food &amp; Devices"));
        assert!(html.contains("Staff Licenses"));
        assert!(html.contains("100% On-Device Search"));
    }
}
