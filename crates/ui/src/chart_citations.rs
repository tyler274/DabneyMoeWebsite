//! Reusable, accessible patient/physician chart citation linking and tooltip
//! components shared between Curie Clinic (`/clinical/physician`) and Curie
//! Mobile (`/clinical/patient`).
//!
//! All citation matching is strictly scoped to the active patient's own record
//! IDs so foreign chart identifiers are never linked or surfaced across patient
//! boundaries.

use std::collections::HashMap;

use clinical_workspace::{ChartRecord, ChatCitation, RecordKind};
use leptos::prelude::*;

use crate::i18n::{t, use_locale, Locale, Msg};

/// Visual accent palette for citation badges, cards, and tooltips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CitationAccent {
    /// Curie Clinic physician workspace accent.
    #[default]
    Cyan,
    /// Curie Mobile patient workspace accent.
    Emerald,
}

impl CitationAccent {
    const fn badge_link_class(self) -> &'static str {
        match self {
            Self::Cyan => "inline-flex items-center gap-1 rounded-md border border-cyan-400/45 bg-cyan-500/15 px-1.5 py-0.5 font-mono text-[11px] font-semibold leading-tight text-cyan-200 transition hover:border-cyan-300 hover:bg-cyan-500/25 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400",
            Self::Emerald => "inline-flex items-center gap-1 rounded-md border border-emerald-400/45 bg-emerald-500/15 px-1.5 py-0.5 font-mono text-[11px] font-semibold leading-tight text-emerald-200 transition hover:border-emerald-300 hover:bg-emerald-500/25 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400",
        }
    }

    const fn badge_tooltip_open_class(self) -> &'static str {
        match self {
            Self::Cyan => "pointer-events-auto absolute bottom-full left-1/2 z-50 mb-2 block w-72 max-w-[85vw] -translate-x-1/2 rounded-xl border border-cyan-400/40 bg-slate-950/95 p-3 text-start text-[11px] font-normal not-italic normal-case tracking-normal leading-relaxed text-slate-100 shadow-2xl shadow-black/60 backdrop-blur sm:w-80",
            Self::Emerald => "pointer-events-auto absolute bottom-full left-1/2 z-50 mb-2 block w-72 max-w-[85vw] -translate-x-1/2 rounded-xl border border-emerald-400/40 bg-slate-950/95 p-3 text-start text-[11px] font-normal not-italic normal-case tracking-normal leading-relaxed text-slate-100 shadow-2xl shadow-black/60 backdrop-blur sm:w-80",
        }
    }

    const fn tooltip_id_badge_class(self) -> &'static str {
        match self {
            Self::Cyan => "rounded bg-cyan-500/20 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-cyan-200",
            Self::Emerald => "rounded bg-emerald-500/20 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-emerald-200",
        }
    }

    const fn tooltip_cta_class(self) -> &'static str {
        match self {
            Self::Cyan => "mt-2 block border-t border-white/10 pt-1.5 text-[10px] font-semibold text-cyan-300",
            Self::Emerald => "mt-2 block border-t border-white/10 pt-1.5 text-[10px] font-semibold text-emerald-300",
        }
    }

    const fn card_link_class(self) -> &'static str {
        match self {
            Self::Cyan => "group block rounded-xl border border-cyan-400/40 bg-cyan-500/[0.08] p-3 text-start transition hover:border-cyan-300/70 hover:bg-cyan-500/[0.14] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400",
            Self::Emerald => "group block rounded-xl border border-emerald-400/40 bg-emerald-500/[0.08] p-3 text-start transition hover:border-emerald-300/70 hover:bg-emerald-500/[0.14] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400",
        }
    }

    const fn card_id_badge_class(self) -> &'static str {
        match self {
            Self::Cyan => "rounded bg-cyan-500/20 px-2 py-0.5 font-mono text-[11px] font-semibold text-cyan-200",
            Self::Emerald => "rounded bg-emerald-500/20 px-2 py-0.5 font-mono text-[11px] font-semibold text-emerald-200",
        }
    }

    const fn card_cta_class(self) -> &'static str {
        match self {
            Self::Cyan => {
                "text-[11px] font-semibold text-cyan-300 transition group-hover:text-cyan-200"
            }
            Self::Emerald => {
                "text-[11px] font-semibold text-emerald-300 transition group-hover:text-emerald-200"
            }
        }
    }

    const fn card_tooltip_open_class(self) -> &'static str {
        match self {
            Self::Cyan => "pointer-events-auto absolute bottom-full left-1/2 z-50 mb-2 w-72 max-w-[90vw] -translate-x-1/2 rounded-xl border border-cyan-400/40 bg-slate-950/95 p-3 text-start text-[11px] font-normal leading-relaxed text-slate-100 shadow-2xl shadow-black/60 backdrop-blur sm:w-80",
            Self::Emerald => "pointer-events-auto absolute bottom-full left-1/2 z-50 mb-2 w-72 max-w-[90vw] -translate-x-1/2 rounded-xl border border-emerald-400/40 bg-slate-950/95 p-3 text-start text-[11px] font-normal leading-relaxed text-slate-100 shadow-2xl shadow-black/60 backdrop-blur sm:w-80",
        }
    }
}

