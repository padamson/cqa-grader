use leptos::prelude::*;

use super::CodeBlock;
use crate::snippets;

#[component]
pub fn Install() -> impl IntoView {
    // The dev (main HEAD) build is unreleased, so it installs from git, not
    // the crates.io version.
    let is_dev = crate::version::is_dev();

    view! {
        <section id="install" class="mx-auto max-w-5xl px-6 py-12">
            <h2 class="mb-4 text-2xl font-bold text-chalk-yellow">"Install"</h2>
            {if is_dev {
                view! {
                    <CodeBlock
                        html=snippets::INSTALL_DEV_SH
                        caption="Unreleased: installs from GitHub main HEAD"
                    />
                }
                    .into_any()
            } else {
                view! { <CodeBlock html=snippets::INSTALL_SH/> }.into_any()
            }}
            <p class="mt-4 text-sm text-chalk-50/80">
                "One binary with three commands: "
                <code class="text-chalk-yellow">"grade"</code>
                " scores a trace, "
                <code class="text-chalk-yellow">"baseline"</code>
                " runs a deliberately naive retriever over a graph, and "
                <code class="text-chalk-yellow">"trace-schema"</code>
                " prints the JSON Schema a trace must follow. It reads Turtle and JSON and "
                "needs nothing else installed."
            </p>
        </section>
    }
}
