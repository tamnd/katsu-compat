//! Runs the Node.js test suite against a JavaScript runtime and reports what fails.
//!
//! The argument this tool exists to make is in `README.md`: there is no single hard problem
//! in Node compatibility, there are eleven hundred small ones, and the only defence that
//! has ever worked is running other people's real test suites instead of writing your own.
//!
//! It takes a runtime binary as an argument rather than assuming katsu, so it can be run
//! against Node itself as a control. A harness that has never been checked against a
//! runtime known to pass is a harness whose zeroes mean nothing.

mod outcome;
mod report;
mod runner;
mod suite;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rayon::prelude::*;

use outcome::{Expectations, TestResult};

/// Node.js compatibility testing for JavaScript runtimes.
#[derive(Debug, Parser)]
#[command(name = "katsu-compat", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Clone the Node.js test suite at a given tag.
    Vendor {
        /// The tag to check out, for example `v24.18.0`.
        #[arg(long, default_value = "v24.18.0")]
        tag: String,
        /// Where to put it.
        #[arg(long, default_value = "vendor/node")]
        into: PathBuf,
    },
    /// Run the suite against a runtime and write the results.
    Run {
        /// The runtime binary to test. Use `node` to produce a control run.
        #[arg(long, default_value = "node")]
        runtime: PathBuf,
        /// A checkout of nodejs/node.
        #[arg(long, default_value = "vendor/node")]
        suite: PathBuf,
        /// Only run tests whose path contains this string.
        #[arg(long)]
        filter: Option<String>,
        /// Seconds a single test gets before it is killed.
        #[arg(long, default_value_t = runner::DEFAULT_TIMEOUT.as_secs())]
        timeout: u64,
        /// How many tests to run at once. Defaults to the number of cores.
        #[arg(long)]
        jobs: Option<usize>,
        /// Where to write the machine readable results.
        #[arg(long, default_value = "results/latest.json")]
        out: PathBuf,
        /// Where to write the published table.
        #[arg(long)]
        markdown: Option<PathBuf>,
        /// Compare against this expectations file and fail on any difference.
        #[arg(long)]
        against: Option<PathBuf>,
        /// Overwrite the expectations file instead of failing on a difference.
        #[arg(long)]
        bless: bool,
    },
    /// Render a table from a previous run.
    Report {
        /// The results file to read.
        #[arg(long, default_value = "results/latest.json")]
        input: PathBuf,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("katsu-compat: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    match Cli::parse().command {
        Task::Vendor { tag, into } => {
            vendor(&tag, &into)?;
            Ok(true)
        }
        Task::Run {
            runtime,
            suite,
            filter,
            timeout,
            jobs,
            out,
            markdown,
            against,
            bless,
        } => run_suite(RunOptions {
            runtime,
            suite,
            filter,
            timeout: Duration::from_secs(timeout),
            jobs,
            out,
            markdown,
            against,
            bless,
        }),
        Task::Report { input } => {
            let results: Vec<TestResult> = serde_json::from_slice(
                &std::fs::read(&input).with_context(|| format!("reading {}", input.display()))?,
            )?;
            print!("{}", report::markdown("unknown", "unknown", &results));
            Ok(true)
        }
    }
}

fn vendor(tag: &str, into: &std::path::Path) -> Result<()> {
    if into.exists() {
        bail!(
            "{} already exists. Remove it first if you want a different tag.",
            into.display()
        );
    }
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent)?;
    }
    eprintln!("cloning nodejs/node at {tag} into {}", into.display());
    let status = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            tag,
            "https://github.com/nodejs/node",
        ])
        .arg(into)
        .status()
        .context("running git clone")?;
    if !status.success() {
        bail!("git clone failed");
    }
    Ok(())
}

struct RunOptions {
    runtime: PathBuf,
    suite: PathBuf,
    filter: Option<String>,
    timeout: Duration,
    jobs: Option<usize>,
    out: PathBuf,
    markdown: Option<PathBuf>,
    against: Option<PathBuf>,
    bless: bool,
}

fn run_suite(options: RunOptions) -> Result<bool> {
    if !options.suite.is_dir() {
        bail!(
            "no suite at {}. Run `katsu-compat vendor` first.",
            options.suite.display()
        );
    }

    let runtime_version = describe_runtime(&options.runtime)?;
    let suite_version = suite::suite_version(&options.suite)?;
    let tests = suite::discover(&options.suite, options.filter.as_deref())?;

    if tests.is_empty() {
        bail!("no tests matched");
    }

    eprintln!(
        "{} tests, runtime {runtime_version}, suite {suite_version}",
        tests.len()
    );

    if let Some(jobs) = options.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .context("setting the thread pool size")?;
    }

    let done = AtomicUsize::new(0);
    let total = tests.len();
    let mut results: Vec<TestResult> = tests
        .par_iter()
        .map(|test| {
            let result = runner::run(&options.runtime, &options.suite, test, options.timeout);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            let mut stderr = std::io::stderr().lock();
            let _ = write!(stderr, "{}", result.outcome.glyph());
            if n.is_multiple_of(80) {
                let _ = writeln!(stderr, " {n}/{total}");
            }
            let _ = stderr.flush();
            result
        })
        .collect();
    eprintln!();

    results.sort_by(|a, b| a.test.cmp(&b.test));

    if let Some(parent) = options.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&options.out, serde_json::to_vec_pretty(&results)?)?;

    let table = report::markdown(&runtime_version, &suite_version, &results);
    if let Some(path) = &options.markdown {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &table)?;
    }

    let actual = Expectations::from_results(runtime_version, suite_version, &results);
    eprintln!(
        "{} of {} attempted tests pass",
        actual.passing(),
        actual.attempted()
    );

    let Some(expectations_path) = options.against else {
        return Ok(true);
    };

    if options.bless {
        if let Some(parent) = expectations_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&expectations_path, serde_json::to_vec_pretty(&actual)?)?;
        eprintln!("blessed {}", expectations_path.display());
        return Ok(true);
    }

    let Ok(bytes) = std::fs::read(&expectations_path) else {
        bail!(
            "no expectations file at {}. Create one with --bless once you are happy with the run.",
            expectations_path.display()
        );
    };
    let expected: Expectations = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing {}", expectations_path.display()))?;

    let difference = outcome::diff(&expected, &actual);
    if difference.is_empty() {
        eprintln!("no change against {}", expectations_path.display());
        return Ok(true);
    }

    for (test, now) in &difference.regressed {
        eprintln!("regressed: {test} is now {now:?}");
    }
    for test in &difference.fixed {
        eprintln!("fixed: {test} now passes and the expectations file has not been updated");
    }
    for (test, now) in &difference.added {
        eprintln!("new: {test} is {now:?}");
    }
    for test in &difference.removed {
        eprintln!("gone: {test} did not run");
    }
    eprintln!();
    eprintln!(
        "The expectations file is out of date. If these changes are intended, rerun with --bless and commit the result."
    );
    Ok(false)
}

/// Ask a runtime what version it is, so the result can be labelled.
fn describe_runtime(runtime: &std::path::Path) -> Result<String> {
    let output = Command::new(runtime)
        .arg("--version")
        .output()
        .with_context(|| format!("running {} --version", runtime.display()))?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let name = runtime
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("runtime");
    if version.is_empty() {
        Ok(name.to_owned())
    } else {
        Ok(format!("{name} {version}"))
    }
}
