//! Commission showcase pages: lapidary, artwork, and songs.

use leptos::prelude::*;
use leptos_router::components::A;

use crate::commissions_data::{
    CommissionMedia, GalleryItem, ARTWORK_INTRO, ARTWORK_ITEMS, EILI_YOUTUBE, LAPIDARY_INTRO,
    LAPIDARY_ITEMS, SONGS, SONGS_INTRO,
};
use crate::sections::{Footer, NavBar};

#[component]
pub fn CommissionsMenu() -> impl IntoView {
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
                "Commissions"
                <span class="text-[10px] opacity-70">{move || if open.get() { "▲" } else { "▼" }}</span>
            </button>
            <Show when=move || open.get()>
                <div
                    class="absolute right-0 z-50 mt-2 min-w-44 rounded-xl border border-white/10 bg-slate-900 py-2 shadow-xl"
                    role="menu"
                >
                    <A href="/commissions/lapidary" on:click=close>
                        <span class="block px-4 py-2 text-slate-200 transition hover:bg-white/5 hover:text-white" role="menuitem">
                            "Lapidary"
                        </span>
                    </A>
                    <A href="/commissions/artwork" on:click=close>
                        <span class="block px-4 py-2 text-slate-200 transition hover:bg-white/5 hover:text-white" role="menuitem">
                            "Artwork"
                        </span>
                    </A>
                    <A href="/commissions/songs" on:click=close>
                        <span class="block px-4 py-2 text-slate-200 transition hover:bg-white/5 hover:text-white" role="menuitem">
                            "Songs"
                        </span>
                    </A>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn CommissionsPageShell(
    eyebrow: &'static str,
    title: &'static str,
    intro: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <NavBar />
        <section class="border-b border-white/5 px-6 py-20">
            <div class="mx-auto max-w-4xl">
                <p class="mb-2 text-sm font-semibold uppercase tracking-widest text-cyan-400">
                    {eyebrow}
                </p>
                <h1 class="mb-3 text-3xl font-bold tracking-tight text-white sm:text-4xl">
                    {title}
                </h1>
                <p class="mb-10 max-w-2xl text-lg leading-relaxed text-slate-300">
                    {intro}
                </p>
                {children()}
            </div>
        </section>
        <Footer />
    }
}

#[component]
fn YoutubeEmbed(video_id: &'static str, title: &'static str) -> impl IntoView {
    let src = format!("https://www.youtube.com/embed/{video_id}");
    view! {
        <div class="aspect-video overflow-hidden rounded-2xl border border-white/10 bg-black shadow-lg">
            <iframe
                src=src
                title=title
                class="h-full w-full"
                allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
                referrerpolicy="strict-origin-when-cross-origin"
                allowfullscreen
            />
        </div>
    }
}

#[component]
fn LapidaryMediaCard(item: CommissionMedia) -> impl IntoView {
    match item {
        CommissionMedia::Image { src, alt, caption } => view! {
            <figure class="overflow-hidden rounded-2xl border border-white/10 bg-white/[0.02]">
                <img src=src alt=alt class="aspect-[4/3] w-full object-cover" loading="lazy" />
                {caption.map(|text| view! {
                    <figcaption class="border-t border-white/5 px-4 py-3 text-sm text-slate-400">
                        {text}
                    </figcaption>
                })}
            </figure>
        }
        .into_any(),
        CommissionMedia::Youtube { video_id, title } => view! {
            <figure class="overflow-hidden rounded-2xl border border-white/10 bg-white/[0.02]">
                <YoutubeEmbed video_id=video_id title=title />
                <figcaption class="border-t border-white/5 px-4 py-3 text-sm text-slate-400">
                    {title}
                </figcaption>
            </figure>
        }
        .into_any(),
    }
}

#[component]
pub fn LapidaryPage() -> impl IntoView {
    view! {
        <CommissionsPageShell
            eyebrow="Commissions"
            title="Lapidary"
            intro=LAPIDARY_INTRO
        >
            <div class="grid gap-6 sm:grid-cols-2">
                {LAPIDARY_ITEMS
                    .iter()
                    .map(|item| view! { <LapidaryMediaCard item=*item /> })
                    .collect_view()}
            </div>
        </CommissionsPageShell>
    }
}

fn use_carousel_auto_advance(current: RwSignal<usize>, len: usize) {
    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    let _ = (current, len);

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        const CAROUSEL_INTERVAL_MS: i32 = 5000;

        if len <= 1 {
            return;
        }
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::JsCast;

        Effect::new(move |_| {
            let closure = Closure::wrap(Box::new(move || {
                current.update(|index| *index = (*index + 1) % len);
            }) as Box<dyn FnMut()>);

            let interval_id = web_sys::window()
                .and_then(|window| {
                    window
                        .set_interval_with_callback_and_timeout_and_arguments_0(
                            closure.as_ref().unchecked_ref(),
                            CAROUSEL_INTERVAL_MS,
                        )
                        .ok()
                })
                .unwrap_or(0);
            closure.forget();

            on_cleanup(move || {
                if interval_id != 0 {
                    if let Some(window) = web_sys::window() {
                        window.clear_interval_with_handle(interval_id);
                    }
                }
            });
        });
    }
}

