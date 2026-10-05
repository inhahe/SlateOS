"""Tests for mutation_harness.check_the_table: where a table's tests are found.

A row names the tests that must fail when its mutation is applied, and the
table check refuses a row naming a test that does not exist -- up front,
before a sweep spends a build per row. So the check has to look everywhere a
test can be: the mutated file, the files beside it, and (since 2026-09-27)
the crate's integration tests in `tests/`, where a library's public promises
are pinned -- and the crate whose tests the sweep runs, which is not the
mutated file's own when a library is swept by a program that uses it.

Run with no arguments; exits non-zero on the first failure.
"""

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from mutation_harness import cargo_test_command, check_the_table, failed_tests  # noqa: E402  (path set above)

# `cargo test`'s report when a unit test in a submodule and an integration
# test both fail -- two binaries, each with its own `failures:` summary. The
# first `failures:` of each is followed by the tests' output, not by names.
CARGO_OUTPUT = """running 3 tests
test input::tests::closes_when_asked ... FAILED
test tests::draws_a_frame ... ok
test tests::saves_on_exit ... FAILED

failures:

---- input::tests::closes_when_asked stdout ----
thread panicked at src/input.rs:10:5:
    assertion failed

failures:
    input::tests::closes_when_asked
    tests::saves_on_exit

test result: FAILED. 1 passed; 2 failed; 0 ignored

     Running tests/store.rs (target/debug/deps/store-0123)

running 2 tests
test a_capture_never_reuses_a_taken_id ... FAILED
test a_blob_is_named_by_what_was_stored ... ok

failures:

---- a_capture_never_reuses_a_taken_id stdout ----
    indented text a panic message may hold

failures:
    a_capture_never_reuses_a_taken_id

test result: FAILED. 1 passed; 1 failed; 0 ignored

     Running unittests src/main.rs (target/debug/deps/editor-4567)

running 2 tests
test undo_tests::an_undone_branch_is_kept ... FAILED
test external_merge_tests::a_reload_starts_the_history_again ... FAILED

failures:

---- undo_tests::an_undone_branch_is_kept stdout ----
thread panicked at src/main.rs:20:5:

failures:
    undo_tests::an_undone_branch_is_kept
    external_merge_tests::a_reload_starts_the_history_again

test result: FAILED. 0 passed; 2 failed; 0 ignored
"""

SRC = "pub fn f() -> u8 {\n    1\n}\n"


def crate(tmp: Path, beside: str = "", integration: str = "") -> Path:
    """A crate with `src/lib.rs`, and optionally a file beside it and one in tests/."""
    src = tmp / "src"
    src.mkdir(parents=True)
    (src / "lib.rs").write_text(SRC, encoding="utf-8", newline="")
    if beside:
        (src / "other.rs").write_text(beside, encoding="utf-8", newline="")
    if integration:
        tests = tmp / "tests"
        tests.mkdir()
        (tests / "it.rs").write_text(integration, encoding="utf-8", newline="")
    return src


def row(test: str):
    return [("one is two", "    1\n", "    2\n", [test])]


def check(label: str, got: int, want: int) -> bool:
    ok = got == want
    print(f"  {'ok  ' if ok else 'FAIL'}  {label} (problems: {got}, expected {want})")
    return ok


def main() -> int:
    results = []
    with tempfile.TemporaryDirectory() as d:
        src = crate(Path(d) / "a", integration="#[test]\nfn pinned_in_tests() {}\n")
        results.append(check(
            "a test in the crate's tests/ is found",
            check_the_table(SRC, row("pinned_in_tests"), src), 0))
    with tempfile.TemporaryDirectory() as d:
        src = crate(Path(d) / "b", beside="#[test]\nfn pinned_beside() {}\n")
        results.append(check(
            "a test in a file beside the mutated one is found",
            check_the_table(SRC, row("pinned_beside"), src), 0))
    with tempfile.TemporaryDirectory() as d:
        src = crate(Path(d) / "c", integration="#[test]\nfn something_else() {}\n")
        results.append(check(
            "a test that is nowhere is still refused",
            check_the_table(SRC, row("pinned_nowhere"), src), 1))
    with tempfile.TemporaryDirectory() as d:
        # A directory that is not `src` has no crate root known to hold
        # `tests/`, so a sibling `tests/` is not searched.
        base = Path(d) / "d"
        other = base / "lib"
        other.mkdir(parents=True)
        (other / "lib.rs").write_text(SRC, encoding="utf-8", newline="")
        (base / "tests").mkdir()
        (base / "tests" / "it.rs").write_text("#[test]\nfn only_in_tests() {}\n", encoding="utf-8", newline="")
        results.append(check(
            "only a src/ directory's crate is searched for tests/",
            check_the_table(SRC, row("only_in_tests"), other), 1))
    with tempfile.TemporaryDirectory() as d:
        # A library swept by the tests of a program that uses it: the test is
        # in the program's crate, in a nested module.
        lib = crate(Path(d) / "store")
        user = Path(d) / "app" / "src" / "ui"
        user.mkdir(parents=True)
        (user / "form.rs").write_text("#[test]\nfn pinned_by_the_user() {}\n", encoding="utf-8", newline="")
        results.append(check(
            "a test in the crate the sweep runs is found",
            check_the_table(SRC, row("pinned_by_the_user"), lib, [Path(d) / "app" / "src"]), 0))
        results.append(check(
            "...and is not found when the sweep runs another crate",
            check_the_table(SRC, row("pinned_by_the_user"), lib), 1))
    got = failed_tests(CARGO_OUTPUT)
    want = {
        "closes_when_asked",
        "saves_on_exit",
        "a_capture_never_reuses_a_taken_id",
        # In modules not called `tests`: unread until 2026-09-28.
        "an_undone_branch_is_kept",
        "a_reload_starts_the_history_again",
    }
    ok = got == want
    print(f"  {'ok  ' if ok else 'FAIL'}  failures are read from unit and integration "
          f"binaries alike, whatever the test's module is called (got {sorted(got)})")
    results.append(ok)
    # A sweep's cargo: every test target by default, as before targets could
    # be named; the named ones alone when they are, before what comes last.
    host = ["cargo", "test", "-p", "videocodec", "--target", "x86_64-pc-windows-gnu"]
    for label, got, want in [
        ("a sweep builds every test target by default",
         cargo_test_command("videocodec", (), "--no-run"), host + ["--no-run"]),
        ("...and runs every one",
         cargo_test_command("videocodec", (), "--no-fail-fast"), host + ["--no-fail-fast"]),
        ("a sweep given targets builds and runs those alone",
         cargo_test_command("videocodec", ("--lib", "--test", "subtitles"), "--no-run"),
         host + ["--lib", "--test", "subtitles", "--no-run"]),
    ]:
        ok = got == want
        print(f"  {'ok  ' if ok else 'FAIL'}  {label} (got {got})")
        results.append(ok)
    passed = sum(results)
    print(f"all {passed} mutation_harness tests passed" if all(results)
          else f"{len(results) - passed} of {len(results)} mutation_harness tests FAILED")
    return 0 if all(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
