# Files

- [Format Crate Test Strategy](format-crate-tests.md) - How the crates under crates/ prove correctness against their upstream references - round-trip suites that exercise the crate alone, interop suites that run maxcso, xdelta3 or Flips and skip rather than fail when the tool is absent, cross-language known-answer vectors for nsz-rs crypto, byte-sweep malformed-input suites, the repeated progress and .part invariants, and the one ignored 2 GiB slow test.
- [Test Fixtures and End-to-End Suites](test-fixtures.md)
- [Test Harness and Conventions](testing-conventions.md) - How oxyromon's tests are structured and run — module-per-test files wired in with cfg(test), the temp-DB/temp-dir skeleton guarded by the global test MUTEX, prompts that never block under cfg(test), tool-dependent tests that skip rather than fail, and the workspace test and coverage commands.