const fn record_kind_badge_class(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::LabResult => "border-violet-400/30 bg-violet-500/15 text-violet-300",
        RecordKind::ProgressNote => "border-cyan-400/30 bg-cyan-500/15 text-cyan-300",
        RecordKind::CgmSummary => "border-emerald-400/30 bg-emerald-500/15 text-emerald-300",
        RecordKind::PatientMessage => "border-amber-400/30 bg-amber-500/15 text-amber-300",
        RecordKind::Imaging => "border-sky-400/30 bg-sky-500/15 text-sky-300",
    }
}

/// Deterministic FNV-1a ID so SSR and client hydration always emit the exact
/// same tooltip `id` and `aria-describedby` reference.
fn deterministic_citation_tip_id(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            hash ^= u64::from(b':');
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        for byte in part.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("cite-tip-{hash:016x}")
}

/// A parsed segment of prose containing zero or more patient-scoped record ID
/// citations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitationTextSegment {
    /// Unmodified plain text segment.
    Plain(String),
    /// A matched citation token pointing to a valid record ID on the active
    /// patient's chart.
    Citation {
        /// Exact matched substring from the source text (e.g., `"[nk-cgm-01]"`
        /// or `"nk-cgm-01"`).
        raw: String,
        /// Canonical record ID from `valid_record_ids`.
        record_id: String,
    },
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Splits `text` into plain prose segments and inline citations strictly scoped
/// to `valid_record_ids`.
///
/// Recognizes both bracketed (`[nk-cgm-01]`) and bare word-boundary
/// (`nk-cgm-01`) occurrences using longest-match-first, exact case-sensitive
/// matching. Partial tokens inside larger alphanumeric, underscore, or
/// hyphenated words are never matched, and identifiers outside
/// `valid_record_ids` remain plain text.
pub fn split_text_with_record_citations(
    text: &str,
    valid_record_ids: &[&str],
) -> Vec<CitationTextSegment> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut candidates: Vec<&str> = valid_record_ids
        .iter()
        .copied()
        .filter(|id| !id.is_empty())
        .collect();
    if candidates.is_empty() {
        return vec![CitationTextSegment::Plain(text.to_string())];
    }

    candidates.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    candidates.dedup();

    let mut segments = Vec::new();
    let mut plain_start = 0usize;
    let mut i = 0usize;

    while i < text.len() {
        let rest = &text[i..];
        let mut matched: Option<(usize, &str)> = None;

        // 1. Check bracketed `[record_id]` match first.
        if let Some(after_open) = rest.strip_prefix('[') {
            for &id in &candidates {
                if let Some(after_id) = after_open.strip_prefix(id) {
                    if after_id.starts_with(']') {
                        matched = Some((i + 1 + id.len() + 1, id));
                        break;
                    }
                }
            }
        }

        // 2. Check bare word-boundary `record_id` match.
        if matched.is_none() {
            let left_boundary_ok = i == 0
                || text[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|prev| !is_token_char(prev));

            if left_boundary_ok {
                for &id in &candidates {
                    if rest.starts_with(id) {
                        let end = i + id.len();
                        let right_boundary_ok = end == text.len()
                            || text[end..]
                                .chars()
                                .next()
                                .is_none_or(|next| !is_token_char(next));
                        if right_boundary_ok {
                            matched = Some((end, id));
                            break;
                        }
                    }
                }
            }
        }

        if let Some((match_end, id)) = matched {
            if i > plain_start {
                segments.push(CitationTextSegment::Plain(text[plain_start..i].to_string()));
            }
            segments.push(CitationTextSegment::Citation {
                raw: text[i..match_end].to_string(),
                record_id: id.to_string(),
            });
            i = match_end;
            plain_start = match_end;
        } else if let Some(ch) = rest.chars().next() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    if plain_start < text.len() {
        segments.push(CitationTextSegment::Plain(text[plain_start..].to_string()));
    }

    segments
}