#[component]
fn RotatingGallery(items: &'static [GalleryItem]) -> impl IntoView {
    let current = RwSignal::new(0usize);
    let len = items.len();
    use_carousel_auto_advance(current, len);

    let go_prev = move |_| {
        current.update(|index| {
            *index = if *index == 0 { len - 1 } else { *index - 1 };
        });
    };
    let go_next = move |_| {
        current.update(|index| *index = (*index + 1) % len);
    };

    view! {
        <div class="space-y-4">
            <div class="relative aspect-[4/3] overflow-hidden rounded-2xl border border-white/10 bg-slate-900 shadow-lg">
                {items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let src = item.src;
                        let alt = item.alt;
                        view! {
                            <img
                                src=src
                                alt=alt
                                class=move || {
                                    if current.get() == index {
                                        "absolute inset-0 h-full w-full object-contain opacity-100 transition-opacity duration-700"
                                    } else {
                                        "absolute inset-0 h-full w-full object-contain opacity-0 transition-opacity duration-700"
                                    }
                                }
                                loading="lazy"
                            />
                        }
                    })
                    .collect_view()}

                <button
                    type="button"
                    class="absolute left-3 top-1/2 -translate-y-1/2 rounded-full border border-white/10 bg-slate-950/70 px-3 py-2 text-white backdrop-blur transition hover:bg-slate-900"
                    aria-label="Previous artwork"
                    on:click=go_prev
                >
                    "‹"
                </button>
                <button
                    type="button"
                    class="absolute right-3 top-1/2 -translate-y-1/2 rounded-full border border-white/10 bg-slate-950/70 px-3 py-2 text-white backdrop-blur transition hover:bg-slate-900"
                    aria-label="Next artwork"
                    on:click=go_next
                >
                    "›"
                </button>
            </div>

            <div class="flex items-center justify-between gap-4">
                <div class="min-h-[1.5rem] text-sm text-slate-400">
                    {move || {
                        let item = items.get(current.get())?;
                        Some(view! {
                            <p class="font-medium text-slate-300">{item.title}</p>
                            <p>
                                "Art by "
                                <a
                                    href=item.artist_url
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    class="text-cyan-400 transition hover:text-cyan-300"
                                >
                                    {item.artist}
                                </a>
                            </p>
                        })
                    }}
                </div>
                <div class="flex items-center gap-2">
                    {(0..len)
                        .map(|index| {
                            let select = move |_| current.set(index);
                            view! {
                                <button
                                    type="button"
                                    class=move || {
                                        if current.get() == index {
                                            "h-2 w-2 rounded-full bg-cyan-400 transition"
                                        } else {
                                            "h-2 w-2 rounded-full bg-white/20 transition hover:bg-white/40"
                                        }
                                    }
                                    aria-label=format!("Show artwork {}", index + 1)
                                    on:click=select
                                />
                            }
                        })
                        .collect_view()}
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn ArtworkPage() -> impl IntoView {
    view! {
        <CommissionsPageShell
            eyebrow="Commissions"
            title="Artwork"
            intro=ARTWORK_INTRO
        >
            <RotatingGallery items=ARTWORK_ITEMS />
        </CommissionsPageShell>
    }
}

#[component]
pub fn SongsPage() -> impl IntoView {
    view! {
        <CommissionsPageShell
            eyebrow="Commissions"
            title="Songs"
            intro=SONGS_INTRO
        >
            <p class="-mt-4 mb-10 text-sm text-slate-400">
                "Vocals by "
                <a
                    href=EILI_YOUTUBE
                    target="_blank"
                    rel="noopener noreferrer"
                    class="text-cyan-400 transition hover:text-cyan-300"
                >
                    "eili"
                </a>
                " ("
                <a
                    href=EILI_YOUTUBE
                    target="_blank"
                    rel="noopener noreferrer"
                    class="text-cyan-400 transition hover:text-cyan-300"
                >
                    "@EiliYT"
                </a>
                " on YouTube)"
            </p>
            <div class="space-y-10">
                {SONGS
                    .iter()
                    .map(|song| {
                        let title = song.title;
                        let description = song.description;
                        view! {
                            <article class="space-y-3">
                                <div>
                                    <h2 class="text-xl font-semibold text-white">{title}</h2>
                                    {description.map(|text| view! {
                                        <p class="mt-1 text-sm text-slate-400">{text}</p>
                                    })}
                                </div>
                                <YoutubeEmbed video_id=song.video_id title=title />
                            </article>
                        }
                    })
                    .collect_view()}
            </div>
        </CommissionsPageShell>
    }
}
