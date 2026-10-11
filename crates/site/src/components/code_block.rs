use leptos::prelude::*;

/// Renders a pre-highlighted code snippet (build-time syntect output) with an
/// optional caption above it.
#[component]
pub fn CodeBlock(
    /// Highlighted inner HTML (token spans), from `crate::snippets`.
    html: &'static str,
    #[prop(optional, into)] caption: Option<&'static str>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col">
            {caption.map(|c| view! {
                <span class="mb-2 text-xs font-semibold uppercase tracking-wider text-chalk-yellow">
                    {c}
                </span>
            })}
            <pre
                class="overflow-x-auto rounded-lg border border-board-600 bg-board-800 p-4 text-sm leading-relaxed"
                inner_html=html
            ></pre>
        </div>
    }
}
