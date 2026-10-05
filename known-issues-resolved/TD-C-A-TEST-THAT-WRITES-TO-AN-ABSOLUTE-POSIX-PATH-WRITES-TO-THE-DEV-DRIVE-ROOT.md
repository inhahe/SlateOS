## TD-C-A-TEST-THAT-WRITES-TO-AN-ABSOLUTE-POSIX-PATH-WRITES-TO-THE-DEV-DRIVE-ROOT -- FIXED 2026-09-13 by lane B

**Date:** 2026-09-13. **Lane:** C filed it; **the fix is lane B's** --
`userspace/**`, which lane C must not write. Filed to them as
`requests/c-b-tests-create-real-directories-at-the-drive-root.md`.

**In short:** on this Windows development machine a path beginning with `/` is
not an absent Linux path, it is a path on whatever drive the tests are running
from. `/sys/fs/cgroup` means `E:\sys\fs\cgroup`, and a test that creates it
succeeds. One did, during a workspace run this afternoon, and two `systemctl`
tests that assert "this machine has no cgroups" then failed on every run
afterwards -- not intermittently, permanently, until the directory was deleted
by hand.

**The evidence, because "it was load" is the usual first guess and was wrong
here:**

| run | result | left `E:\sys` behind? |
|---|---|---|
| `cargo test --workspace` | 25 484 passed, **2 failed** | yes |
| `cargo test -p cgroup` alone | 13 passed | no |
| `cargo test -p systemctl` alone, after deleting `E:\sys` | **154 passed** | no |

So neither crate's own suite creates it and neither has a logic bug. A third
crate's tests create the path and these two then fail for everybody.

**It is a family.** `E:\run\firejail\40084.sandbox`, `E:\var\lib\audit\rules.state`
and `E:\dev\test_dev` all exist on this machine for the same reason, from
`let _ = fs::create_dir_all("/run/polkit-1")` and six siblings in `polkit`,
`powerctl` and `udevd`. On the target OS those lines are right. On the dev host
they are silent writes to the root of the operator's data drive, and the
discarded `Result` means neither success nor failure is ever reported.

**Why it matters more than the litter does.** The state the test asserts about
is not state the test controls. "This machine has no cgroups" is an ambient
fact any test in any crate can falsify from the far side of the workspace, and
the failure then surfaces in a crate that did not change. Lane C has already
paid for this shape once, in
`BUG-C-THE-KEYBOARD-LAYOUT-TEST-FAILS-ABOUT-ONE-WORKSPACE-RUN-IN-TWO`, where a
test wrote settings outside a scratch directory and a neighbour read them --
which is what gate 35 now refuses.

