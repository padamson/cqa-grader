use leptos::prelude::*;

use crate::links::{CI_RUNS, ONTOLOGY_TEMPLATE};

#[component]
pub fn DogfoodBanner() -> impl IntoView {
    view! {
        <section id="dogfood-banner" class="mx-auto max-w-5xl px-6 py-12">
            <div class="rounded-2xl border-2 border-dashed border-chalk-yellow/60 p-8 text-center">
                <h2 class="text-2xl font-bold text-chalk-yellow">"Graded in its own CI"</h2>
                <p class="mx-auto mt-3 max-w-2xl text-chalk-50/80">
                    "On every push, the "
                    <code class="text-chalk-yellow">"baseline_then_grade_leaves_only_closed_world_negatives_unanswered"</code>
                    " test runs the baseline over the published graph of "
                    <a href=ONTOLOGY_TEMPLATE class="text-chalk-blue underline hover:text-chalk-50">"wine"</a>
                    ", grades it, and fails the build unless every question passes except "
                    "the closed-world negatives, which the baseline leaves unanswered."
                </p>
                <p class="mt-6 text-sm font-semibold">
                    <a href=CI_RUNS class="text-chalk-blue underline hover:text-chalk-50">
                        "See the CI runs"
                    </a>
                </p>
            </div>
        </section>
    }
}
