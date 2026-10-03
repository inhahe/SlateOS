## TD-B-THE-UNIX-HALF-GATE-COMPILES-THE-LINUX-TESTS-AND-NEVER-RUNS-THEM (lane B, 2026-10-03)

**Status:** OPEN -- the fix is a one-flag change to `scripts/hooks/pre-push`,
held back only because it reaches other lanes' pushes; see "What to do".

**In short:** Before a push, gate 12 builds every changed crate for Linux
through WSL, so code that only compiles on Unix cannot go in broken. It does
not *run* that crate's tests on Linux, and the comment beside it says that is
because they cannot be run from this machine. They can: the same script runs
them through WSL in about a minute. So a test that passes on the Windows
development host and fails on Linux -- which is where everything here actually
runs -- goes in green, and two did on 2026-10-03.

### What it let in

| crate | test | why it is red on Linux only |
|---|---|---|
| `coreutils` | `pgrep::tests::selection_follows_upstreams_order`, `names_and_command_lines_go_with_the_pids` | asserted ascending PIDs from a fixture directory; ext4 lists in hash order, NTFS sorts |
| `udevd` | `tests::daemon_reload_rules` | "reload from a nonexistent directory" read the real `/etc/udev/rules.d`, which exists under Linux |

Both fixed (`ef8c1ca44`, `b2edfd373`). Both were found by running
`scripts/coreutils-check.sh --only linux` by hand, which is the point: nothing
in the normal workflow runs it with tests.

### Why the gate says what it says

`scripts/hooks/pre-push` passes `--no-test`, with this justification:

> Running the linux tests additionally needs a `cc` for
> x86_64-unknown-linux-gnu to link with, which this machine lacks, and then
> needs to EXECUTE a linux binary on Windows, which is not possible at all.

That describes running the Linux half *on the Windows side*. The script does
not do that: `run_linux` runs one `wsl -e bash -c` for the whole half, where
`cc` exists and Linux binaries execute. Measured 2026-10-03 on this host:

    bash scripts/coreutils-check.sh --only linux --no-clippy -p coreutils -- pgrep
      -> links, runs the lib and every bin's tests, "clean (linux half checked)"
    bash scripts/coreutils-check.sh --only linux --no-clippy --dir userspace/udevd
      -> 98 passed, 1 failed (the test above)

The entry that introduced `--no-test`
(`known-issues-resolved/B-td-b-the-unix-half-gate-cannot-link-on-this-host-lane-b-2026-09-10.md`)
recorded `error: linker 'cc' not found` for `udevd` on 2026-09-10. Whatever
produced that then, it does not reproduce now.

### What to do

1. Drop `--no-test` from gate 12's invocation (and from the two "reproduce"
   lines in its refusal text), and correct the comment above it.
2. Before that, run every crate in the gate's scope through
   `coreutils-check.sh --only linux` once, and fix or file what is red. The
   gate's scope is computed per push from the crates touched, so a crate that
   is red today would refuse the next push that touches it -- including
   another lane's (`services/**` is lane D's and has `cfg(unix)` arms).
3. Cost: the test half of the Linux run was measured at 1m04s on 2026-09-04
   against a warm target directory. It is paid only by pushes that touch a
   crate with a platform-conditional arm.

Step 2 is why this is not done in the same change: turning the tests on is a
one-line edit to a hook every lane runs, and doing it before the survey would
hand another lane a refusal for a test it has never seen fail.
