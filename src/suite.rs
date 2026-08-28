//! Finding tests in a checkout of nodejs/node and reading what each one needs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// One test file and the things Node's own runner would have done for it.
#[derive(Clone, Debug)]
pub struct Test {
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Path relative to the suite root. The stable identifier.
    pub name: String,
    /// Flags from the `// Flags:` comment, which Node's runner passes to the process.
    ///
    /// Ignoring these silently would make a set of tests fail for a reason that has nothing
    /// to do with compatibility, which is worse than not running them.
    pub flags: Vec<String>,
}

/// The directories worth running, and why.
///
/// `parallel` is the bulk of the suite and is safe to run concurrently. `es-module` is the
/// ESM surface, which is where the semantics differ most from CommonJS and therefore where
/// a runtime is most likely to be quietly wrong. `sequential` is excluded here and run
/// separately, because those tests bind ports and assume they are alone on the machine.
const DIRECTORIES: &[&str] = &["test/parallel", "test/es-module"];

/// Find every test under a checkout of nodejs/node.
///
/// Paths come back absolute. The runner sets the child's working directory to the suite
/// root, because Node's tests resolve `require('../common')` and their fixtures relative to
/// it, and a path that was relative to our own working directory would stop resolving the
/// moment we did that.
pub fn discover(root: &Path, filter: Option<&str>) -> Result<Vec<Test>> {
    let root = &root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let mut tests = Vec::new();

    for directory in DIRECTORIES {
        let base = root.join(directory);
        if !base.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&base).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !matches!(extension, "js" | "mjs" | "cjs") {
                continue;
            }
            let stem = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !stem.starts_with("test-") {
                continue;
            }
            let name = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if filter.is_some_and(|f| !name.contains(f)) {
                continue;
            }
            let source = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            tests.push(Test {
                flags: parse_flags(&source),
                name,
                path: path.to_path_buf(),
            });
        }
    }

    tests.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tests)
}

/// Read the `// Flags:` directive Node's own test runner honours.
///
/// The directive appears in the first few lines and may appear more than once. Anything
/// after the marker up to the end of the line is split on whitespace.
fn parse_flags(source: &str) -> Vec<String> {
    let mut flags = Vec::new();
    for line in source.lines().take(40) {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("// Flags:") {
            flags.extend(rest.split_whitespace().map(str::to_owned));
        }
    }
    flags
}

/// The version string of a checked out suite, read from its `src/node_version.h`.
///
/// Reported next to every pass rate, because a pass rate against an unnamed suite version
/// is not a number anybody can check.
pub fn suite_version(root: &Path) -> Result<String> {
    let header = root.join("src/node_version.h");
    let text = std::fs::read_to_string(&header)
        .with_context(|| format!("reading {}", header.display()))?;

    let field = |name: &str| -> Option<&str> {
        text.lines()
            .find(|l| l.starts_with(&format!("#define {name} ")))
            .and_then(|l| l.rsplit(' ').next())
    };

    match (
        field("NODE_MAJOR_VERSION"),
        field("NODE_MINOR_VERSION"),
        field("NODE_PATCH_VERSION"),
    ) {
        (Some(major), Some(minor), Some(patch)) => {
            Ok(format!("nodejs/node v{major}.{minor}.{patch}"))
        }
        _ => Ok("nodejs/node (version unknown)".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_flags;

    #[test]
    fn flags_are_read_from_the_directive_node_uses() {
        let source = "'use strict';\n// Flags: --expose-internals --no-warnings\nrequire('x');";
        assert_eq!(
            parse_flags(source),
            vec!["--expose-internals", "--no-warnings"]
        );
    }

    #[test]
    fn more_than_one_directive_accumulates() {
        let source = "// Flags: --a\n// Flags: --b\n";
        assert_eq!(parse_flags(source), vec!["--a", "--b"]);
    }

    #[test]
    fn a_file_with_no_directive_has_no_flags() {
        assert!(parse_flags("console.log(1);").is_empty());
    }

    #[test]
    fn a_directive_below_the_header_is_not_read() {
        let source = format!("{}// Flags: --late\n", "\n".repeat(60));
        assert!(parse_flags(&source).is_empty());
    }
}
