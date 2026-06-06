//! Shared Leptos UI for dabney.moe.
//!
//! The same components render in three configurations selected by feature:
//! - `ssr`     : server-side rendering on the Axum web server.
//! - `hydrate` : browser hydration of the server-rendered HTML.
//! - `csr`     : pure client-side rendering for the Tauri (Trunk) build.

pub mod data;
pub mod sections;

use leptos::prelude::*;
use leptos_meta::{provide_meta_context, Link, Meta, MetaTags, Stylesheet, Title};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::StaticSegment;

use sections::{Contact, Experience, Footer, Hero, NavBar, Services, Skills};

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
        <Title text="Tyler Port — Freelance Software Engineer" />
        <Meta
            name="description"
            content="Tyler Port is a Caltech-trained freelance software engineer specializing in Rust, GPU computing, and high-performance systems. Available for contract work."
        />
        <Meta name="author" content="Tyler Alamo Port" />
        <Link rel="canonical" href="https://dabney.moe/" />

        // Open Graph / social previews.
        <Meta property="og:type" content="website" />
        <Meta property="og:title" content="Tyler Port — Freelance Software Engineer" />
        <Meta
            property="og:description"
            content="Rust, GPU computing, and high-performance systems. Available for freelance and contract work."
        />
        <Meta property="og:url" content="https://dabney.moe/" />
        <Meta property="og:site_name" content="dabney.moe" />
        <Meta name="twitter:card" content="summary_large_image" />
        <Meta name="twitter:title" content="Tyler Port — Freelance Software Engineer" />
        <Meta
            name="twitter:description"
            content="Rust, GPU computing, and high-performance systems. Available for freelance and contract work."
        />

        <Router>
            <main>
                <Routes fallback=|| view! { <HomePage /> }>
                    <Route path=StaticSegment("") view=HomePage />
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! {
        <NavBar />
        <Hero />
        <Services />
        <Experience />
        <Skills />
        <Contact />
        <Footer />
    }
}
