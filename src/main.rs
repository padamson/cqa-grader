use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
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
    /// load into one store.
    #[arg(long, requires_all = ["dataset", "dataset_version"])]
    graph: Vec<PathBuf>,
    /// The dataset those files hold, as the benchmark's `target_dataset`
    /// would name it. Recorded in the trace, which grading refuses unless
    /// it is the benchmark's target.
    #[arg(long, required = false, requires = "graph")]
    dataset: String,
    /// The version of that dataset.
    #[arg(long, required = false, requires = "graph")]
    dataset_version: String,
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
                    let t = retrieval.context("--baseline needs --graph")?.trace(&b)?;
                    // Before grading, so the trace survives a grade that
                    // cannot run.
                    if let Some(path) = save_trace {
                        write_trace(&t, &path)?;
                    }
                    grade(&b, &t, judge.require).context(
                        "the baseline's trace names its graph by --dataset and --dataset-version",
                    )?
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
            let trace = retrieval.trace(&b)?;
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
    /// Run the baseline over the graph and return its trace for `b`.
    fn trace(self, b: &Benchmark) -> Result<Trace> {
        let retriever = Baseline::from_paths(&self.graph).context("reading the graph")?;
        let over = Graph {
            dataset: self.dataset,
            dataset_version: self.dataset_version,
        };
        Ok(retriever.run(b, over, self.hops)?)
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
