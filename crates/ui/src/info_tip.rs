//! Lightweight, accessible `<InfoTip>` popover component for explaining clinical
//! acronyms, thresholds, and chart terms on hover, keyboard focus, and mobile tap.

use leptos::prelude::*;

/// Deterministic FNV-1a ID so SSR and client hydration always emit the exact same `id`.
fn deterministic_tip_id(term: &str, text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in term
        .bytes()
        .chain(std::iter::once(b':'))
        .chain(text.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("infotip-{hash:016x}")
}

/// Accessible inline clinical tooltip trigger (`?`).
///
/// Opens on mouse hover, keyboard focus, or tap/click; dismisses on mouse leave
/// (when not pinned by click/tap), blur, second tap, or `Escape`.
#[component]
pub fn InfoTip(
    /// Short term name used in `aria-label` (e.g., `"GMI"`, `"TIR"`, `"PHQ-9"`).
    term: &'static str,
    /// Plain-language explanation & threshold guidance shown inside the tooltip.
    text: &'static str,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let pinned = RwSignal::new(false);
    let tip_id = deterministic_tip_id(term, text);
    let describedby_id = tip_id.clone();

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            open.set(false);
            pinned.set(false);
        }
    };

    view! {
        <span class="relative inline-flex items-center align-middle">
            <button
                type="button"
                aria-label=format!("Explain {term}")
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-describedby=move || open.get().then(|| describedby_id.clone())
                on:mouseenter=move |_| open.set(true)
                on:mouseleave=move |_| {
                    if !pinned.get_untracked() {
                        open.set(false);
                    }
                }
                on:focus=move |_| open.set(true)
                on:blur=move |_| {
                    open.set(false);
                    pinned.set(false);
                }
                on:click=move |_| {
                    let next = !pinned.get_untracked();
                    pinned.set(next);
                    open.set(next);
                }
                on:keydown=on_keydown
                class="ml-1 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full border border-white/20 bg-slate-900/90 text-[10px] font-bold leading-none text-slate-300 transition hover:border-cyan-400/60 hover:text-cyan-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400"
            >
                "?"
            </button>
            <span
                id=tip_id
                role="tooltip"
                data-infotip-term=term
                class=move || {
                    if open.get() {
                        "pointer-events-auto absolute bottom-full left-1/2 z-40 mb-2 w-64 max-w-[calc(100vw-2rem)] -translate-x-1/2 rounded-xl border border-cyan-400/35 bg-slate-950/95 p-3 text-left text-[11px] font-normal normal-case tracking-normal leading-relaxed text-slate-100 shadow-xl backdrop-blur"
                    } else {
                        "hidden"
                    }
                }
            >
                <strong class="mb-0.5 block font-semibold text-cyan-200">{term}</strong>
                <span>{text}</span>
            </span>
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    #[test]
    fn infotip_renders_accessible_trigger_and_tooltip_role() {
        let html = view! {
            <InfoTip
                term="GMI"
                text="Glucose Management Indicator estimates lab A1C from average sensor glucose."
            />
        }
        .to_html();
        assert!(html.contains("aria-label=\"Explain GMI\""));
        assert!(html.contains("role=\"tooltip\""));
        assert!(html.contains("data-infotip-term=\"GMI\""));
        assert!(html.contains("Glucose Management Indicator"));
    }
}
