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
                    blurb="Every record the key says an answer must reach is among what the system retrieved. Recall and precision are reported for every question; recall of one is required, precision never is."
                />
                <FeatureCard
                    id="feature-citations"
                    title="Citations grounded"
                    blurb="Nothing is cited that was not retrieved, and an answer that must rest on records cites them."
                />
                <FeatureCard
                    id="feature-absence"
                    title="Closed-world negatives"
                    blurb="Where the key says nothing on record joins two records, the system has to reach both and then say so. Reaching them and saying nothing is not an answer."
                />
                <FeatureCard
                    id="feature-kinds"
                    title="One check per answer kind"
                    blurb="Lookup, comparison, synthesis, attribution, closed-world negative: the grade dispatches on what the question asks for, and a question with two kinds must pass both."
                />
                <FeatureCard
                    id="feature-baseline"
                    title="A naive baseline"
                    blurb="Label linking and a hop of lookup, which never reads the key. It gives a benchmark's CI a student, and a real retriever a precision floor to beat."
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
