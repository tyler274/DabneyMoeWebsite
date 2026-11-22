//! Public creator history at `/koizetsu`.

use leptos::prelude::*;
use leptos_meta::Title;

use crate::sections::{Footer, NavBar};

#[component]
pub fn KoizetsuPage() -> impl IntoView {
    view! {
        <Title text="Koizetsu — Public Streaming History | dabney.moe" />
        <NavBar />
        <article class="mx-auto max-w-3xl px-6 py-20 text-slate-300">
            <header class="space-y-6 border-b border-slate-800 pb-10">
                <p class="text-sm font-semibold uppercase tracking-widest text-cyan-400">"Creator history · Independent research"</p>
                <h1 class="text-4xl font-bold tracking-tight text-white sm:text-6xl">"Koizetsu"</h1>
                <p class="text-xl leading-relaxed">"A koi taking a break from becoming a dragon. Games, worldbuilding, and creative streams on Twitch."</p>
                <p class="text-sm text-slate-400">"Research snapshot: November 22, 2026. Unofficial; not affiliated with or endorsed by the creator."</p>
                <div class="flex flex-wrap gap-6">
                    <a class="text-cyan-400 underline" href="https://www.twitch.tv/koizetsu">"Twitch channel"</a>
                    <a class="text-cyan-400 underline" href="https://bsky.app/profile/koizetsu.bsky.social">"Bluesky profile"</a>
                    <a class="text-cyan-400 underline" href="#timeline">"Explore the timeline"</a>
                </div>
            </header>

            <section class="mt-10 space-y-4 leading-relaxed" aria-labelledby="overview">
                <h2 id="overview" class="text-2xl font-semibold text-white">"The short version"</h2>
                <p>"Koizetsu is an English-language VTuber whose character is a koi taking a break from the struggle to ascend into dragonhood. Public promotion predates recorded streams in late August 2023. Games and writing are recurring parts of the channel, with third-party broadcast records continuing into November 2026."</p>
                <p>"This page covers the public creator persona only. It does not identify the person behind the avatar or collect private personal information."</p>
            </section>

            <section id="timeline" class="mt-12 space-y-6" aria-labelledby="timeline-heading">
                <h2 id="timeline-heading" class="text-2xl font-semibold text-white">"Public timeline"</h2>
                <ol class="space-y-8 border-l border-slate-700 pl-6">
                    <li class="space-y-2">
                        <p class="text-sm font-medium text-cyan-400">"March 16, 2022 · Tracker record"</p>
                        <h3 class="text-lg font-semibold text-white">"The Twitch account is created"</h3>
                        <p>"SullyGnome reports this account-creation date. It is not a streaming debut date."</p>
                        <a class="text-cyan-400 underline" href="https://sullygnome.com/channel/koizetsu">"Source: channel tracker"</a>
                    </li>
                    <li class="space-y-2">
                        <p class="text-sm font-medium text-cyan-400">"July 25, 2023 · Creator announcement"</p>
                        <h3 class="text-lg font-semibold text-white">"A model reveal before streaming begins"</h3>
                        <p>"A Reddit promotion introduces the koi character, links a Twitter model reveal, and says streaming is planned for the near future."</p>
                        <a class="text-cyan-400 underline" href="https://www.reddit.com/r/VTuberPromotions/comments/159lx9r/trying_to_do_a_model_reveal_hopefully_posting/">"Source: model-reveal promotion"</a>
                    </li>
                    <li class="space-y-2">
                        <p class="text-sm font-medium text-cyan-400">"August 2023 · Self-report and tracker records"</p>
                        <h3 class="text-lg font-semibold text-white">"The streaming launch"</h3>
                        <p>"A later Bluesky introduction gives August 28 as the start date. The independent tracker lists broadcasts on August 29, 30, and 31. This corroborates a late-August launch, but not the exact first day."</p>
                        <a class="text-cyan-400 underline" href="https://sullygnome.com/channel/koizetsu/2023august">"Source: August broadcast archive"</a>
                    </li>
                    <li class="space-y-2">
                        <p class="text-sm font-medium text-cyan-400">"2023 · Third-party aggregate"</p>
                        <h3 class="text-lg font-semibold text-white">"An established first season"</h3>
                        <p>"SullyGnome reports 78 streams and approximately 290 hours streamed in 2023. These are tracker figures, not audited Twitch totals."</p>
                        <a class="text-cyan-400 underline" href="https://sullygnome.com/channel/koizetsu/2023">"Source: 2023 archive"</a>
                    </li>
                    <li class="space-y-2">
                        <p class="text-sm font-medium text-cyan-400">"April 21, 2025 · Creator introduction"</p>
                        <h3 class="text-lg font-semibold text-white">"Games meet writing and art"</h3>
                        <p>"In a public introduction, Koizetsu describes an interest in roguelikes, indie games, soulslikes, drawing, and writing. A planned lecture explores cultivation fantasy and how its themes can fit futuristic settings. This is an introduction post, not a debut announcement."</p>
                        <a class="text-cyan-400 underline" href="https://www.reddit.com/r/VirtualYoutubers/comments/1k4ii7y/behold_a_list_of_things/">"Source: creator introduction"</a>
                    </li>
                    <li class="space-y-2">
                        <p class="text-sm font-medium text-cyan-400">"October 22–November 21, 2026 · Tracker snapshot"</p>
                        <h3 class="text-lg font-semibold text-white">"Still broadcasting"</h3>
                        <p>"At the research date, SullyGnome reported 20 streams and approximately 116 hours for this window, including approximately 63 hours in Writing & Reading. Dated entries included November 19. Sparse social announcements do not imply a streaming hiatus."</p>
                        <a class="text-cyan-400 underline" href="https://sullygnome.com/channel/koizetsu">"Source: rolling channel summary (changes over time)"</a>
                    </li>
                </ol>
            </section>

            <section class="mt-12 space-y-4 leading-relaxed" aria-labelledby="content-heading">
                <h2 id="content-heading" class="text-2xl font-semibold text-white">"What the channel explores"</h2>
                <div class="rounded-xl border border-slate-800 bg-slate-900 p-6 space-y-4">
                    <p><strong class="text-white">"Games. "</strong>"Public announcements mention Darkest Dungeon, Slay the Spire, Yi Xian, Lies of P, Nioh, Hades II, and Where Winds Meet, among others. This is a selection, not a complete broadcast catalog."</p>
                    <p><strong class="text-white">"Writing. "</strong>"Worldbuilding, cultivation fantasy, drafting, and community-led ideas recur alongside gameplay. The creator’s profile also describes writing a cultivation novel."</p>
                    <p><strong class="text-white">"Creative work. "</strong>"Drawing and writing are explicitly part of the creator’s public introduction; social announcements also discuss emote-making and animation."</p>
                    <a class="text-cyan-400 underline" href="https://bsky.app/profile/koizetsu.bsky.social">"Read the creator’s public announcements"</a>
                </div>
            </section>

            <section class="mt-12 space-y-4 leading-relaxed" aria-labelledby="limits-heading">
                <h2 id="limits-heading" class="text-2xl font-semibold text-white">"What remains uncertain"</h2>
                <ul class="list-disc space-y-3 pl-6">
                    <li>"The exact first broadcast and Twitch Affiliate qualification dates are not independently established here."</li>
                    <li>"The early game history is incomplete. Missing posts or expired videos are not evidence that a stream did not happen."</li>
                    <li>"No YouTube archive was verified as belonging to this creator. Similarly named channels were excluded."</li>
                    <li>"Tracker totals can change or disagree with cached search results. Figures above describe the fetched research snapshot, not live statistics."</li>
                </ul>
                <p class="text-sm text-slate-400">"Method: creator-authored public introductions and announcements, corroborated where possible by third-party stream records. Self-reports, observed records, and unknowns are labeled separately. No private communities were accessed."</p>
            </section>
        </article>
        <Footer />
    }
}
