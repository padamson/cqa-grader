use leptos::prelude::*;

use super::CodeBlock;
use crate::snippets;

/// The baseline graded on wine's benchmark: the commands, the report they
/// print, and the CI step a benchmark's own repository would run.
#[component]
pub fn Example() -> impl IntoView {
    view! {
        <section id="example" class="mx-auto max-w-5xl px-6 py-12">
            <h2 class="mb-2 text-2xl font-bold text-chalk-yellow">"Grading the baseline on wine"</h2>
            <p class="mb-6 text-chalk-50/80">
                "The wine ontology is a worked example with six competency questions. The baseline "
                "finds records whose labels appear in the question, adds everything one link "
                "away, and cites all of it. That passes every question, which shows the "
                "benchmark can be answered by lookup alone. Its precision, as low as 0.02, is "
                "the bar a real retriever has to clear."
            </p>
            <CodeBlock html=snippets::EXAMPLE_RUN_SH caption="the baseline, then the grade"/>
            <div class="mt-5">
                <CodeBlock html=snippets::EXAMPLE_OUTPUT_TXT caption="the report"/>
            </div>
            <div class="mt-5">
                <CodeBlock html=snippets::CI_STEP_YAML caption="the same two commands, in a benchmark repository's CI"/>
            </div>
        </section>
    }
}
