//! Minimal public creator directory at `/creators`.

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};

use crate::sections::{Footer, NavBar};

#[component]
pub(crate) fn CreatorsMetadata() -> impl IntoView {
    view! {
        <Title text="Creators | dabney.moe" />
        <Meta name="description" content="mnnaz05 on Twitch." />
        <Link rel="canonical" href="https://dabney.moe/creators" />
        <Meta property="og:type" content="website" />
        <Meta property="og:title" content="Creators | dabney.moe" />
        <Meta property="og:description" content="mnnaz05 on Twitch." />
        <Meta property="og:url" content="https://dabney.moe/creators" />
        <Meta property="og:site_name" content="dabney.moe" />
        <Meta name="twitter:card" content="summary" />
        <Meta name="twitter:title" content="Creators | dabney.moe" />
        <Meta name="twitter:description" content="mnnaz05 on Twitch." />
    }
}

#[component]
pub fn CreatorsPage() -> impl IntoView {
    view! {
        <NavBar />
        <article class="mx-auto max-w-3xl px-6 py-20 text-slate-300">
            <h1 class="text-4xl font-bold tracking-tight text-white sm:text-6xl">"Creators"</h1>
            <ul class="mt-10 space-y-6">
                <li class="rounded-xl border border-slate-800 bg-slate-900 p-6">
                    <h2 class="text-2xl font-semibold text-white">"mnnaz05"</h2>
                    <a class="mt-4 inline-block text-cyan-400 underline" href="https://www.twitch.tv/mnnaz05">"Twitch"</a>
                </li>
            </ul>
        </article>
        <Footer />
    }
}
