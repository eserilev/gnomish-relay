//! The summary lines of test tools in the output of a command (SPEC.md 9.10). The agent
//! writes this output, so the counts are a report, not a proof.

/// The counts of one test command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TestCounts {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
}

impl TestCounts {
    fn add(&mut self, other: TestCounts) {
        self.passed = self.passed.saturating_add(other.passed);
        self.failed = self.failed.saturating_add(other.failed);
        self.skipped = self.skipped.saturating_add(other.skipped);
    }
}

/// The number right before `word` in `text`, as in "12 passed".
fn number_before(text: &str, word: &str) -> Option<u32> {
    let at = text.find(word)?;
    let before = text[..at].trim_end();
    let digits: String = before
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// The number right after `word` in `text`, as in "# pass 12".
fn number_after(text: &str, word: &str) -> Option<u32> {
    let rest = text.strip_prefix(word)?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn counts(line: &str, passed: &str, failed: &str, skipped: &str) -> TestCounts {
    TestCounts {
        passed: number_before(line, passed).unwrap_or(0),
        failed: number_before(line, failed).unwrap_or(0),
        skipped: number_before(line, skipped).unwrap_or(0),
    }
}

/// `test result: ok. 12 passed; 0 failed; 1 ignored; …` of `cargo test`.
fn cargo_line(line: &str) -> Option<TestCounts> {
    let rest = line.trim().strip_prefix("test result: ")?;
    Some(counts(rest, " passed", " failed", " ignored"))
}

/// `Summary [ 1.2s] 412 tests run: 410 passed, 2 failed, 3 skipped` of `cargo nextest`.
fn nextest_line(line: &str) -> Option<TestCounts> {
    let line = line.trim();
    if !line.starts_with("Summary [")
        || !line.contains(" tests run: ") && !line.contains(" test run: ")
    {
        return None;
    }
    let rest = line.split_once(" run: ")?.1;
    Some(counts(rest, " passed", " failed", " skipped"))
}

/// `Tests:       2 failed, 410 passed, 412 total` of jest.
fn jest_line(line: &str) -> Option<TestCounts> {
    let rest = line.trim().strip_prefix("Tests:")?;
    rest.contains(" total")
        .then(|| counts(rest, " passed", " failed", " skipped"))
}

/// ` Tests  2 failed | 410 passed (412)` of vitest.
fn vitest_line(line: &str) -> Option<TestCounts> {
    let rest = line.trim().strip_prefix("Tests ")?;
    let rest = rest.trim_start();
    let known = rest.contains(" passed") || rest.contains(" failed");
    (known && rest.ends_with(')')).then(|| counts(rest, " passed", " failed", " skipped"))
}

/// `==== 2 failed, 410 passed, 3 skipped in 1.2s ====` of pytest.
fn pytest_line(line: &str) -> Option<TestCounts> {
    let line = line.trim();
    let inner = line.strip_prefix('=')?.trim_start_matches('=').trim();
    let inner = inner.strip_suffix('=')?.trim_end_matches('=').trim();
    let known = inner.contains(" passed") || inner.contains(" failed") || inner.contains(" error");
    if !known || !inner.contains(" in ") {
        return None;
    }
    let mut found = counts(inner, " passed", " failed", " skipped");
    found.failed = found
        .failed
        .saturating_add(number_before(inner, " error").unwrap_or(0));
    Some(found)
}

/// One whole summary line of a tool.
fn summary_line(line: &str) -> Option<TestCounts> {
    cargo_line(line)
        .or_else(|| nextest_line(line))
        .or_else(|| jest_line(line))
        .or_else(|| vitest_line(line))
        .or_else(|| pytest_line(line))
}

/// `412 passing (2s)` of mocha: the number first, then the word.
fn number_first(line: &str, word: &str) -> Option<u32> {
    let (number, rest) = line.split_once(' ')?;
    rest.starts_with(word).then(|| number.parse().ok())?
}

/// The tools with one line for each count: mocha, `node --test`, and `go test`.
#[derive(Default)]
struct CountLines {
    mocha: TestCounts,
    node: TestCounts,
    go_tests: TestCounts,
    go_packages: TestCounts,
}

impl CountLines {
    fn read(&mut self, line: &str) {
        let trimmed = line.trim();
        let add = |count: &mut u32, n: Option<u32>| *count = count.saturating_add(n.unwrap_or(0));
        add(&mut self.mocha.passed, number_first(trimmed, "passing"));
        add(&mut self.mocha.failed, number_first(trimmed, "failing"));
        add(&mut self.node.passed, number_after(trimmed, "# pass"));
        add(&mut self.node.failed, number_after(trimmed, "# fail"));
        add(
            &mut self.go_tests.passed,
            trimmed.starts_with("--- PASS: ").then_some(1),
        );
        add(
            &mut self.go_tests.failed,
            trimmed.starts_with("--- FAIL: ").then_some(1),
        );
        add(
            &mut self.go_packages.passed,
            line.starts_with("ok  \t").then_some(1),
        );
        add(
            &mut self.go_packages.failed,
            line.starts_with("FAIL\t").then_some(1),
        );
    }

    fn best(self) -> Option<TestCounts> {
        [self.mocha, self.node, self.go_tests, self.go_packages]
            .into_iter()
            .find(|c| c.passed > 0 || c.failed > 0)
    }
}

/// The summary of one command, or `None` when no line is a summary line of a test tool.
pub fn summary(output: &str) -> Option<TestCounts> {
    let mut total: Option<TestCounts> = None;
    let mut count_lines = CountLines::default();
    for line in output.lines() {
        if let Some(found) = summary_line(line) {
            total.get_or_insert_with(TestCounts::default).add(found);
        }
        count_lines.read(line);
    }
    total.or_else(|| count_lines.best())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(passed: u32, failed: u32, skipped: u32) -> TestCounts {
        TestCounts {
            passed,
            failed,
            skipped,
        }
    }

    #[test]
    fn cargo_test_adds_up_the_result_line_of_each_crate() {
        let output = "running 3 tests\n\
            test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\
            test result: FAILED. 7 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s\n";

        assert_eq!(summary(output), Some(tally(10, 2, 1)));
    }

    #[test]
    fn cargo_nextest_reads_its_summary_line() {
        let output = "     Summary [   1.234s] 412 tests run: 410 passed, 2 failed, 3 skipped\n";

        assert_eq!(summary(output), Some(tally(410, 2, 3)));
    }

    #[test]
    fn jest_reads_its_tests_line() {
        let output = "Test Suites: 1 failed, 9 passed, 10 total\nTests:       2 failed, 410 passed, 412 total\n";

        assert_eq!(summary(output), Some(tally(410, 2, 0)));
    }

    #[test]
    fn vitest_reads_its_tests_line() {
        let output =
            " Test Files  1 failed | 4 passed (5)\n      Tests  2 failed | 410 passed (412)\n";

        assert_eq!(summary(output), Some(tally(410, 2, 0)));
    }

    #[test]
    fn mocha_reads_passing_and_failing() {
        let output = "\n  412 passing (2s)\n  2 failing\n\n  1) thing\n";

        assert_eq!(summary(output), Some(tally(412, 2, 0)));
    }

    #[test]
    fn node_test_reads_its_pass_and_fail_lines() {
        let output = "# tests 12\n# pass 10\n# fail 2\n";

        assert_eq!(summary(output), Some(tally(10, 2, 0)));
    }

    #[test]
    fn pytest_reads_its_last_line_and_counts_errors_as_failed() {
        let output = "==== 2 failed, 410 passed, 3 skipped, 1 error in 1.23s ====\n";

        assert_eq!(summary(output), Some(tally(410, 3, 3)));
        assert_eq!(
            summary("============ 12 passed in 0.50s ============"),
            Some(tally(12, 0, 0))
        );
    }

    #[test]
    fn go_test_counts_tests_with_v_else_packages() {
        let verbose = "=== RUN   TestA\n--- PASS: TestA (0.00s)\n--- FAIL: TestB (0.00s)\nFAIL\tapp/x\t0.01s\n";
        let quiet = "ok  \tapp/a\t0.01s\nFAIL\tapp/b\t0.02s\nok  \tapp/c\t(cached)\n";

        assert_eq!(summary(verbose), Some(tally(1, 1, 0)));
        assert_eq!(summary(quiet), Some(tally(2, 1, 0)));
    }

    #[test]
    fn output_with_no_summary_line_has_no_counts() {
        assert_eq!(summary("Compiling app v0.1.0\nFinished dev\n"), None);
        assert_eq!(summary(""), None);
        assert_eq!(summary("Tests: nothing here"), None);
    }
}
