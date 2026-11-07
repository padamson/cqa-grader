use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use cqa_grader::{Baseline, Benchmark, Graph, Report, Require, Trace, grade};

/// Grade a system's retrieval trace against a cqa benchmark.
#[derive(Parser)]
#[command(name = "cqa-grader", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Score a system against a benchmark: the trace it wrote, or the
    /// naive baseline run here. Exits 1 if it does not meet what
    /// `--require` asks of it.
    #[command(group(ArgGroup::new("system").required(true).args(["trace", "baseline"])))]
    Grade {
        /// The benchmark's published Turtle artifact.
        #[arg(long)]
        benchmark: PathBuf,
        /// The trace to grade (JSON, see `trace-schema`).
        #[arg(long, conflicts_with = "Retrieval")]
        trace: Option<PathBuf>,
        /// Grade the naive baseline instead of a trace, run here over
        /// `--graph`. It reads only the benchmark's question text.
        #[arg(long, requires = "graph")]
        baseline: bool,
        #[command(flatten)]
        judge: Judge,
        #[command(flatten, next_help_heading = "With --baseline")]
        retrieval: Option<Retrieval>,
        /// Also write the baseline's trace to this path.
        #[arg(long, requires = "baseline", conflicts_with = "trace")]
        save_trace: Option<PathBuf>,
    },
    /// Run the naive baseline retriever over a graph and write its trace.
    #[command(mut_arg("graph", |a| a.required(true)))]
    Baseline {
        #[command(flatten)]
        retrieval: Retrieval,
        /// The benchmark whose questions to attempt. Only question text is read.
        #[arg(long)]
        benchmark: PathBuf,
        /// Where to write the trace. Stdout if omitted.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Print the JSON Schema for the trace format.
    TraceSchema,
}

/// What the baseline runs over. Each subcommand that takes it says
/// whether it is required; given at all, it is given whole.
#[derive(Args)]
struct Retrieval {
    /// The target graph's published Turtle artifact (schema plus
    /// instances). Repeat for a graph published as several files; they
    /// load into one store. The dataset and version the files state they
    /// hold are recorded in the trace, which grading refuses unless they
    /// are the benchmark's target.
    #[arg(long)]
    graph: Vec<PathBuf>,
    /// The dataset those files hold, as the benchmark's `target_dataset`
    /// would name it, for a graph that does not state it. One that
    /// contradicts what the graph states is refused.
    #[arg(long, requires_all = ["graph", "dataset_version"])]
    dataset: Option<String>,
    /// The version of that dataset, given with `--dataset`.
    #[arg(long, requires_all = ["graph", "dataset"])]
    dataset_version: Option<String>,
    /// Rounds of one-hop expansion from each entry point.
    #[arg(long, default_value_t = 1)]
    hops: usize,
}

