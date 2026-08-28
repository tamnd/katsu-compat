# katsu-compat

Node.js compatibility testing for [katsu](https://github.com/tamnd/katsu). Runs Node's own test suite against a runtime binary and publishes the pass rate per module, with every single failure named.

**Status: the harness works, the runtime it exists to test does not yet.** katsu is pre M0 and cannot execute JavaScript, so every number here is currently a control run against Node itself. That is deliberate rather than a placeholder. A harness that has never been checked against a runtime known to pass is a harness whose zeroes mean nothing, so it takes the runtime as an argument and Node is the first thing it was pointed at.

## Why this is a separate repository with its own budget

Node compatibility is what kills projects like katsu, and it kills them slowly. There is no single hard problem, there are eleven hundred small ones, and the failure mode is a program that works for twenty minutes and then hits a stream edge case nobody documented.

The only defence that has ever worked is running other people's real test suites instead of writing your own. Writing your own tests means testing the behaviour you already thought of, which is exactly the set of behaviours that are already correct.

The bar to clear is public. Bun claims around 98% of Node's API surface, and Deno lands somewhere in the 90 to 95% range depending on who is counting and what they counted. Those numbers are not directly comparable to each other because nobody agrees on the denominator, which is the second reason this repository exists: to state a denominator and then never quietly change it.

## What it does

`katsu-compat vendor` clones nodejs/node at a tag.

`katsu-compat run` discovers every `test-*.js` under `test/parallel` and `test/es-module`, reads the `// Flags:` directive that Node's own runner honours, runs each test in parallel with a time limit, and classifies the result as pass, fail, timeout or skipped.

Skipped means the test itself decided it could not run here, which Node signals by printing `1..0 # Skipped`. Those are counted separately and kept out of the denominator. A pass rate over a denominator that quietly drops the hard tests is the oldest trick in this field.

`--against expectations.json` compares the run to a committed file and fails on any difference. A test that stops passing fails the build, and so does a test that starts passing, because an improvement that is not committed lets the file slowly accumulate permission to fail. `--bless` is how you update it deliberately.

## Using it

```
cargo build --release
./target/release/katsu-compat vendor --tag v24.18.0
./target/release/katsu-compat run --runtime node --markdown results/node.md
./target/release/katsu-compat run --runtime ../katsu/target/release/katsu --against expectations/katsu.json
```

Stable Rust 1.98 or newer, plus `git` and whichever runtime you are pointing it at.

## Reading the output

The table is sorted worst module first. A table sorted alphabetically hides the problem in the middle, and the whole point of publishing this is that the problem is visible.

Every failing test is listed by name underneath it. A compatibility percentage with no failures behind it is a number nobody can act on, including us.

A module where every test was skipped reports `n/a` rather than `0%` or `100%`, because it has no pass rate and inventing one in either direction would be wrong.

## What is deliberately not measured here

Node's `sequential` tests, for now. They bind fixed ports and assume they are alone on the machine, so they need a different execution model and they will get their own run rather than being quietly folded into this one.

Anything about speed. That is [tamnd/katsu-bench](https://github.com/tamnd/katsu-bench), and mixing correctness and performance into one number is how both stop meaning anything.

Whether a package works, as opposed to whether a test passes. The npm package corpus is the second half of this repository and it is the half that matters more, because passing Node's `fs` tests and failing to install `sharp` is a runtime nobody can use. It lands alongside katsu's M7.

## License

MIT or Apache-2.0, at your option. Node's test suite is vendored rather than committed and carries its own license.
