//! What happened when one test ran, and how a run compares to the last known one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The result of running one test file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Exited zero.
    Pass,
    /// Exited non zero, or was killed by a signal.
    Fail,
    /// Ran past the time limit and was killed.
    Timeout,
    /// The test declares that it needs something this build does not have, for example a
    /// crypto backend or internationalization data. Not a failure, and counted separately
    /// so that it cannot be quietly used to inflate a pass rate.
    Skipped,
}

impl Outcome {
    /// The single character used in the progress stream.
    pub const fn glyph(self) -> char {
        match self {
            Outcome::Pass => '.',
            Outcome::Fail => 'F',
            Outcome::Timeout => 'T',
            Outcome::Skipped => 's',
        }
    }
}

/// One test's result, with enough of the failure to be actionable without rerunning it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestResult {
    /// Path relative to the suite root, which is the stable identifier for a test.
    pub test: String,
    /// What happened.
    pub outcome: Outcome,
    /// The tail of stderr, when it failed. Empty on a pass.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// A whole run, ready to be written to disk and diffed against the next one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Expectations {
    /// Which runtime produced this, for example `node v24.18.0`.
    pub runtime: String,
    /// Which suite it was run against, for example `nodejs/node v24.18.0`.
    pub suite: String,
    /// Every test, keyed by path so the file has a stable order and diffs cleanly.
    pub results: BTreeMap<String, Outcome>,
}

impl Expectations {
    /// Build an expectations file from a finished run.
    pub fn from_results(runtime: String, suite: String, results: &[TestResult]) -> Self {
        Self {
            runtime,
            suite,
            results: results
                .iter()
                .map(|r| (r.test.clone(), r.outcome))
                .collect(),
        }
    }

    /// How many tests passed.
    pub fn passing(&self) -> usize {
        self.results
            .values()
            .filter(|o| **o == Outcome::Pass)
            .count()
    }

    /// How many tests were actually attempted, which excludes the skipped ones.
    ///
    /// A pass rate over a denominator that quietly drops the hard tests is the oldest trick
    /// in this field, so the two numbers are kept apart everywhere.
    pub fn attempted(&self) -> usize {
        self.results
            .values()
            .filter(|o| **o != Outcome::Skipped)
            .count()
    }
}

/// What changed between the committed expectations and this run.
#[derive(Debug, Default)]
pub struct Diff {
    /// Tests that used to pass and now do not. Always a build failure.
    pub regressed: Vec<(String, Outcome)>,
    /// Tests that now pass and did not before. Also a build failure, because an
    /// improvement that is not committed lets the file quietly accumulate permission to
    /// fail, which is exactly what the ratchet exists to prevent.
    pub fixed: Vec<String>,
    /// Tests that are in this run but not in the expectations file.
    pub added: Vec<(String, Outcome)>,
    /// Tests that are in the expectations file but did not run.
    pub removed: Vec<String>,
}

impl Diff {
    /// Whether anything at all moved.
    pub fn is_empty(&self) -> bool {
        self.regressed.is_empty()
            && self.fixed.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
    }
}

/// Compare a run against the committed expectations.
pub fn diff(expected: &Expectations, actual: &Expectations) -> Diff {
    let mut out = Diff::default();

    for (test, &now) in &actual.results {
        match expected.results.get(test) {
            None => out.added.push((test.clone(), now)),
            Some(&before) if before == now => {}
            Some(Outcome::Pass) => out.regressed.push((test.clone(), now)),
            Some(_) if now == Outcome::Pass => out.fixed.push(test.clone()),
            Some(_) => {}
        }
    }

    for test in expected.results.keys() {
        if !actual.results.contains_key(test) {
            out.removed.push(test.clone());
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{Expectations, Outcome, diff};

    fn expectations(pairs: &[(&str, Outcome)]) -> Expectations {
        Expectations {
            runtime: "test".into(),
            suite: "test".into(),
            results: pairs.iter().map(|(t, o)| ((*t).to_string(), *o)).collect(),
        }
    }

    #[test]
    fn a_test_that_stops_passing_is_a_regression() {
        let before = expectations(&[("a.js", Outcome::Pass)]);
        let after = expectations(&[("a.js", Outcome::Fail)]);
        let d = diff(&before, &after);
        assert_eq!(d.regressed.len(), 1);
        assert!(d.fixed.is_empty());
    }

    #[test]
    fn a_test_that_starts_passing_also_has_to_be_committed() {
        let before = expectations(&[("a.js", Outcome::Fail)]);
        let after = expectations(&[("a.js", Outcome::Pass)]);
        let d = diff(&before, &after);
        assert_eq!(d.fixed, vec!["a.js".to_string()]);
        assert!(d.regressed.is_empty());
    }

    #[test]
    fn skipped_tests_are_outside_the_denominator() {
        let run = expectations(&[
            ("a.js", Outcome::Pass),
            ("b.js", Outcome::Fail),
            ("c.js", Outcome::Skipped),
        ]);
        assert_eq!(run.passing(), 1);
        assert_eq!(run.attempted(), 2);
    }

    #[test]
    fn an_unchanged_run_produces_no_diff() {
        let run = expectations(&[("a.js", Outcome::Pass), ("b.js", Outcome::Fail)]);
        assert!(diff(&run, &run).is_empty());
    }
}
