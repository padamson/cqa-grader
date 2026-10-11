use leptos::prelude::*;
use leptos::task::spawn_local;

use super::icons;
use crate::links::{CHANGELOG, ISSUES, SOURCE};
use crate::roadmap::{self, Criterion, Horizon, Item, Roadmap as Data, Status};
use crate::version::{fetch_json, is_dev};

/// Later's line under its heading, where Now and Next show their theme.
const LATER: &str = "Under consideration, not yet planned";

/// Now, Next and Later, from `public/roadmap.json`. Now is the next release
/// and carries each item's status and a progress bar, Next the release
/// after it, and Later what is under consideration beyond that. What
/// already shipped is in the changelog.
///
/// The dev build's own copy is main's, so it shows it at once. A release
/// build waits for main's, so it never shows its own roadmap as if it were
/// current, and falls back to its own only when the fetch fails.
#[component]
pub fn Roadmap() -> impl IntoView {
    let dev = is_dev();
    let data = RwSignal::new(dev.then(Data::built));
    let fetched = RwSignal::new(false);
    if !dev {
        spawn_local(async move {
            match fetch_json::<Data>(&roadmap::current_url()).await {
                Some(current) => {
                    data.set(Some(current));
                    fetched.set(true);
                }
                None => data.set(Some(Data::built())),
            }
        });
    }
    // Only a release build fetches, so `fetched` means a release build is
    // showing main's roadmap rather than its own.
    let note = move || {
        fetched.get().then(|| {
            view! {
                <span id="roadmap-current">"This is the current roadmap, from main. "</span>
            }
        })
    };

    view! {
        <section id="roadmap" class="mx-auto max-w-5xl px-6 py-12">
            <h2 class="text-2xl font-bold text-chalk-yellow">"Roadmap"</h2>
            {move || {
                data.with(|data| {
                    data.as_ref()
                        .map(|data| {
                            view! {
                                <p class="mt-2 mb-6 max-w-3xl text-sm text-chalk-50/80">
                                    {note}
                                    {format!(
                                        "Now is {}, Next is {}, and Later is under consideration beyond that. ",
                                        data.milestone,
                                        data.next_milestone,
                                    )}
                                    "Each item's criteria say what done means and name the test "
                                    "that proves it. What already shipped is in the "
                                    <a href=CHANGELOG class="text-chalk-blue underline hover:text-chalk-50">
                                        "changelog"
                                    </a>
                                    "."
                                </p>
                                <div class="grid grid-cols-1 gap-5 md:grid-cols-3">
                                    {Horizon::ALL
                                        .into_iter()
                                        .map(|horizon| column(data, horizon))
                                        .collect_view()}
                                </div>
                            }
                        })
                })
            }}
        </section>
    }
}

/// One column's view, given only what it shows.
fn column(data: &Data, horizon: Horizon) -> impl IntoView + use<> {
    let heading = match horizon {
        Horizon::Now => format!("Now · {}", data.milestone),
        Horizon::Next => format!("Next · {}", data.next_milestone),
        other => other.label().to_string(),
    };
    let note = match horizon {
        Horizon::Later => Some(LATER.to_string()),
        _ => data.theme(horizon).map(String::from),
    };
    let progress = (horizon == Horizon::Now).then(|| {
        let (done, total) = data.progress();
        (done, total, data.milestone.clone())
    });
    let items: Vec<Item> = data.column(horizon).cloned().collect();
    view! { <Column horizon heading note progress items/> }
}

#[component]
fn Column(
    horizon: Horizon,
    heading: String,
    /// What the column's release is about (Later: that nothing is planned).
    note: Option<String>,
    progress: Option<(usize, usize, String)>,
    items: Vec<Item>,
) -> impl IntoView {
    let note = note.map(|note| {
        view! { <p data-roadmap-theme class="mt-1 text-xs text-chalk-50/70">{note}</p> }
    });
    let progress = progress.map(|(done, total, milestone)| {
        let percent = (done * 100).checked_div(total).unwrap_or(0);
        view! {
            <div class="mt-2">
                <div
                    id="roadmap-progress"
                    role="progressbar"
                    aria-label=format!("{milestone} progress")
                    aria-valuemin="0"
                    aria-valuemax=total
                    aria-valuenow=done
                    class="h-1.5 overflow-hidden rounded-full bg-board-900"
                >
                    <div class="h-full bg-chalk-yellow" style=format!("width: {percent}%")></div>
                </div>
                <p class="mt-1 text-xs text-chalk-50/70">{format!("{done} of {total} done")}</p>
            </div>
        }
    });

    view! {
        <div id=horizon.id() class="flex flex-col rounded-xl border border-board-600 bg-board-800 p-5">
            <h3 class="text-lg font-semibold text-chalk-yellow">{heading}</h3>
            {note}
            {progress}
            <ul class="mt-4 flex flex-col gap-4">
                {items.into_iter().map(|item| view! { <RoadmapItem item/> }).collect_view()}
            </ul>
        </div>
    }
}

