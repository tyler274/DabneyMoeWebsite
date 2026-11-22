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
                        "The operator reports that all server messages are logged and community "
                        "messages have been retained outside Discord. Logging is separate from "
                        "public archive publication and the planned activity dashboard; logging alone "
                        "is not permission for public posting or AI uses. Storage location, who has "
                        "access, the retention period, "
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
                    <h2 class="text-xl font-semibold text-white">"Community activity dashboard"</h2>
                    <p>
                        "The planned community activity dashboard will publish limited Discord "
                        "activity metadata for participants who authorize the applicable uses. Its "
                        "public snapshots will not include message bodies, attachments, searchable "
                        "conversation archives, tokens, private consent records, or backup material. "
                        "This is separate from server message logging and the separately tracked "
                        "searchable archive. This policy amendment alone does not authorize posting "
                        "chats or establish that an activity listener is running."
                    </p>
                    <p>"Permissions remain separate for:"</p>
                    <ul class="list-disc space-y-2 pl-6">
                        <li>"Contributing to aggregate activity statistics."</li>
                        <li>"Publishing identified message counts."</li>
                        <li>"Collecting Discord-reported presence."</li>
                        <li>"Publishing identified presence history."</li>
                    </ul>
                    <p>
                        "Agreement to one scope does not authorize another. Joining the server, "
                        "receiving a notice, remaining a member, or remaining silent does not grant "
                        "permission. Participation in the community does not require opting in."
                    </p>
                    <p>
                        "Before collecting or publishing activity under these scopes, we will explain "
                        "the included information, identifying details, public recipients, and "
                        "withdrawal controls. Public-sharing permission must explicitly cover the "
                        "persistent historical downloads and other published-history limits below. "
                        "Permission given under an earlier, narrower explanation does not by itself "
                        "authorize this expanded scope."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Publication delay and dashboard visibility"</h2>
                    <p>
                        "We will not publish an individual's presence observation until at least one "
                        "hour after that observation. This delay applies to public snapshot uploads "
                        "as well as display on the dashboard."
                    </p>
                    <p>
                        "The dashboard will show a rolling seven-day window of permitted presence "
                        "and message-count activity. Seven days describes what appears on the current "
                        "dashboard; it is not a promise to delete previously published data after "
                        "seven days."
                    </p>
                    <p>
                        "Presence describes what Discord reported. Disconnected, stale, or missing "
                        "observations appear as unknown, not as inferred inactivity. We will not "
                        "reconstruct historical presence from message timestamps. We will suppress "
                        "aggregates that could expose nonparticipants or unsafe small groups."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Public snapshots and published history"</h2>
                    <p>
                        "Filtered activity snapshots are intended for publication using Git Large "
                        "File Storage (Git LFS) in the public "
                        <a href="https://github.com/tyler274/DabneyMoeWebsite">"DabneyMoeWebsite repository"</a>
                        ". Publication makes this information available beyond the dashboard itself."
                    </p>
                    <p>
                        "Older snapshots, repository history, and LFS objects may remain accessible "
                        "after information leaves the seven-day dashboard window. People may download "
                        "or copy them. Removing a current file, repository reference, or dashboard "
                        "entry does not reliably erase the underlying published objects or previously "
                        "obtained copies."
                    </p>
                    <p>
                        "Dashboard access checks, cache controls, and requests not to index the page "
                        "do not control historical downloads or prevent copying. There is no automatic "
                        "seven-day purge of published history. We also do not guarantee that published "
                        "history will remain available forever."
                    </p>
                </section>

                <section class="mt-10 space-y-4 text-slate-300">
                    <h2 class="text-xl font-semibold text-white">"Activity withdrawal and removal"</h2>
                    <p>
                        "You may withdraw any activity scope independently through its announced "
                        "controls or by contacting server moderators privately. You do not need to "
                        "repost sensitive information publicly."
                    </p>
                    <p>
                        "Withdrawal stops the affected collection and excludes the affected data "
                        "from future published snapshots. We will remove affected individual "
                        "information from the current dashboard and invalidate its current serving "
                        "caches. Restoring or replaying old data must not reinstate withdrawn "
                        "permission or restore affected information to the current dashboard."
                    </p>
                    <p>
                        "Withdrawal does not reliably erase snapshots already published, Git or "
                        "LFS history, historical downloads, or third-party copies. Those copies "
                        "may remain accessible after withdrawal. Moderators will explain what a "
                        "request covers, what action can be taken, and these limits."
                    </p>
                    <p>
                        "This dashboard permission does not authorize AI training, AI retrieval, "
                        "impersonation, public conversations, or other uses covered separately by "
                        "this policy. Material expansions require renewed affirmative permission "
                        "before they take effect."
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
