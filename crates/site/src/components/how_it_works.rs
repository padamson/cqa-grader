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
            <p class="mb-6 text-chalk-50/80">
                "A graph's repository publishes its "
                <a href=CQA_SCHEMA class="text-chalk-blue underline hover:text-chalk-50">"cqa"</a>
                " benchmark as Turtle. For each question, the key lists the records a correct "
                "answer must reach (its anchors), the ones it must cite, and the kind of answer "
                "it is: a lookup, a comparison, a synthesis, an attribution, or \"none on "
                "record\". The system you are grading writes a trace: for each question, the "
                "records it retrieved and the ones it cited. "
                <code class="text-chalk-yellow">"cqa-grader grade"</code>
                " compares the two and exits 1 if any question fails."
            </p>
            <CodeTabs tabs=vec![
                ("benchmark", snippets::STEP_BENCHMARK_TTL),
                ("trace.json", snippets::STEP_TRACE_JSON),
                ("grade", snippets::STEP_GRADE_SH),
            ]/>
            <div id="rules" class="mt-8 grid grid-cols-1 gap-5 md:grid-cols-3">
                <div class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"Only the trace crosses over"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "A trace holds record IRIs and one flag for \"none on record\", and "
                        "nothing about how the system found them, so a private retriever can be "
                        "graded in public."
                    </p>
                </div>
                <div class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"The baseline never reads the key"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "It sees each question's text and the graph. A test blanks the whole key "
                        "and requires the baseline's attempts to come out the same."
                    </p>
                </div>
                <div class=CARD>
                    <h3 class="font-semibold text-chalk-yellow">"Same trace, same grade"</h3>
                    <p class="mt-1 text-sm text-chalk-50/80">
                        "Grading reads the key and the trace and nothing else: no model, no "
                        "network. The same pair always gets the same report."
                    </p>
                </div>
            </div>
        </section>
    }
}