#[component]
fn RoadmapItem(item: Item) -> impl IntoView {
    let status = (item.horizon == Horizon::Now)
        .then(|| item.status.label())
        .flatten()
        .map(|label| {
            let tone = match item.status {
                Status::Done => "border-chalk-blue/60 bg-chalk-blue/15 text-chalk-blue",
                Status::InProgress => "border-chalk-yellow/60 bg-chalk-yellow/10 text-chalk-yellow",
                Status::Planned | Status::Unknown => "border-chalk-50/25 text-chalk-50/70",
            };
            view! {
                <span
                    data-status=label
                    class=format!(
                        "shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider {tone}",
                    )
                >
                    {label}
                </span>
            }
        });
    let issue = item.issue.map(|number| {
        view! {
            <a href=format!("{ISSUES}/{number}") class="text-xs text-chalk-blue underline hover:text-chalk-50">
                {format!("#{number}")}
            </a>
        }
    });
    let criteria = (!item.acceptance.is_empty()).then(|| {
        view! {
            <Criteria
                id=format!("{}-criteria", item.id)
                title=item.title.clone()
                done=item.status == Status::Done
                criteria=item.acceptance.clone()
            />
        }
    });

    view! {
        <li id=item.id>
            <div class="flex items-start justify-between gap-2">
                <h4 class="font-semibold text-chalk-50">{item.title}</h4>
                {status}
            </div>
            <p class="mt-1 text-sm text-chalk-50/80">{item.blurb} " " {issue}</p>
            {criteria}
        </li>
    }
}

/// The item's acceptance criteria, behind a button, as a native popover:
/// the browser handles the top layer, Escape and a click outside. Each
/// criterion names its proving test; on a done item the test is in the
/// tree (a host test holds the roadmap to that), so it links to the source.
#[component]
fn Criteria(id: String, title: String, done: bool, criteria: Vec<Criterion>) -> impl IntoView {
    let count = criteria.len();
    let label = if count == 1 {
        "1 criterion".to_string()
    } else {
        format!("{count} criteria")
    };
    let mark = if done { icons::TICK } else { icons::BOX };
    let dialog_label = format!("Criteria for {title}");
    let (open_target, close_target) = (id.clone(), id.clone());
    view! {
        <button
            type="button"
            popovertarget=open_target
            data-criteria-button
            class="mt-2 inline-flex items-center gap-1.5 rounded-md border border-board-600 px-2.5 py-1.5 text-xs font-semibold text-chalk-blue transition hover:border-chalk-blue"
        >
            <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                <path d=mark/>
            </svg>
            {label}
        </button>
        <div
            id=id
            popover="auto"
            role="dialog"
            aria-label=dialog_label
            class="m-auto w-[min(32rem,calc(100vw-2rem))] rounded-xl border-0 bg-paper p-6 text-left text-ink shadow-2xl"
        >
            <div class="flex items-start gap-3">
                <div class="flex-1">
                    <p class="text-xs font-bold uppercase tracking-wider text-red-pen">"Done when"</p>
                    <h5 class="mt-1 text-lg font-bold">{title}</h5>
                </div>
                <button
                    type="button"
                    popovertarget=close_target
                    popovertargetaction="hide"
                    aria-label="Close criteria"
                    class="-mt-1 -mr-2 rounded p-2 text-ink/70 hover:text-ink"
                >
                    <svg class="h-5 w-5" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                        <path d="M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/>
                    </svg>
                </button>
            </div>
            <ol class="mt-4 flex flex-col gap-3">
                {criteria
                    .into_iter()
                    .map(|c| {
                        let test = if done {
                            let href = c
                                .test_location()
                                .map(|(path, _)| format!("{SOURCE}/{path}"))
                                .unwrap_or_default();
                            view! {
                                <a href=href class="text-ink/70 underline hover:text-red-pen">{c.test.clone()}</a>
                            }
                                .into_any()
                        } else {
                            view! { <span class="text-ink/70">{c.test.clone()}</span> }.into_any()
                        };
                        view! {
                            <li data-criterion class="flex gap-3">
                                <svg class="mt-0.5 h-4 w-4 shrink-0 text-red-pen" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                                    <path d=mark/>
                                </svg>
                                <div class="min-w-0">
                                    <p class="text-sm">{c.criterion}</p>
                                    <code class="mt-0.5 block text-xs break-all">{test}</code>
                                </div>
                            </li>
                        }
                    })
                    .collect_view()}
            </ol>
        </div>
    }
}
