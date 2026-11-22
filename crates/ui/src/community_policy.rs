//! Community data handling notice and prospective sharing policy (`/community-policy`).

use leptos::prelude::*;

use crate::sections::{Footer, NavBar};

#[component]
pub fn CommunityPolicyPage() -> impl IntoView {
    view! {
        <NavBar />
        <article class="px-6 py-20">
            <div class="prose prose-invert mx-auto max-w-3xl prose-headings:text-white prose-a:text-cyan-400">
                <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                    "Community Data and Sharing Policy"
                </h1>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Current retention and disclosure limits"</h2>
                    <p>
                        "The operator reports that community messages have already been retained "
                        "outside Discord. Storage location, who has access, the retention period, "
                        "and any previous AI use are not yet documented in this notice. "
                        "This is not a complete data-handling disclosure."
                    </p>
                    <p>
                        "The operator's declared purposes are AI model training or fine-tuning, "
                        "AI retrieval or reference, human training, and memorial preservation. "
                        "These are intended purposes, not a claim that training or any other "
                        "listed use has already happened, nor evidence of permission for those uses."
                    </p>
                    <p>
                        "This page sets commitments for future use and sharing of Discord community "
                        "content. It does not publish conversations or establish that a public archive "
                        "or consent controls are implemented."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Separate permissions"</h2>
                    <p>
                        "Joining or remaining in the server is not consent to public sharing. "
                        "Permission to back up messages is not permission to publish them, use them "
                        "for AI, or impersonate a participant with AI."
                    </p>
                    <p>
                        "AI model training or fine-tuning, AI retrieval or reference, human training, "
                        "and a public memorial each require separate, affirmative permission from "
                        "the affected participants before that use. Agreement to one does not "
                        "authorize the others. We will explain the intended use, recipients, and "
                        "any external AI service before seeking permission."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Public sharing is opt-in"</h2>
                    <p>"Before any public sharing, we will explain and seek affirmative opt-in covering:"</p>
                    <ul class="list-disc space-y-2 pl-6">
                        <li>"Which messages, channels, attachments, and timestamps would be included."</li>
                        <li>"Whether names, handles, or other identifying information would appear."</li>
                        <li>"How public search and search-engine indexing would work."</li>
                        <li>"How to decline, withdraw permission, or request removal."</li>
                    </ul>
                    <p>
                        "Private community participation does not require opting in to public sharing. "
                        "Consent to an AI or training use is not consent to public publication."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Earlier conversations"</h2>
                    <p>
                        "This policy does not retroactively authorize use or publication of earlier "
                        "messages. Earlier messages require separate permission covering the specific "
                        "messages and intended use from the affected participants. A new notice, "
                        "continued membership, or silence is not retroactive consent."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Sensitive information"</h2>
                    <p>
                        "We will not publish other people's private health, legal, or contact "
                        "information, or credentials. An author's permission does not authorize "
                        "disclosing another person's private information."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Removal and withdrawal"</h2>
                    <p>
                        "Contact server moderators to withdraw permission for future use or sharing, "
                        "or to request removal. Identify the messages and uses your request covers; "
                        "you do not need to repost sensitive information publicly. Moderators should "
                        "confirm what the request covers, what action can be taken, and any limits."
                    </p>
                    <p>
                        "Removal from a public page, a search index, retained backups, or AI and "
                        "training materials are different requests. Deletion capabilities and "
                        "timelines for these locations are not yet documented here. This notice "
                        "does not promise deletion from every copy, reversal of completed training, "
                        "or model unlearning. We cannot guarantee removal of copies downloaded "
                        "or cached by third parties or search engines."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Changes"</h2>
                    <p>
                        "We will announce material changes before they take effect. Expanded sharing "
                        "or new uses require renewed permission. Silence is not agreement."
                    </p>
                </section>
            </div>
        </article>
        <Footer />
    }
}
