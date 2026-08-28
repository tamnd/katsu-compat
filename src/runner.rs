//! Running one test against one runtime binary, with a time limit.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::outcome::{Outcome, TestResult};
use crate::suite::Test;

/// How long a single test gets before it is killed.
///
/// Node's own runner uses sixty seconds. We use less, because a katsu test that takes
/// thirty seconds is a bug rather than a slow test, and a suite of four thousand tests
/// with a sixty second limit takes too long to be run on every commit.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Run one test and classify what happened.
pub fn run(runtime: &Path, suite_root: &Path, test: &Test, timeout: Duration) -> TestResult {
    // The suite root is canonicalized here for the same reason discover() canonicalizes
    // test paths: everything the child resolves is relative to this directory.
    let suite_root = suite_root.canonicalize();
    let suite_root = suite_root.as_deref().unwrap_or(Path::new("."));

    let mut command = Command::new(runtime);
    command
        .args(&test.flags)
        .arg(&test.path)
        .current_dir(suite_root)
        .env("NODE_TEST_DIR", suite_root.join("test/tmp"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return TestResult {
                test: test.name.clone(),
                outcome: Outcome::Fail,
                detail: format!("could not spawn {}: {error}", runtime.display()),
            };
        }
    };

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return TestResult {
                    test: test.name.clone(),
                    outcome: Outcome::Timeout,
                    detail: format!("killed after {}s", timeout.as_secs()),
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                return TestResult {
                    test: test.name.clone(),
                    outcome: Outcome::Fail,
                    detail: format!("waiting on the child failed: {error}"),
                };
            }
        }
    }

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => {
            return TestResult {
                test: test.name.clone(),
                outcome: Outcome::Fail,
                detail: format!("collecting output failed: {error}"),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Node's own `common` module prints this and exits zero when a test decides it cannot
    // run here. Treating it as a pass would inflate the rate with tests that never ran.
    if stdout.contains("1..0 # Skipped") || stdout.starts_with("1..0") {
        return TestResult {
            test: test.name.clone(),
            outcome: Outcome::Skipped,
            detail: first_line(&stdout),
        };
    }

    if output.status.success() {
        TestResult {
            test: test.name.clone(),
            outcome: Outcome::Pass,
            detail: String::new(),
        }
    } else {
        TestResult {
            test: test.name.clone(),
            outcome: Outcome::Fail,
            detail: tail(&stderr, 12),
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

/// The last `lines` lines of some output, which is where the assertion usually is.
fn tail(text: &str, lines: usize) -> String {
    let collected: Vec<&str> = text.lines().collect();
    let start = collected.len().saturating_sub(lines);
    collected[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn the_tail_keeps_the_end_where_the_assertion_is() {
        let text = (1..=20)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(tail(&text, 3), "18\n19\n20");
    }

    #[test]
    fn short_output_survives_intact() {
        assert_eq!(tail("only one line", 12), "only one line");
    }
}
