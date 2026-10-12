use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use cqa_grader::{Baseline, Benchmark, Graph, Trace, grade};

/// Grade a system's retrieval trace against a cqa benchmark.
#[derive(Parser)]
#[command(name = "cqa-grader", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Score a trace against a benchmark. Exits 1 if any question fails.
    Grade {
        /// The benchmark's published Turtle artifact.
        #[arg(long)]
        benchmark: PathBuf,
        /// The trace to grade (JSON, see `trace-schema`).
        #[arg(long)]
        trace: PathBuf,
        /// How to print the report.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Also write the report as JSON to this path.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Run the naive baseline retriever over a graph and write its trace.
    Baseline {
        /// The target graph's published Turtle artifact (schema plus
        /// instances). Repeat for a graph published as several files; they
        /// load into one store.
        #[arg(long, required = true)]
        graph: Vec<PathBuf>,
        /// The dataset those files hold, as the benchmark's
        /// `target_dataset` would name it. Recorded in the trace, which
        /// grading refuses unless it is the benchmark's target.
        #[arg(long)]
        dataset: String,
        /// The version of that dataset.
        #[arg(long)]
        dataset_version: String,
        /// The benchmark whose questions to attempt. Only question text is read.
        #[arg(long)]
        benchmark: PathBuf,
        /// Rounds of one-hop expansion from each entry point.
        #[arg(long, default_value_t = 1)]
        hops: usize,
        /// Where to write the trace. Stdout if omitted.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Print the JSON Schema for the trace format.
    TraceSchema,
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
            format,
            report,
        } => {
            let b = Benchmark::from_path(&benchmark)
                .with_context(|| format!("reading benchmark {}", benchmark.display()))?;
            let t = Trace::from_path(&trace)
                .with_context(|| format!("reading trace {}", trace.display()))?;
            let r = grade(&b, &t)?;
            match format {
                Format::Text => print!("{}", r.to_text()),
                Format::Json => println!("{}", r.to_json()?),
            }
            if let Some(path) = report {
                r.write(&path)
                    .with_context(|| format!("writing report {}", path.display()))?;
            }
            Ok(if r.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Baseline {
            graph,
            dataset,
            dataset_version,
            benchmark,
            hops,
            output,
        } => {
            let b = Benchmark::from_path(&benchmark)
                .with_context(|| format!("reading benchmark {}", benchmark.display()))?;
            let retriever = Baseline::from_paths(&graph).context("reading the graph")?;
            let over = Graph {
                dataset,
                dataset_version,
            };
            let trace = retriever.run(&b, over, hops)?;
            match output {
                Some(path) => trace
                    .write(&path)
                    .with_context(|| format!("writing trace {}", path.display()))?,
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
