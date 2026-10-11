use leptos::prelude::*;

use crate::links::{N_AND_M, ONTOLOGY_TEMPLATE};

const CARD: &str = "rounded-xl border border-board-600 bg-board-800 p-5";
const LINK: &str = "text-chalk-blue underline hover:text-chalk-50";

/// Who the grader is for: someone designing an ontology, someone
/// publishing a graph and its benchmark, and someone building retrieval
/// over it.
#[component]
pub fn ThreeUses() -> impl IntoView {
    view! {
        <section id="uses" class="mx-auto max-w-5xl px-6 py-12">
            <h2 class="mb-6 text-2xl font-bold text-chalk-yellow">"Three uses"</h2>
            <div class="grid grid-cols-1 gap-5 md:grid-cols-3">
                <div id="use-developing" class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"Developing an ontology"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "Noy and McGuinness's "
                        <a href=N_AND_M class=LINK>"Ontology Development 101"</a>
                        " starts from competency questions. Write an example instance graph and "
                        "a benchmark for it, and the baseline shows whether every record an "
                        "answer needs can be found in the example. The "
                        <a href=ONTOLOGY_TEMPLATE class=LINK>"ontology-authoring-template"</a>
                        "'s wine ontology carries both."
                    </p>
                </div>
                <div id="use-publishing" class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"Publishing a graph"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "Run the baseline in your repository's CI to show that every record "
                        "your benchmark's answers need can be found in the graph you publish."
                    </p>
                </div>
                <div id="use-building" class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"Building a retriever"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "Have your system write a trace for a graph's benchmark and grade it. "
                        "Pass every question first, then work on precision: the share of what it "
                        "retrieved that the answer actually needed."
                    </p>
                </div>
            </div>
        </section>
    }
}
