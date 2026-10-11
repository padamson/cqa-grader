use leptos::prelude::*;

/// One feature highlight: title, one-line blurb, and an optional code area
/// supplied as children (a `CodeBlock` for a single snippet, or `CodeTabs`
/// for variants).
#[component]
pub fn FeatureCard(
    /// Stable id so the dogfood test can assert the card rendered.
    id: &'static str,
    title: &'static str,
    blurb: &'static str,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div id=id class="flex flex-col rounded-xl border border-board-600 bg-board-800 p-5">
            <h3 class="text-lg font-semibold text-chalk-yellow">{title}</h3>
            <p class="mt-1 text-sm text-chalk-50/80">{blurb}</p>
            {children.map(|c| view! { <div class="mt-4">{c()}</div> })}
        </div>
    }
}