fn sanitize_id_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Builds a deterministic, sanitized DOM anchor ID for a patient chart record
/// (for example, `record_anchor_id("chart-record", "nk-cgm-01")` yields
/// `"chart-record-nk-cgm-01"`).
pub fn record_anchor_id(prefix: &str, record_id: &str) -> String {
    let clean_prefix = sanitize_id_segment(prefix);
    let clean_record = sanitize_id_segment(record_id);
    match (clean_prefix.is_empty(), clean_record.is_empty()) {
        (false, false) => format!("{clean_prefix}-{clean_record}"),
        (true, false) => clean_record,
        (false, true) => clean_prefix,
        (true, true) => "record".to_string(),
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn scroll_element_into_view(target_id: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(target_id))
    {
        element.scroll_into_view();
    }
}

/// Scrolls the target record anchor into view in the browser.
///
/// Defers by one animation frame when `requestAnimationFrame` is available so
/// reactive tab switches or un-hidden sections finish mounting before scrolling.
/// Safe no-op during SSR.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn scroll_to_record_anchor(dom_id: &str) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let id = dom_id.trim_start_matches('#').to_string();
    if id.is_empty() {
        return;
    }

    if let Some(window) = web_sys::window() {
        let id_for_raf = id.clone();
        let closure = Closure::once(Box::new(move || {
            scroll_element_into_view(&id_for_raf);
        }) as Box<dyn FnMut()>);

        if window
            .request_animation_frame(closure.as_ref().unchecked_ref())
            .is_ok()
        {
            closure.forget();
            return;
        }
    }

    scroll_element_into_view(&id);
}

/// Scrolls the target record anchor into view in the browser (no-op on SSR).
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub fn scroll_to_record_anchor(_dom_id: &str) {}

fn truncate_excerpt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let cutoff = trimmed
        .char_indices()
        .nth(max_chars)
        .map_or(trimmed.len(), |(idx, _)| idx);
    let slice = &trimmed[..cutoff];
    let clean = match slice.rfind(char::is_whitespace) {
        Some(ws) if ws > cutoff / 2 => slice[..ws].trim_end(),
        _ => slice.trim_end(),
    };
    format!("{clean}…")
}

#[allow(clippy::too_many_arguments)]
fn render_citation_badge(
    record: ChartRecord,
    anchor_prefix: &'static str,
    accent: CitationAccent,
    on_select: Option<Callback<String>>,
    excerpt_override: Option<String>,
    occurrence: usize,
    kind_tag: &'static str,
    scope: &str,
    context_text: &str,
) -> impl IntoView {
    let locale = use_locale();
    let open = RwSignal::new(false);
    let anchor = record_anchor_id(anchor_prefix, &record.id);
    let href = format!("#{anchor}");
    let excerpt = excerpt_override
        .as_deref()
        .map(|s| truncate_excerpt(s, 180))
        .unwrap_or_else(|| truncate_excerpt(&record.body, 180));
    let occurrence_str = occurrence.to_string();
    let tip_id = deterministic_citation_tip_id(&[
        kind_tag,
        anchor_prefix,
        scope,
        context_text,
        &record.id,
        &excerpt,
        &occurrence_str,
    ]);
    let describedby_id = tip_id.clone();

    let kind_label = record.kind.label();
    let kind_cls = record_kind_badge_class(record.kind);
    let aria_label = format!("Jump to record {}: {}", record.id, record.title);
    let meta = format!("{} · {}", record.recorded_at, record.source_label);
    let record_id_link = record.id.clone();
    let record_id_tip = record.id.clone();
    let record_id_attr = record.id.clone();
    let record_id_click = record.id;
    let anchor_click = anchor;
    let title = record.title;
    let key_values: Vec<(String, String)> = record.key_values.into_iter().take(3).collect();

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            open.set(false);
        }
    };

    let on_click = move |_ev: leptos::ev::MouseEvent| {
        open.set(false);
        if let Some(cb) = on_select {
            cb.run(record_id_click.clone());
        }
        scroll_to_record_anchor(&anchor_click);
    };

    view! {
        <span
            class="relative inline-flex items-baseline align-baseline"
            on:mouseenter=move |_| open.set(true)
            on:mouseleave=move |_| open.set(false)
        >
            <a
                href=href
                aria-label=aria_label
                aria-describedby=describedby_id
                data-citation-record-id=record_id_attr
                on:focus=move |_| open.set(true)
                on:blur=move |_| open.set(false)
                on:keydown=on_keydown
                on:click=on_click
                class=accent.badge_link_class()
            >
                <span>{record_id_link}</span>
                <span aria-hidden="true" class="text-[10px] opacity-80">
                    "↗"
                </span>
            </a>
            <span
                id=tip_id
                role="tooltip"
                class=move || {
                    if open.get() {
                        accent.badge_tooltip_open_class()
                    } else {
                        "hidden"
                    }
                }
            >
                <span class="flex flex-wrap items-center justify-between gap-1.5">
                    <span class=accent.tooltip_id_badge_class()>{record_id_tip}</span>
                    <span class=format!(
                        "rounded-full border px-2 py-0.5 text-[10px] font-semibold {kind_cls}"
                    )>
                        {kind_label}
                    </span>
                </span>
                <strong class="mt-1.5 block text-xs font-semibold text-white">{title}</strong>
                <span class="mt-0.5 block font-mono text-[10px] text-slate-400">{meta}</span>
                {(!key_values.is_empty()).then(|| view! {
                    <span class="mt-2 flex flex-wrap gap-1.5">
                        {key_values
                            .into_iter()
                            .map(|(k, v)| {
                                view! {
                                    <span class="inline-flex items-baseline gap-1 rounded-md border border-white/10 bg-slate-900/90 px-1.5 py-0.5 text-[10px]">
                                        <span class="text-slate-400">{format!("{k}:")}</span>
                                        <strong class="font-semibold text-slate-100">{v}</strong>
                                    </span>
                                }
                            })
                            .collect_view()}
                    </span>
                })}
                <span class="mt-2 block text-[11px] leading-relaxed text-slate-200">{excerpt}</span>
                {move || {
                    let loc = locale.get();
                    (loc != Locale::En).then(|| {
                        view! {
                            <span class="mt-1.5 block text-[10px] italic text-slate-400">
                                {t(loc, Msg::CitationSourceLanguageNote)}
                            </span>
                        }
                    })
                }}
                <span class=accent.tooltip_cta_class()>
                    {move || t(locale.get(), Msg::CitationClickToJump)}
                </span>
            </span>
        </span>
    }
}