**What the proper fix is** (lane B's to make): make the cgroup root injectable
so the two tests point at a scratch path they own -- the sibling test
`the_tree_is_the_directories_that_are_actually_there` already uses
`scratchdir::ScratchDir` and shows the shape -- and fix whichever test creates
`/sys/fs/cgroup/<name>`. A gate refusing `create_dir_all` on a literal starting
`/` inside `#[cfg(test)]` would catch the whole family; **lane C has
deliberately not added one**, because it would refuse lane B's pushes and that
is their decision to make, not ours.

**Second instance the same afternoon, and it is the reason this matters.**
`init/loginmgr` failed six tests in the 14:2x workspace run, all with
*save_user_database writes the whole database, not a subset*, because something
had written `E:\etc\passwd`, `E:\etc\shadow` and `E:\etc\users.yaml`. With
`E:\etc` deleted, `cargo test -p loginmgr` is 46 passed, 0 failed, and running
it alone does not recreate the directory.

Three workspace runs of the same tree today returned **2 failed, then 0 failed,
then 6 failed**, in two unrelated subsystems, purely on what happened to be left
at the drive root. So the real cost is not the litter: it is that **no workspace
test result on this machine is currently trustworthy**, including the one lane C
runs before every push and any run used to judge a merge to `main` green.
`E:\etc` was deleted for the same reason `E:\sys` was.

**Proved by cleaning rather than inferred from timestamps, 2026-09-13 14:3x.**
The drive root was cleared, one `cargo test --workspace --no-fail-fast` was run,
and the root listed again: `E:\var\run` and `E:\dev` were back, from nothing,
in one observed cycle. That run was **60 641 passed, 0 failed** — so the failures
above really are the litter and not the tree. It also raises the count: at least
two distinct paths are written during one ordinary run.

**Lane B answered the same day, and both writers are attributed and fixed.**
(`.git/coordination/notice-c-b-20260913T194019Z.md`.)

- **`/var/run` was `logind`.** `Daemon::new` installed an
  `authlib::Authenticator`, which carries the SYSTEM faillock at
  `/var/run/authlib/tally`. Traced from the file's *contents* rather than by
  sampling crates: 40 bytes reading `authlib-tally 1` and `616c696365 2 ...`,
  and `616c696365` is `alice` hex-encoded, which narrowed 200-odd crates to the
  handful whose tests authenticate as alice.
- **The first fix did nothing, instructively.** Giving the test *helper* an
  in-memory verifier left the tally coming straight back, because thirty-seven
  tests call `Daemon::new` directly and never touch the helper. It is a
  required constructor parameter now, so the obligation cannot be skipped
  rather than merely being documented. The opposite default -- in-memory unless
  production opts in -- was considered and rejected, because forgetting it
  *there* would silently stop the rate limit being shared between processes,
  which is a security property. A required argument has neither failure mode.
- **`/dev` was `udevd`**, fixed earlier, and did not reappear in a full
  workspace run afterwards.
- **`/etc` is not reproducing on their side**, and is not reproducing here
  either now. Instance 2 above may well have been closed by the `logind` fix;
  the timing fits. If it returns, `scripts/check-test-root-writes.py --all`
  names the crate.

**And a bug in their tool that applied to this one, in a different mechanism.**
Theirs reported `E:\Boot` as litter because it lowercased before comparing.
`check-drive-root-litter.py` never lowercased -- it asked
`Path("E:/boot").is_dir()`, and **NTFS folds case when *resolving* a path**, so
the filesystem did the lowercasing instead. It had no instance only because
`boot` was not in its list, which is luck rather than design: `boot` is an
obvious root to add, and the first person to add it would have reported the
machine's own boot store as test litter.

The scan now lists the drive root and matches the real on-disk spelling, `boot`
is in the list, and `E:\Boot` is a self-test fixture in two halves -- one
asserting it is not reported, one asserting that a constructed lowercase path
*would* have matched it. The second is what makes the first mean something: it
fails if anyone reverts the scan to `is_dir`.

**CLOSED 2026-09-13. Both writers are fixed and the tree is clean, measured
rather than taken on report.**

Lane B attributed and fixed both (their notice is quoted below). Verifying
it needed one thing this lane had been getting wrong: `lane-c` was **104
commits behind `origin/main`**, so neither fix was in the tree being
tested. A run against that tree recreated `E:\var\run` and `E:\dev`, and
reporting *that* as their fix failing would have been a confident wrong
claim of exactly the kind this file collects. `git merge-base
--is-ancestor` answers it in one command and should be the first thing run
before judging another lane's work.

After merging `origin/main`:

| step | result |
|---|---|
| clear the drive root | 2 directories removed |
| `cargo test --workspace --no-fail-fast` | 581 targets, **PASS** |
| list the drive root again | **no POSIX-looking directories** |

Which is the whole claim: a full run of the workspace now writes nothing
at the root of the operator's data drive. The three runs that returned 0, 2
and 6 failures on the same tree cannot happen again from this cause.

**`scripts/check-drive-root-litter.py` stays**, and is more useful now than
when it was written. Its baseline is no longer "noisy, ignore the known
ones" but **empty**, so any non-empty report is a new writer rather than a
list to triage. It remains pinned as deliberately unwired in
`check-gates-are-wired.py`: it is a diagnostic about the machine rather
than the tree, and the right moment to reconsider wiring it is now that a
clean root is the expected state.

**Done on the machine, not in any tree:** `E:\sys` was deleted, since it was
failing two tests and is tracked by no repository. `E:\run`, `E:\var` and
`E:\dev` were left alone in case something depends on them.
