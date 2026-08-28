//! End to end tests against a real Node.js binary.
//!
//! The fixture suite under `tests/fixture-suite` mimics the shape of nodejs/node closely
//! enough to exercise discovery, the `// Flags:` directive, skip detection and the report,
//! without cloning a repository measured in gigabytes. Running it against Node itself is
//! the control: a harness that has never been checked against a runtime known to pass is a
//! harness whose zeroes mean nothing.

use std::path::Path;
use std::process::Command;

fn node() -> Option<std::path::PathBuf> {
    which("node")
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

#[test]
fn the_fixture_suite_produces_the_expected_verdicts_under_node() {
    let Some(node) = node() else {
        eprintln!("skipping: no node on PATH");
        return;
    };

    let out = std::env::temp_dir().join("katsu-compat-fixture.json");
    let markdown = std::env::temp_dir().join("katsu-compat-fixture.md");

    let status = Command::new(env!("CARGO_BIN_EXE_katsu-compat"))
        .args(["run", "--runtime"])
        .arg(&node)
        .args(["--suite", "tests/fixture-suite", "--timeout", "30", "--out"])
        .arg(&out)
        .arg("--markdown")
        .arg(&markdown)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("the harness should run");
    assert!(status.success(), "the harness exited non zero");

    let report = std::fs::read_to_string(&markdown).expect("a report should have been written");

    // The fixture is built so that exactly one test passes per assertion below. If any of
    // these move, the harness has changed behaviour rather than the runtime having.
    assert!(
        report.contains("| `fs` | 1 | 0 | 0 | 0 | 100.0% |"),
        "{report}"
    );
    assert!(
        report.contains("| `http` | 0 | 1 | 0 | 0 | 0.0% |"),
        "{report}"
    );
    assert!(
        report.contains("| `vm` | 0 | 0 | 0 | 1 | n/a |"),
        "{report}"
    );

    // test-util-flags.js only passes if the `// Flags:` directive was read and passed
    // through to the child process, so this line is the test for that feature.
    assert!(
        report.contains("| `util` | 1 | 0 | 0 | 0 | 100.0% |"),
        "{report}"
    );

    assert!(
        report.contains("1 test was skipped and it is not in that denominator"),
        "{report}"
    );
    assert!(
        report.contains("test-http-fails.js"),
        "every failure must be named: {report}"
    );

    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&markdown);
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixture-suite")
            .is_dir()
    );
}