/// Inline accessible citation pill link (`<a href="#...">`) with a rich
/// hover/focus tooltip preview of the referenced [`ChartRecord`].
///
/// Opens on mouse hover or keyboard focus, dismisses on mouse leave, blur,
/// `Escape`, or click activation, and invokes `on_select` +
/// [`scroll_to_record_anchor`] on click.
#[component]
pub fn CitationBadgeLink(
    record: ChartRecord,
    anchor_prefix: &'static str,
    #[prop(default = CitationAccent::Cyan)] accent: CitationAccent,
    #[prop(optional)] on_select: Option<Callback<String>>,
    #[prop(optional)] excerpt_override: Option<String>,
    #[prop(optional)] occurrence: usize,
    #[prop(optional)] scope: &'static str,
) -> impl IntoView {
    render_citation_badge(
        record,
        anchor_prefix,
        accent,
        on_select,
        excerpt_override,
        occurrence,
        "badge",
        scope,
        "",
    )
}

/// Renders prose preserving line breaks (`whitespace-pre-line`) while replacing
/// every matched record ID from `records` with an inline [`CitationBadgeLink`].
///
/// Only record IDs present in `records` are linked; foreign or unknown IDs stay
/// plain text.
#[component]
pub fn CitationLinkedText(
    text: String,
    records: Vec<ChartRecord>,
    anchor_prefix: &'static str,
    #[prop(default = CitationAccent::Cyan)] accent: CitationAccent,
    #[prop(optional)] on_select: Option<Callback<String>>,
    #[prop(optional)] class: &'static str,
    #[prop(optional)] scope: &'static str,
) -> impl IntoView {
    let valid_ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    let segments = split_text_with_record_citations(&text, &valid_ids);
    let record_map: HashMap<String, ChartRecord> =
        records.into_iter().map(|r| (r.id.clone(), r)).collect();

    let has_display_override = class.split_whitespace().any(|tok| {
        matches!(
            tok,
            "inline" | "inline-block" | "inline-flex" | "flex" | "grid"
        )
    });
    let wrapper_class = match (class.is_empty(), has_display_override) {
        (true, _) => "block whitespace-pre-line".to_string(),
        (false, true) => format!("whitespace-pre-line {class}"),
        (false, false) => format!("block whitespace-pre-line {class}"),
    };

    let mut seen_counts: HashMap<String, usize> = HashMap::new();

    view! {
        <span class=wrapper_class>
            {segments
                .into_iter()
                .map(|seg| match seg {
                    CitationTextSegment::Plain(plain) => view! { <span>{plain}</span> }.into_any(),
                    CitationTextSegment::Citation { raw, record_id } => {
                        let occurrence = {
                            let entry = seen_counts.entry(record_id.clone()).or_insert(0);
                            let current = *entry;
                            *entry += 1;
                            current
                        };
                        if let Some(record) = record_map.get(&record_id).cloned() {
                            render_citation_badge(
                                record,
                                anchor_prefix,
                                accent,
                                on_select,
                                None,
                                occurrence,
                                "linked",
                                scope,
                                &text,
                            )
                            .into_any()
                        } else {
                            view! { <span>{raw}</span> }.into_any()
                        }
                    }
                })
                .collect_view()}
        </span>
    }
}

