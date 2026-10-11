use leptos::prelude::*;

use crate::links::{CLI_TEST, WINE};

#[component]
pub fn DogfoodBanner() -> impl IntoView {
    view! {
        <section id="dogfood-banner" class="mx-auto max-w-5xl px-6 py-12">
            <div class="rounded-2xl border-2 border-dashed border-chalk-yellow/60 p-8 text-center">
                <h2 class="text-2xl font-bold text-chalk-yellow">"Graded in its own CI"</h2>
                <p class="mx-auto mt-3 max-w-2xl text-chalk-50/80">
                    "On every push, the baseline runs over the published graph of "
                    <a href=WINE class="text-chalk-blue underline hover:text-chalk-50">"wine"</a>
                    ", a worked-example ontology, and the build fails unless it passes every "
                    "question in wine's benchmark."
                </p>
                <p class="mt-6 text-sm font-semibold">
                    <a href=CLI_TEST class="text-chalk-blue underline hover:text-chalk-50">
                        "See the test that runs it"
                    </a>
                </p>
            </div>
        </section>
    }
}
