### [F] Gate 5 (GNU option tables) is non-deterministic when WSL is sick: a fast WSL failure passes the push unjudged, a hang fails it -- 2026-09-25 -- **FIXED 2026-09-25 by lane A, 54e1c8743**

**Status:** FIXED -- lane A's 54e1c8743: every probe's shell prints a marker first, and no marker, a timeout or a runner that will not start all make the checker exit 3 ("could not run, and why"), which pre-push tallies as a loud skip; the boot's bash-oracle gate declines the same way (d62c2e790). The original report follows.

**In short:** before a push is published, a set of checks ("gates") runs over
it, and one of them compares the long-option tables of the ported command-line
utilities against the real GNU ones — which on Windows it reaches through WSL
(the Linux subsystem). If asking WSL fails even once, that check quietly
decides there is no Linux to compare against and lets the push through
unjudged. That is what made lane F's boot test of `76ea696ee` refuse to run
after two hours: the harness's self-test of this gate saw a push it expected
to be refused go through. The same self-test then passed all 124 of its cases
when run alone on the same commit.

**Where.** `scripts/getopt-ambiguity-check.py`, `find_runner`: it runs
`wsl -e true` with a 30-second timeout and returns `None` on *any* failure —
timeout, a WSL service hiccup, a VM being restarted — which `main` reports as
"no GNU userland available (no WSL, not Linux); nothing to check" and exits
0. `scripts/hooks/pre-push`, gate 5, takes that exit as a pass. So "this
machine has no WSL" and "WSL did not answer this time" are the same verdict,
and the second one is a push published without the check.

**How it showed.** `scripts/test-checkers-honour-head.py`,
`case_gate5_the_hook_refuses_a_commit_the_worktree_no_longer_shows`, during a
boot test's tooling phase (heavily loaded: every harness suite runs, and other
lanes were building): "gate 5 end to end: the push is refused — got
'allowed'", and "...naming the option only the commit drops — got False".
The case checks for GNU userland in its *own* process before pushing
(`_gnu_userland_missing`), and the hook probes again in its own; the second
probe failing is exactly this result. Its sibling case asserts from the hook's
tally that gate 5 actually ran; this one asserts only the verdict, so a skip
reads as "allowed" instead of as a skip. Standalone rerun: all 124 cases
passed.

**The other half** (lane B, same outage): when WSL *hangs* rather than failing
fast — `Wsl/Service/HCS_E_CONNECTION_TIMEOUT` after about a minute — the
30-second probe can succeed and a later WSL call in the sweep time out, which
surfaces as an uncaught `subprocess.TimeoutExpired` and exit 1: the gate
*fails* the push instead. So under a sick WSL the same push is waved through
or refused depending on how WSL happens to be failing. Both are wrong.

**The proper fix** is the convention `coreutils-unix-half` already follows
(`scripts/hooks/pre-push`, its `run_checker --may-skip` call): a check that
cannot reach its reference *declines loudly* — exits 2 with the reason on its
first line ("WSL did not answer", not "no GNU userland") — and the hook
records it in its tally as skipped rather than run. In
`getopt-ambiguity-check.py` that means `find_runner` failing, and a WSL call
timing out mid-sweep, both become that decline; and gate 5's `run_checker`
call gains `--may-skip` with the same tally correction. Then have the refusal
self-test assert from the tally that gate 5 ran, as its sibling does, so a
skipped gate can never pass for a refused push.

**How to see it.** It happened machine-wide on 2026-09-25 from about 05:58:
every new `wsl -e ...` failed with `Wsl/Service/E_UNEXPECTED` (lanes B, C and F
all saw it; lane B first as the hang), which also stopped every Linux-side test
and the boot test's rootfs repack. On any host where `wsl -e true` fails fast
when gate 5 runs, the gate prints "no GNU userland available" and the push is
allowed. (Do not provoke it with `wsl --shutdown`: other lanes' builds and tests
run in the same WSL.)
