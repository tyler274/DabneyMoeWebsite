//! Shared Leptos UI for dabney.moe.
#![recursion_limit = "1024"]
//!
//! The same components render in three configurations selected by feature:
//! - `ssr`     : server-side rendering on the Axum web server.
//! - `hydrate` : browser hydration of the server-rendered HTML.
//! - `csr`     : pure client-side rendering for the Tauri (Trunk) build.

pub mod commissions;
pub mod commissions_data;
pub mod data;
pub mod portfolio;
pub mod portfolio_chart;
pub mod sections;
pub mod tools;

use leptos::prelude::*;
use leptos_meta::{provide_meta_context, Link, Meta, MetaTags, Stylesheet, Title};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::StaticSegment;

use commissions::{ArtworkPage, LapidaryPage, SongsPage};
use tools::InvestmentAccountPage;

use sections::{About, Contact, Experience, Expertise, Footer, Hero, NavBar, Services, Skills};

/// Server-rendered HTML document shell. Used only by the SSR build; the Tauri
/// CSR build supplies its own `index.html`.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en" class="scroll-smooth">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body class="bg-slate-950 font-sans text-slate-200 antialiased">
                <App />
            </body>
        </html>
    }
}

/// Root application component shared by every target.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/dabney.css" />
        <Title text="Tyler Port - Freelance Software Engineer" />
        <Meta
            name="description"
            content="Tyler Port is a Caltech-trained freelance software engineer specializing in Rust, GPU computing, and high-performance systems. Available for contract work."
        />
        <Meta name="author" content="Tyler Alamo Port" />
        <Link rel="icon" href="/favicon.png" type_="image/png" />
        <Link rel="canonical" href="https://dabney.moe/" />

        // Open Graph / social previews.
        <Meta property="og:type" content="website" />
        <Meta property="og:title" content="Tyler Port - Freelance Software Engineer" />
        <Meta
            property="og:description"
            content="Rust, GPU computing, and high-performance systems. Available for freelance and contract work."
        />
        <Meta property="og:url" content="https://dabney.moe/" />
        <Meta property="og:site_name" content="dabney.moe" />
        <Meta property="og:image" content="https://dabney.moe/photos/tyler-dunes.jpg" />
        <Meta name="twitter:card" content="summary_large_image" />
        <Meta name="twitter:image" content="https://dabney.moe/photos/tyler-dunes.jpg" />
        <Meta name="twitter:title" content="Tyler Port - Freelance Software Engineer" />
        <Meta
            name="twitter:description"
            content="Rust, GPU computing, and high-performance systems. Available for freelance and contract work."
        />

        <Router>
            <main>
                <Routes fallback=|| view! { <HomePage /> }>
                    <Route path=StaticSegment("") view=HomePage />
                    <Route
                        path=(StaticSegment("tools"), StaticSegment("investment-account"))
                        view=InvestmentAccountPage
                    />
                    <Route
                        path=(StaticSegment("tools"), StaticSegment("investment-account-sell"))
                        view=InvestmentAccountPage
                    />
                    <Route
                        path=(StaticSegment("commissions"), StaticSegment("lapidary"))
                        view=LapidaryPage
                    />
                    <Route
                        path=(StaticSegment("commissions"), StaticSegment("artwork"))
                        view=ArtworkPage
                    />
                    <Route
                        path=(StaticSegment("commissions"), StaticSegment("songs"))
                        view=SongsPage
                    />
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! {
        <HomeHashScroll />
        <NavBar />
        <Hero />
        <About />
        <Expertise />
        <Services />
        <Experience />
        <Skills />
        <Contact />
        <Footer />
    }
}

/// When navigating from another route to `/#section`, the browser tries to
/// scroll before home sections exist. Re-scroll after the home page mounts.
#[component]
#[allow(clippy::unused_unit)]
fn HomeHashScroll() -> impl IntoView {
    let location = use_location();

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    Effect::new(move |_| {
        let hash = location.hash.get();
        if hash.len() > 1 {
            schedule_scroll_to_hash(&hash);
        }
    });

    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    let _ = location;

    view! {}
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn schedule_scroll_to_hash(hash: &str) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let id = hash.trim_start_matches('#').to_string();

    let scroll = move || {
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(&id))
            .map(|element| element.scroll_into_view())
            .is_some()
    };

    if scroll() {
        return;
    }

    let closure = Closure::once(Box::new(move || {
        scroll();
    }) as Box<dyn FnMut()>);

    if let Some(window) = web_sys::window() {
        let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
    }
    closure.forget();
}
