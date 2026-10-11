use leptos::prelude::*;

use super::{CodeBlock, FeatureCard};
use crate::snippets;

/// Six cards. The dogfood test counts them (`[id^='feature-']`), so an
/// addition here needs its count bumped there.
#[component]
pub fn Features() -> impl IntoView {
    view! {
        <section id="features" class="mx-auto max-w-5xl px-6 py-12">
            <h2 class="mb-6 text-2xl font-bold text-chalk-yellow">"What it checks"</h2>
            <div class="grid grid-cols-1 gap-5 md:grid-cols-2">
                <FeatureCard
                    id="feature-anchors"
                    title="Anchors reached"
                    blurb="Every record the key says an answer needs is among what the system retrieved. Recall and precision are reported for each question; recall must be 1, and precision is reported but never required."
                />
                <FeatureCard
                    id="feature-citations"
                    title="Citations grounded"
                    blurb="Every cited record was retrieved, and an answer the key says must rest on records cites them."
                />
                <FeatureCard
                    id="feature-absence"
                    title="\"None on record\" answers"
                    blurb="Some questions are correctly answered \"none on record\". The system has to reach the records the question names and then say so; reaching them and saying nothing fails."
                />
                <FeatureCard
                    id="feature-exit-codes"
                    title="Exit codes and reports"
                    blurb="0 when every question passes, 1 when any fails, and 2 when the input can't be graded: a trace for another benchmark, a field the format doesn't define, or two attempts at one question. --report also writes the grade as JSON."
                />
                <FeatureCard
                    id="feature-baseline"
                    title="A naive baseline"
                    blurb="Finds records by label and looks one link away, never reading the key. It gives a benchmark's CI something to grade, and a real retriever a precision to beat."
                />
                <FeatureCard
                    id="feature-schema"
                    title="A trace schema"
                    blurb="The trace format is a JSON Schema generated from the grader's own types, so a system can check its output before it is graded."
                >
                    <CodeBlock html=snippets::CARD_SCHEMA_SH/>
                </FeatureCard>
            </div>
        </section>
    }
}
