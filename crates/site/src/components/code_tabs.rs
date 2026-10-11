use leptos::prelude::*;

// The active-indicator animates; site-e2e captures receipts with
// animations(Disabled) so the transition does not race the screenshot.
const TAB_BASE: &str = "-mb-px border-b-2 px-3 py-1.5 text-xs font-semibold transition-colors";
const TAB_ACTIVE: &str = "border-chalk-yellow text-chalk-yellow";
const TAB_INACTIVE: &str = "border-transparent text-chalk-50/60 hover:text-chalk-50/90";

/// A tabbed code block: one header per label, showing the selected snippet.
/// An interactive component, so the dogfood test exercises real client-side
/// reactivity (click a tab, the code switches and the tab is marked selected).
#[component]
pub fn CodeTabs(
    /// `(label, highlighted_html)` pairs; the first is shown by default.
    tabs: Vec<(&'static str, &'static str)>,
) -> impl IntoView {
    let (active, set_active) = signal(0usize);
    let tab_labels = tabs.clone();

    // Rebuild the whole header row inside one reactive closure. This avoids
    // per-button reactive class/attribute closures, which fail to track when
    // the component is rendered through a parent's `children` slot.
    let headers = move || {
        let act = active.get();
        tab_labels
            .iter()
            .enumerate()
            .map(|(i, (label, _))| {
                let label = *label;
                let selected = act == i;
                let class = if selected {
                    format!("{TAB_BASE} {TAB_ACTIVE}")
                } else {
                    format!("{TAB_BASE} {TAB_INACTIVE}")
                };
                view! {
                    <button
                        type="button"
                        role="tab"
                        data-tab=label
                        aria-selected=selected.to_string()
                        class=class
                        on:click=move |_| set_active.set(i)
                    >
                        {label}
                    </button>
                }
            })
            .collect_view()
    };

    view! {
        <div class="flex flex-col">
            <div role="tablist" class="flex gap-1 border-b border-board-600">
                {headers}
            </div>
            <pre
                role="tabpanel"
                class="overflow-x-auto rounded-b-lg rounded-tr-lg border border-board-600 bg-board-800 p-4 text-sm leading-relaxed"
                inner_html=move || tabs[active.get()].1
            ></pre>
        </div>
    }
}
