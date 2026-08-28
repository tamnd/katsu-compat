//! Turning a run into the table that gets published.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::outcome::{Outcome, TestResult};

/// The module names Node's own test file naming convention uses.
const KNOWN: &[&str] = &[
    "assert",
    "async",
    "buffer",
    "child",
    "cluster",
    "console",
    "crypto",
    "dgram",
    "diagnostics",
    "dns",
    "domain",
    "error",
    "esm",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf",
    "process",
    "promises",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "require",
    "stream",
    "string",
    "timers",
    "tls",
    "trace",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker",
    "zlib",
];

/// The `node:` module a test belongs to, guessed from its file name.
///
/// Node's own naming convention is `test-<module>-<detail>.js`, which is regular enough to
/// group by and irregular enough that some tests land in `other`. Reporting `other` as its
/// own row is better than forcing every test into a module it does not belong to.
pub fn module_of(test_name: &str) -> String {
    let file = test_name.rsplit('/').next().unwrap_or(test_name);
    let stem = file
        .trim_start_matches("test-")
        .split('.')
        .next()
        .unwrap_or_default();
    let first = stem.split('-').next().unwrap_or_default();

    if KNOWN.contains(&first) {
        first.to_owned()
    } else {
        "other".to_owned()
    }
}

/// Counts for one module.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModuleTally {
    pub pass: usize,
    pub fail: usize,
    pub timeout: usize,
    pub skipped: usize,
}

impl ModuleTally {
    /// Tests actually attempted, which is the denominator the rate is over.
    pub fn attempted(self) -> usize {
        self.pass + self.fail + self.timeout
    }

    /// Pass rate as a percentage, or `None` when nothing was attempted.
    ///
    /// Returning `None` rather than zero matters: a module where every test was skipped has
    /// no pass rate, and printing 0% for it would be as wrong as printing 100%.
    pub fn rate(self) -> Option<f64> {
        let attempted = self.attempted();
        if attempted == 0 {
            None
        } else {
            #[allow(clippy::cast_precision_loss)]
            Some(self.pass as f64 * 100.0 / attempted as f64)
        }
    }
}

/// Tally a run by module.
pub fn tally(results: &[TestResult]) -> BTreeMap<String, ModuleTally> {
    let mut by_module: BTreeMap<String, ModuleTally> = BTreeMap::new();
    for result in results {
        let entry = by_module.entry(module_of(&result.test)).or_default();
        match result.outcome {
            Outcome::Pass => entry.pass += 1,
            Outcome::Fail => entry.fail += 1,
            Outcome::Timeout => entry.timeout += 1,
            Outcome::Skipped => entry.skipped += 1,
        }
    }
    by_module
}

/// Render the published compatibility table.
///
/// Worst modules first. A table sorted alphabetically hides the problem in the middle,
/// and the whole point of publishing this is that the problem is visible.
pub fn markdown(runtime: &str, suite: &str, results: &[TestResult]) -> String {
    let by_module = tally(results);

    let total: ModuleTally = by_module
        .values()
        .fold(ModuleTally::default(), |mut acc, t| {
            acc.pass += t.pass;
            acc.fail += t.fail;
            acc.timeout += t.timeout;
            acc.skipped += t.skipped;
            acc
        });

    let mut out = String::new();
    let _ = writeln!(out, "# Node.js compatibility");
    let _ = writeln!(out);
    let _ = writeln!(out, "Runtime: `{runtime}`. Suite: `{suite}`.");
    let _ = writeln!(out);
    match total.rate() {
        Some(rate) => {
            let _ = writeln!(
                out,
                "{} of {} attempted tests pass, which is {rate:.1}%. {} {} skipped and {} not in that denominator.",
                total.pass,
                total.attempted(),
                total.skipped,
                if total.skipped == 1 {
                    "test was"
                } else {
                    "tests were"
                },
                if total.skipped == 1 {
                    "it is"
                } else {
                    "they are"
                }
            );
        }
        None => {
            let _ = writeln!(out, "No tests were attempted.");
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "| Module | Pass | Fail | Timeout | Skipped | Rate |");
    let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|");

    let mut rows: Vec<(&String, &ModuleTally)> = by_module.iter().collect();
    rows.sort_by(|a, b| {
        let left = a.1.rate().unwrap_or(f64::MAX);
        let right = b.1.rate().unwrap_or(f64::MAX);
        left.partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(b.0))
    });

    for (module, tally) in rows {
        let rate = tally
            .rate()
            .map_or_else(|| "n/a".to_string(), |r| format!("{r:.1}%"));
        let _ = writeln!(
            out,
            "| `{module}` | {} | {} | {} | {} | {rate} |",
            tally.pass, tally.fail, tally.timeout, tally.skipped
        );
    }

    let failures: Vec<&TestResult> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail | Outcome::Timeout))
        .collect();

    if !failures.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Every failure");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "All {} of them, because a compatibility number without the failures behind it is a number nobody can act on.",
            failures.len()
        );
        let _ = writeln!(out);
        for failure in failures {
            let _ = writeln!(out, "- `{}` ({:?})", failure.test, failure.outcome);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{ModuleTally, markdown, module_of, tally};
    use crate::outcome::{Outcome, TestResult};

    fn result(test: &str, outcome: Outcome) -> TestResult {
        TestResult {
            test: test.into(),
            outcome,
            detail: String::new(),
        }
    }

    #[test]
    fn tests_are_grouped_by_the_module_their_name_names() {
        assert_eq!(module_of("test/parallel/test-fs-read.js"), "fs");
        assert_eq!(module_of("test/parallel/test-http2-server.js"), "http2");
        assert_eq!(module_of("test/es-module/test-esm-loader.mjs"), "esm");
    }

    #[test]
    fn a_name_that_matches_no_module_lands_in_other_rather_than_being_forced() {
        assert_eq!(module_of("test/parallel/test-blorp-thing.js"), "other");
    }

    #[test]
    fn a_module_with_nothing_but_skips_has_no_rate_at_all() {
        let tallied = tally(&[result("test/parallel/test-vm-a.js", Outcome::Skipped)]);
        assert_eq!(tallied["vm"].rate(), None);
    }

    #[test]
    fn skipped_tests_stay_out_of_the_denominator() {
        let t = ModuleTally {
            pass: 3,
            fail: 1,
            timeout: 0,
            skipped: 96,
        };
        assert_eq!(t.attempted(), 4);
        assert!((t.rate().unwrap() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_report_lists_the_failures_rather_than_only_counting_them() {
        let results = vec![
            result("test/parallel/test-fs-a.js", Outcome::Pass),
            result("test/parallel/test-http-b.js", Outcome::Fail),
        ];
        let out = markdown("node v24.18.0", "nodejs/node v24.18.0", &results);
        assert!(
            out.contains("test-http-b.js"),
            "the failing test must be named"
        );
        assert!(out.contains("50.0%"));
    }
}