/// Clickable cited-record card linking to `#{anchor}` with a hover/focus
/// tooltip preview of `source_label` and `key_values` when the backing
/// [`ChartRecord`] is supplied.
#[component]
pub fn CitationCardLink(
    citation: ChatCitation,
    record: Option<ChartRecord>,
    anchor_prefix: &'static str,
    #[prop(default = CitationAccent::Cyan)] accent: CitationAccent,
    #[prop(optional)] on_select: Option<Callback<String>>,
    #[prop(optional)] scope: &'static str,
) -> impl IntoView {
    let locale = use_locale();
    let open = RwSignal::new(false);
    let anchor = record_anchor_id(anchor_prefix, &citation.record_id);
    let href = format!("#{anchor}");
    let tip_id = deterministic_citation_tip_id(&[
        "card",
        anchor_prefix,
        scope,
        &citation.record_id,
        &citation.recorded_at,
        &citation.excerpt,
    ]);
    let describedby_id = tip_id.clone();

    let kind_label = citation.kind.label();
    let kind_cls = record_kind_badge_class(citation.kind);
    let aria_label = format!(
        "Jump to cited record {}: {}",
        citation.record_id, citation.title
    );
    let card_id_attr = citation.record_id.clone();
    let record_id_card = citation.record_id.clone();
    let record_id_tip = citation.record_id.clone();
    let record_id_click = citation.record_id;
    let anchor_click = anchor;
    let title_card = citation.title.clone();
    let title_tip = citation.title;
    let recorded_at_card = citation.recorded_at.clone();
    let recorded_at_tip = citation.recorded_at;
    let excerpt = citation.excerpt;

    let source_label_card = record.as_ref().map(|r| r.source_label.clone());
    let source_label_tip = source_label_card.clone();
    let key_values: Vec<(String, String)> = record
        .map(|r| r.key_values.into_iter().take(3).collect())
        .unwrap_or_default();

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            open.set(false);
        }
    };

    let on_click = move |_ev: leptos::ev::MouseEvent| {
        open.set(false);
        if let Some(cb) = on_select {
            cb.run(record_id_click.clone());
        }
        scroll_to_record_anchor(&anchor_click);
    };

    view! {
        <div
            class="relative"
            on:mouseenter=move |_| open.set(true)
            on:mouseleave=move |_| open.set(false)
        >
            <a
                href=href
                aria-label=aria_label
                aria-describedby=describedby_id
                data-citation-card-id=card_id_attr
                on:focus=move |_| open.set(true)
                on:blur=move |_| open.set(false)
                on:keydown=on_keydown
                on:click=on_click
                class=accent.card_link_class()
            >
                <div class="flex flex-wrap items-center justify-between gap-1.5">
                    <span class=accent.card_id_badge_class()>{record_id_card}</span>
                    <span class=format!(
                        "rounded-full border px-2 py-0.5 text-[10px] font-semibold {kind_cls}"
                    )>
                        {kind_label}
                    </span>
                </div>
                <p class="mt-1.5 text-xs font-semibold text-white">
                    {title_card}
                    <span class="ms-1.5 font-mono font-normal text-slate-400">
                        {recorded_at_card}
                    </span>
                </p>
                <p class="mt-1 text-xs leading-relaxed text-slate-300">{excerpt}</p>
                {move || {
                    let loc = locale.get();
                    (loc != Locale::En).then(|| {
                        view! {
                            <p class="mt-1.5 text-[10px] italic text-slate-400">
                                {t(loc, Msg::CitationSourceLanguageNote)}
                            </p>
                        }
                    })
                }}
                <div class="mt-2 flex flex-wrap items-center justify-between gap-2 border-t border-white/10 pt-1.5">
                    <span class="font-mono text-[10px] text-slate-400">
                        {source_label_card.unwrap_or_else(|| kind_label.to_string())}
                    </span>
                    <span class=accent.card_cta_class()>
                        {move || t(locale.get(), Msg::CitationJumpToSource)}
                    </span>
                </div>
            </a>

            <div
                id=tip_id
                role="tooltip"
                class=move || {
                    if open.get() {
                        accent.card_tooltip_open_class()
                    } else {
                        "hidden"
                    }
                }
            >
                <div class="flex flex-wrap items-center justify-between gap-1.5">
                    <span class=accent.tooltip_id_badge_class()>{record_id_tip}</span>
                    <span class="font-mono text-[10px] text-slate-400">{recorded_at_tip}</span>
                </div>
                <p class="mt-1 text-xs font-semibold text-white">{title_tip}</p>
                {source_label_tip.map(|src| {
                    view! {
                        <p class="mt-0.5 font-mono text-[10px] text-slate-300">
                            {format!("Source: {src}")}
                        </p>
                    }
                })}
                {(!key_values.is_empty()).then(|| view! {
                    <div class="mt-2 flex flex-wrap gap-1.5">
                        {key_values
                            .into_iter()
                            .map(|(k, v)| {
                                view! {
                                    <span class="inline-flex items-baseline gap-1 rounded-md border border-white/10 bg-slate-900/90 px-1.5 py-0.5 text-[10px]">
                                        <span class="text-slate-400">{format!("{k}:")}</span>
                                        <strong class="font-semibold text-slate-100">{v}</strong>
                                    </span>
                                }
                            })
                            .collect_view()}
                    </div>
                })}
                {move || {
                    let loc = locale.get();
                    (loc != Locale::En).then(|| {
                        view! {
                            <p class="mt-1.5 text-[10px] italic text-slate-400">
                                {t(loc, Msg::CitationSourceLanguageNote)}
                            </p>
                        }
                    })
                }}
                <p class=accent.tooltip_cta_class()>
                    {move || t(locale.get(), Msg::CitationClickToJump)}
                </p>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clinical_workspace::synthetic_panel;
    use leptos::prelude::RenderHtml;

    #[test]
    fn parses_bracketed_and_bare_citations_with_punctuation() {
        let text = "- [nk-cgm-01] 14-day trend; see nk-lab-02, (nk-msg-03).";
        let valid = ["nk-cgm-01", "nk-lab-02", "nk-msg-03"];
        let segments = split_text_with_record_citations(text, &valid);

        assert_eq!(
            segments,
            vec![
                CitationTextSegment::Plain("- ".into()),
                CitationTextSegment::Citation {
                    raw: "[nk-cgm-01]".into(),
                    record_id: "nk-cgm-01".into(),
                },
                CitationTextSegment::Plain(" 14-day trend; see ".into()),
                CitationTextSegment::Citation {
                    raw: "nk-lab-02".into(),
                    record_id: "nk-lab-02".into(),
                },
                CitationTextSegment::Plain(", (".into()),
                CitationTextSegment::Citation {
                    raw: "nk-msg-03".into(),
                    record_id: "nk-msg-03".into(),
                },
                CitationTextSegment::Plain(").".into()),
            ]
        );

        let reconstructed: String = segments
            .iter()
            .map(|seg| match seg {
                CitationTextSegment::Plain(s) => s.as_str(),
                CitationTextSegment::Citation { raw, .. } => raw.as_str(),
            })
            .collect();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn enforces_word_boundaries_longest_match_and_utf8_safety() {
        let text = "Prefix x-nk-cgm-01, nk-cgm-01a, nk-cgm-01-extra, [nk-cgm-01-extra], \
                    foo_nk-cgm-01, NK-CGM-01 — valid nk-cgm-01-ext and nk-cgm-01 · done.";
        let valid = ["nk-cgm-01", "nk-cgm-01-ext"];
        let segments = split_text_with_record_citations(text, &valid);

        let matched_ids: Vec<&str> = segments
            .iter()
            .filter_map(|seg| match seg {
                CitationTextSegment::Citation { record_id, .. } => Some(record_id.as_str()),
                CitationTextSegment::Plain(_) => None,
            })
            .collect();
        assert_eq!(matched_ids, vec!["nk-cgm-01-ext", "nk-cgm-01"]);
    }

    #[test]
    fn prevents_cross_patient_citation_leakage() {
        let panel = synthetic_panel();
        let nadia = &panel[0];
        let valid_ids: Vec<&str> = nadia.records.iter().map(|r| r.id.as_str()).collect();

        let text = "Nadia has nk-cgm-01 and [nk-lab-02]; foreign refs [cm-lab-02], \
                    cm-note-01, pn-note-01, and [do-img-01] must stay plain.";
        let segments = split_text_with_record_citations(text, &valid_ids);

        let matched_ids: Vec<&str> = segments
            .iter()
            .filter_map(|seg| match seg {
                CitationTextSegment::Citation { record_id, .. } => Some(record_id.as_str()),
                CitationTextSegment::Plain(_) => None,
            })
            .collect();
        assert_eq!(matched_ids, vec!["nk-cgm-01", "nk-lab-02"]);
        assert!(segments.iter().any(|seg| matches!(
            seg,
            CitationTextSegment::Plain(p)
                if p.contains("[cm-lab-02]")
                    && p.contains("cm-note-01")
                    && p.contains("pn-note-01")
                    && p.contains("[do-img-01]")
        )));
    }

    #[test]
    fn record_anchor_id_sanitizes_segments() {
        assert_eq!(
            record_anchor_id("chart-record", "nk-cgm-01"),
            "chart-record-nk-cgm-01"
        );
        assert_eq!(
            record_anchor_id("patient-record", "nk-lab-02"),
            "patient-record-nk-lab-02"
        );
        assert_eq!(
            record_anchor_id("Chart Record!", "NK / lab 02 "),
            "chart-record-nk-lab-02"
        );
    }

    #[test]
    fn citation_linked_text_ssr_renders_accessible_links_and_tooltips() {
        let nadia = synthetic_panel().into_iter().next().expect("nadia chart");
        let records = nadia.records;
        let text = "Grounded in [nk-cgm-01] and nk-lab-02 (repeat nk-cgm-01); \
                    foreign cm-lab-02 is ignored."
            .to_string();

        let html = view! {
            <CitationLinkedText
                text=text.clone()
                records=records.clone()
                anchor_prefix="chart-record"
                accent=CitationAccent::Cyan
            />
        }
        .to_html();

        // Deterministic SSR output across renders.
        let html_again = view! {
            <CitationLinkedText
                text=text
                records=records
                anchor_prefix="chart-record"
                accent=CitationAccent::Cyan
            />
        }
        .to_html();
        assert_eq!(html, html_again);

        assert!(html.contains("href=\"#chart-record-nk-cgm-01\""));
        assert!(html.contains("href=\"#chart-record-nk-lab-02\""));
        assert!(!html.contains("href=\"#chart-record-cm-lab-02\""));
        assert!(html.contains("role=\"tooltip\""));
        assert!(html.contains("aria-describedby=\"cite-tip-"));
        assert!(html.contains("14-Day Continuous Glucose Summary"));
        assert!(html.contains("Home CGM Export"));
        assert!(html.contains("78%"));
        assert!(html.contains("Click to jump to source record"));
    }

    #[test]
    fn citation_card_link_ssr_renders_accessible_card_and_preview_tooltip() {
        let nadia = synthetic_panel().into_iter().next().expect("nadia chart");
        let rec = nadia.records[0].clone();
        let cite = ChatCitation {
            record_id: rec.id.clone(),
            title: rec.title.clone(),
            recorded_at: rec.recorded_at.clone(),
            kind: rec.kind,
            excerpt: rec.body.clone(),
        };

        let html = view! {
            <CitationCardLink
                citation=cite.clone()
                record=Some(rec)
                anchor_prefix="patient-record"
                accent=CitationAccent::Emerald
            />
        }
        .to_html();

        assert!(html.contains("href=\"#patient-record-nk-cgm-01\""));
        assert!(html.contains("role=\"tooltip\""));
        assert!(html.contains("aria-describedby=\"cite-tip-"));
        assert!(html.contains("14-Day Continuous Glucose Summary"));
        assert!(html.contains("Home CGM Export"));
        assert!(html.contains("Time in range:"));
        assert!(html.contains("78%"));
        assert!(html.contains("Jump to source record ↓"));
        assert!(html.contains("14-day sensor wear 96%"));

        let html_without_record = view! {
            <CitationCardLink
                citation=cite
                record=None
                anchor_prefix="patient-record"
                accent=CitationAccent::Emerald
            />
        }
        .to_html();
        assert!(html_without_record.contains("href=\"#patient-record-nk-cgm-01\""));
        assert!(html_without_record.contains("role=\"tooltip\""));
        assert!(html_without_record.contains("aria-describedby=\"cite-tip-"));
        assert!(html_without_record.contains("14-Day Continuous Glucose Summary"));
    }

    #[test]
    fn citation_badge_link_supports_excerpt_override() {
        let nadia = synthetic_panel().into_iter().next().expect("nadia chart");
        let rec = nadia.records[0].clone();

        let html = view! {
            <CitationBadgeLink
                record=rec
                anchor_prefix="chart-record"
                excerpt_override="Focused fasting glucose drift 114 to 146 mg/dL.".to_string()
            />
        }
        .to_html();

        assert!(html.contains("href=\"#chart-record-nk-cgm-01\""));
        assert!(html.contains("role=\"tooltip\""));
        assert!(html.contains("aria-describedby=\"cite-tip-"));
        assert!(html.contains("Focused fasting glucose drift 114 to 146 mg/dL."));
    }

    fn extract_attr_values<'a>(html: &'a str, prefix: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut cursor = html;
        while let Some(pos) = cursor.find(prefix) {
            let after = &cursor[pos + prefix.len()..];
            if let Some(end) = after.find('"') {
                out.push(&after[..end]);
                cursor = &after[end + 1..];
            } else {
                break;
            }
        }
        out
    }

    #[test]
    fn no_duplicate_tooltip_ids_across_linked_blocks_and_standalone_badges() {
        use std::collections::HashSet;

        let nadia = synthetic_panel().into_iter().next().expect("nadia chart");
        let records = nadia.records;
        let rec = records[0].clone();
        let cite = ChatCitation {
            record_id: rec.id.clone(),
            title: rec.title.clone(),
            recorded_at: rec.recorded_at.clone(),
            kind: rec.kind,
            excerpt: rec.body.clone(),
        };

        let html = view! {
            <div>
                <CitationLinkedText
                    text="Answer cites [nk-cgm-01], nk-lab-02, and repeats nk-cgm-01.".to_string()
                    records=records.clone()
                    anchor_prefix="chart-record"
                />
                <CitationLinkedText
                    text="Basis: grounded in nk-cgm-01 and nk-lab-02.".to_string()
                    records=records
                    anchor_prefix="chart-record"
                />
                <CitationBadgeLink
                    record=rec.clone()
                    anchor_prefix="chart-record"
                />
                <CitationBadgeLink
                    record=rec.clone()
                    anchor_prefix="chart-record"
                    scope="reply-draft"
                />
                <CitationCardLink
                    citation=cite
                    record=Some(rec)
                    anchor_prefix="chart-record"
                />
            </div>
        }
        .to_html();

        let tooltip_ids = extract_attr_values(&html, "id=\"cite-tip-");
        let describedby_ids = extract_attr_values(&html, "aria-describedby=\"cite-tip-");

        assert_eq!(tooltip_ids.len(), 8);
        assert_eq!(describedby_ids, tooltip_ids);

        let unique: HashSet<&str> = tooltip_ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            tooltip_ids.len(),
            "tooltip DOM IDs must be unique across all rendered citation components: {tooltip_ids:?}"
        );
    }

    #[test]
    fn non_english_locale_shows_source_language_note_and_localized_actions() {
        let nadia = synthetic_panel().into_iter().next().expect("nadia chart");
        let rec = nadia.records[0].clone();
        let cite = ChatCitation {
            record_id: rec.id.clone(),
            title: rec.title.clone(),
            recorded_at: rec.recorded_at.clone(),
            kind: rec.kind,
            excerpt: rec.body.clone(),
        };

        let en_owner = Owner::new();
        let en_html = en_owner.with(|| {
            provide_context(RwSignal::new(Locale::En));
            view! {
                <div>
                    <CitationBadgeLink record=rec.clone() anchor_prefix="chart-record" />
                    <CitationCardLink
                        citation=cite.clone()
                        record=Some(rec.clone())
                        anchor_prefix="patient-record"
                        accent=CitationAccent::Emerald
                    />
                </div>
            }
            .to_html()
        });
        drop(en_owner);
        assert!(!en_html.contains(t(Locale::En, Msg::CitationSourceLanguageNote)));
        assert!(en_html.contains(t(Locale::En, Msg::CitationClickToJump)));
        assert!(en_html.contains(t(Locale::En, Msg::CitationJumpToSource)));
        assert!(en_html.contains("text-start"));
        assert!(!en_html.contains("text-left"));

        let ar_owner = Owner::new();
        let ar_html = ar_owner.with(|| {
            provide_context(RwSignal::new(Locale::Ar));
            view! {
                <div>
                    <CitationBadgeLink record=rec.clone() anchor_prefix="chart-record" />
                    <CitationCardLink
                        citation=cite
                        record=Some(rec)
                        anchor_prefix="patient-record"
                        accent=CitationAccent::Emerald
                    />
                </div>
            }
            .to_html()
        });
        drop(ar_owner);
        assert!(ar_html.contains(t(Locale::Ar, Msg::CitationSourceLanguageNote)));
        assert!(ar_html.contains(t(Locale::Ar, Msg::CitationClickToJump)));
        assert!(ar_html.contains(t(Locale::Ar, Msg::CitationJumpToSource)));
    }
}
