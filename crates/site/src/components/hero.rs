use leptos::prelude::*;

use super::icons::{self, Icon};
use crate::links::{ARCHITECTURE, CI_BADGE, CQA_SCHEMA, CRATES_IO, DOCS_RS, GITHUB};

const BUTTON: &str = "inline-flex items-center gap-2 rounded-lg border border-board-600 px-5 py-2.5 font-semibold text-chalk-50 transition hover:border-chalk-yellow";

#[component]
pub fn Hero() -> impl IntoView {
    let is_dev = crate::version::is_dev();

    view! {
        <header id="hero" class="flex flex-col items-center px-6 pt-20 pb-16 text-center">
            // The mark and the wordmark (brand/wordmark.css), named once for
            // assistive technology and drawn for everyone else.
            <p
                id="hero-brand"
                role="img"
                aria-label="cqa-grader"
                class="mb-8 inline-flex items-center gap-4 text-5xl text-chalk-50 sm:text-6xl"
            >
                <svg
                    class="h-14 w-14 text-chalk-yellow sm:h-16 sm:w-16"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    aria-hidden="true"
                >
                    <path d=icons::MARK.trim()/>
                </svg>
                <span class="wordmark" aria-hidden="true">
                    <span class="wordmark-cqa">"CQA"</span>
                    <span class="wordmark-grader">"grader"</span>
                </span>
            </p>
            <h1 id="hero-title" class="text-4xl font-bold tracking-tight text-chalk-50 sm:text-5xl">
                "Grade what a retriever found"
            </h1>
            <p id="hero-tagline" class="mt-5 max-w-2xl text-lg text-chalk-50/80">
                "cqa-grader grades what a system retrieved and cited against the competency "
                "questions a knowledge graph says it can answer: the records each answer had to "
                "reach, the ones it had to rest on, and the absences it had to report."
            </p>
            <p id="hero-rules" class="mt-3 text-sm text-chalk-50/60">
                "Deterministic. No model in the loop. Runs in CI."
            </p>

            <div id="hero-badges" class="mt-7 flex flex-wrap items-center justify-center gap-2">
                // Unreleased, the crate has no crates.io page to link to.
                {if is_dev {
                    view! {
                        <img
                            alt="crates.io: unreleased"
                            src="https://img.shields.io/badge/crates.io-unreleased-inactive"
                        />
                    }
                        .into_any()
                } else {
                    view! {
                        <a href=CRATES_IO>
                            <img alt="crates.io" src="https://img.shields.io/crates/v/cqa-grader.svg"/>
                        </a>
                    }
                        .into_any()
                }}
                <a href=GITHUB>
                    <img alt="CI" src=CI_BADGE/>
                </a>
            </div>

            <div class="mt-9 flex flex-wrap items-center justify-center gap-3">
                // The architecture pages asbuilt generates from this repo, under
                // architecture/ in every snapshot.
                <a
                    id="cta-architecture"
                    href=ARCHITECTURE
                    class="inline-flex items-center gap-2 rounded-lg bg-chalk-yellow px-5 py-2.5 font-semibold text-board-900 transition hover:bg-chalk-50"
                >
                    <Icon path=icons::ASBUILT/>
                    "See cqa-grader's architecture"
                </a>
                <a id="cta-github" href=GITHUB class=BUTTON>
                    <Icon path=icons::GITHUB/>
                    "GitHub"
                </a>
                <a id="cta-schema" href=CQA_SCHEMA class=BUTTON>
                    "The cqa schema"
                </a>
                {(!is_dev).then(|| view! {
                    <a id="cta-docs" href=DOCS_RS class=BUTTON>
                        <Icon path=icons::DOCS_RS/>
                        "Docs"
                    </a>
                })}
            </div>
        </header>
    }
}