/// What a trace is held to, and how the report is given.
#[derive(Args)]
struct Judge {
    /// What the trace must do to pass.
    #[arg(long, value_enum, default_value_t)]
    require: Require,
    /// How to print the report.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    /// Also write the report as JSON to this path.
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Grade {
            benchmark,
            trace,
            judge,
            retrieval,
            save_trace,
            ..
        } => {
            let b = read_benchmark(&benchmark)?;
            let r = match trace {
                Some(path) => {
                    let t = Trace::from_path(&path)
                        .with_context(|| format!("reading trace {}", path.display()))?;
                    grade(&b, &t, judge.require)?
                }
                None => {
                    let (t, named_by) = retrieval.context("--baseline needs --graph")?.trace(&b)?;
                    // Before grading, so the trace survives a grade that
                    // cannot run.
                    if let Some(path) = save_trace {
                        write_trace(&t, &path)?;
                    }
                    grade(&b, &t, judge.require).with_context(|| {
                        format!("the baseline's trace names its graph by {named_by}")
                    })?
                }
            };
            judge.report(&r)
        }
        Command::Baseline {
            retrieval,
            benchmark,
            output,
        } => {
            let b = read_benchmark(&benchmark)?;
            let (trace, _) = retrieval.trace(&b)?;
            match output {
                Some(path) => write_trace(&trace, &path)?,
                None => println!("{}", trace.to_json()?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::TraceSchema => {
            println!("{}", Trace::json_schema()?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

impl Retrieval {
    /// Run the baseline over the graph and return its trace for `b`, with
    /// what named the graph it records (see [`over`]).
    fn trace(self, b: &Benchmark) -> Result<(Trace, &'static str)> {
        let retriever = Baseline::from_paths(&self.graph).context("reading the graph")?;
        let stated = retriever
            .stated_graph()
            .context("reading which dataset the graph holds")?;
        let given = self
            .dataset
            .zip(self.dataset_version)
            .map(|(dataset, dataset_version)| Graph {
                dataset,
                dataset_version,
            });
        let (graph, named_by) = over(stated, given)?;
        Ok((retriever.run(b, graph, self.hops)?, named_by))
    }
}

/// The graph the baseline runs over, and what named it: what its files
/// state, or what the flags give where the files state nothing. The flags
/// never override the files, since that would relabel a graph past
/// grading's check that it is the benchmark's target. They agree when
/// grading would take them as the same graph, a pre-release as its
/// release.
fn over(stated: Option<Graph>, given: Option<Graph>) -> Result<(Graph, &'static str)> {
    match (stated, given) {
        (Some(stated), Some(given)) if !stated.matches(&given) => bail!(
            "the graph states it holds {stated}, and --dataset and --dataset-version say {given}"
        ),
        (Some(graph), _) => Ok((graph, "the graph's dcat:Dataset node")),
        (None, Some(graph)) => Ok((graph, "--dataset and --dataset-version")),
        (None, None) => bail!(
            "the graph does not state which dataset it holds; name it with \
             --dataset and --dataset-version"
        ),
    }
}

impl Judge {
    /// Print and write `r`, and exit 1 unless it passed.
    fn report(self, r: &Report) -> Result<ExitCode> {
        match self.format {
            Format::Text => print!("{}", r.to_text()),
            Format::Json => println!("{}", r.to_json()?),
        }
        if let Some(path) = self.report {
            r.write(&path)
                .with_context(|| format!("writing report {}", path.display()))?;
        }
        Ok(if r.passed {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        })
    }
}

fn read_benchmark(path: &Path) -> Result<Benchmark> {
    Benchmark::from_path(path).with_context(|| format!("reading benchmark {}", path.display()))
}

fn write_trace(trace: &Trace, path: &Path) -> Result<()> {
    trace
        .write(path)
        .with_context(|| format!("writing trace {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(dataset: &str, version: &str) -> Option<Graph> {
        Some(Graph {
            dataset: dataset.into(),
            dataset_version: version.into(),
        })
    }

    #[test]
    fn the_graphs_statement_names_it_whatever_agreeing_flags_say() {
        let stated = named("worked-example", "0.2.1-dev");
        for given in [
            None,
            named("worked-example", "0.2.1-dev"),
            named("worked-example", "0.2.1"),
        ] {
            let (graph, by) = over(stated.clone(), given.clone()).unwrap();
            assert_eq!(Some(graph), stated, "{given:?}");
            assert!(by.contains("dcat:Dataset"), "{given:?}: {by}");
        }
    }

    #[test]
    fn the_flags_name_a_graph_that_states_nothing() {
        let given = named("a-fork", "9.9.9");
        let (graph, by) = over(None, given.clone()).unwrap();
        assert_eq!(Some(graph), given);
        assert!(by.contains("--dataset"), "{by}");
    }

    #[test]
    fn flags_that_contradict_the_graph_are_refused() {
        let stated = named("worked-example", "0.2.1");
        for given in [named("a-fork", "0.2.1"), named("worked-example", "0.2.2")] {
            assert!(over(stated.clone(), given.clone()).is_err(), "{given:?}");
        }
    }

    #[test]
    fn a_graph_named_by_nothing_is_refused() {
        let err = over(None, None).unwrap_err().to_string();
        assert!(err.contains("--dataset-version"), "{err}");
    }
}
