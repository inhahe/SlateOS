## B-THE-OILS-TESTS-RESOLVED-`grep`/`sed`/`cat`-FROM-THE-CARGO-BUILD-DIRECTORY (lane B, 2026-08-16) — ✅ **FIXED** (`378c71b37`, `051ee45e7`)

**In short:** the shell's test suite passed or failed depending on *what else
had been built* in the same tree. Cargo puts the build directory on the
program-search path before it runs a test, and that directory holds ~200
SlateOS coreutils — so a shell test that piped through `grep` ran *our* `grep`,
whose regular-expression support was a substring search. `cargo test -p oils`
was green and `cargo test --workspace` was red, from the same commit and the
same test binary.

Reported by lane C in
`requests/c-b-workspace-test-red-slateos-coreutils-shadow-host.md`, and it was
urgent: cargo stops at the first failing test binary, so `osh` failing meant
every crate after it alphabetically — `p` through `z` — went untested on every
workspace run anyone did.

### Why it happens

Before running a test binary, cargo prepends `target/<triple>/<profile>/deps`
and the profile directory above it to the platform's dynamic-library search
variable, which on Windows is `PATH` — so that a test can find the shared
libraries its crate links against. It is not trying to stage executables; on
this workspace the same directory simply *is* where every coreutil lands.

So the shadowing is conditional on build order. `cargo test -p oils` in a clean
tree builds no coreutils and the tests find the host's tools. `cargo test
--workspace`, or any run after a `cargo build`, does not.

### Why the tests are right to want the host's tools

The tools these tests reach for are **scaffolding, and the scaffolding has to be
the reference implementation.** They are differential tests against bash's
documented behaviour: `set -o | grep '^posix'` is a question about `set -o`, and
when it fails it must be because `set -o` is wrong. A `grep` of our own in that
position turns every gap in our coreutils into a shell-test failure filed
against the shell — the defect misattributed, and the real one hidden behind it.

That is why fixing the harness is not hiding the bug. The coreutils gaps it
uncovered are real and are fixed where they live; see
`B-FOUR-PROGRAMS-MATCHED-REGULAR-EXPRESSIONS-WITH-str::contains` below.

### The fix

`userspace/oils/src/hostpath.rs`. It derives the two injected directories from
`std::env::current_exe()` — `…/deps/<test binary>`, so its parent and
grandparent — rather than matching on path shape, which keeps it working under
a custom `--target-dir`, another profile, or a different triple. It then strikes
them out of the ambient `PATH`.

Two suites need it and they reach the child differently:

| suite | how the shell is created | how the path reaches it |
|---|---|---|
| `src/interp.rs`'s `#[cfg(test)]` | a `Shell` in this process | bound as the shell **variable** `PATH` in `new_shell()` |
| `tests/*.rs` | the real `osh` binary, spawned | `Command::env("PATH", …)`, via `hostpath::scrub` |

The in-process half deliberately does **not** mutate the process environment:
libtest runs every test on a thread, and `std::env::set_var` is unsound while
another thread may be reading the environment — which is why Rust 2024 made it
`unsafe`. A shell variable is per-shell and touches nothing shared. The
end-to-end half cannot use a shell variable at all, because a child inherits the
*process* environment; that is why the first commit fixed only the unit tests
and `redirect_dup`'s `{ … | sed 's/^/piped: /'; } 2>&1` stayed red.

`hostpath.rs` lives in `src/` and is declared `#[cfg(test)]` by `lib.rs`, while
each integration test pulls the same file in with
`#[path = "../src/hostpath.rs"]`. One definition serves both suites and none of
it reaches the shipped shell — a `tests/common/mod.rs` would have served only
the second, and a `pub` module only by shipping test scaffolding.

### What guards it

`a_test_shell_resolves_commands_outside_the_build_directory` (in `interp.rs`)
asserts that neither injected directory is on the test shell's `$PATH` and that
`command -v cat` does not resolve into the build tree. `hostpath`'s own three
tests assert the same of the `OsString`, and — the other half, which is easy to
forget — that *nothing else* was dropped: a scrub that removed too much would
fail every test that needs a real `sed`, for a reason of its own making.

### The general shape, for whoever hits this next

**A test suite whose result depends on what else is in the build directory is
not testing the code.** Any crate here that shells out to a program by name has
the same exposure — the build directory is on the search path for the whole
workspace, not just for `oils`. If you write such a test, take `hostpath` with
it rather than re-deriving it.
