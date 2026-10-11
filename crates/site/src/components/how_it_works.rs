use leptos::prelude::*;

use super::CodeTabs;
use crate::links::CQA_SCHEMA;
use crate::snippets;

const CARD: &str = "rounded-xl border border-board-600 bg-board-800 p-5";

/// Benchmark, trace, grade: the loop, and the three rules behind it.
#[component]
pub fn HowItWorks() -> impl IntoView {
    view! {
        <section id="how-it-works" class="mx-auto max-w-5xl px-6 py-12">
            <h2 class="mb-2 text-2xl font-bold text-chalk-yellow">"How it works"</h2>
            <p class="mb-6 max-w-3xl text-chalk-50/80">
                "A graph's own repository publishes its competency questions as a "
                <a href=CQA_SCHEMA class="text-chalk-blue underline hover:text-chalk-50">"cqa"</a>
                " benchmark: for each question, the records a correct answer must reach, the ones "
                "it must rest on, and the kinds of answering it calls for. A system under test "
                "writes a trace of what it retrieved and cited for each question. "
                <code class="text-chalk-yellow">"cqa-grader grade"</code>
                " scores one against the other and exits 1 when any question fails."
            </p>
            <CodeTabs tabs=vec![
                ("benchmark", snippets::STEP_BENCHMARK_TTL),
                ("trace.json", snippets::STEP_TRACE_JSON),
                ("grade", snippets::STEP_GRADE_SH),
            ]/>
            <div id="rules" class="mt-8 grid grid-cols-1 gap-5 md:grid-cols-3">
                <div class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"The trace is the contract"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "IRIs retrieved, IRIs cited, and whether the system says none is on record. "
                        "Nothing about how it found them crosses into the grader, so a private "
                        "retriever can be graded in public."
                    </p>
                </div>
                <div class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"One check per answer kind"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "Every question gets the base checks, then one for each kind it names: "
                        "lookup, comparison, synthesis, attribution, closed-world negative. All "
                        "must pass."
                    </p>
                </div>
                <div class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"Unusable input is not a grade"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "A trace for another benchmark, a field the format doesn't define, or two "
                        "attempts at one question exits 2, not 1."
                    </p>
                </div>
            </div>
        </section>
    }
}
