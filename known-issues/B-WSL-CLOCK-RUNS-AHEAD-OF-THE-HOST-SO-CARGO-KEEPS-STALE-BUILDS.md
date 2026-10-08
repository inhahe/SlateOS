## B-WSL-CLOCK-RUNS-AHEAD-OF-THE-HOST-SO-CARGO-KEEPS-STALE-BUILDS (lane B, 2026-10-08)

**Status:** PARTLY FIXED 2026-10-08 (lane B): `scripts/diff-wsl.sh`, which
builds every `*-diff.sh` harness's subject, allows for the lead and refuses a
stale build. **OPEN** for the clock itself (needs root inside WSL -- the
operator's call, below) and for `scripts/coreutils-check.sh`'s WSL half, the
push gate's Linux clippy and tests, which still take cargo's word.

**In short:** the clock inside WSL runs ahead of Windows' clock, and the gap
grows -- 5.0 s at 05:31, 6.6 s at 05:42, 7.2 s at 06:00 on 2026-10-08, with
the WSL machine up under an hour. Cargo decides what to rebuild by comparing a
source file's time stamp, which Windows sets, with the time it started the
last build, which it reads from WSL's clock. An edit made within that gap
after a build started therefore looks *older* than the build, and cargo keeps
the program it built before the edit. A test run then checks code that is no
longer in the tree, and says it passed.

**How it showed.** `pr-diff.sh` was run with a fix stashed and again with the
fix restored a few seconds later. Both runs compared the same binary -- the
one built from the stashed tree -- so the fix appeared to change nothing, and
a wrong diagnosis (a read error lost under `-m`) followed. `strace` of that
binary showed the file opened onto descriptor 0 with no move off it, which the
source in the tree did not do; `cargo build -v` called the package `Fresh`.

**Measured on a throwaway project** (`cargo new`, build, then rewrite
`src/main.rs` and stamp it 3 s before the binary): cargo keeps the old binary,
and `-Zchecksum-freshness` (or `CARGO_UNSTABLE_CHECKSUM_FRESHNESS=true`) does
not change that on this nightly (cargo 1.100.0, 2026-09-02) -- a backdated edit
is still missed, and a bare `touch` still rebuilds.

**Fixed in `diff-wsl.sh`.** After its build it measures the lead (a file made
in `$root/target`, its stamp read back) and checks each artifact against the
sources its own dependency file lists, late if newer than the artifact's
stamp less the lead and a second; a late source means `cargo clean -p` for the
subject and for the package that owns the source, a rebuild, and a refusal if
that does not cure it. Writing that turned up a second bug: the old check
handed `find` an unquoted list of roots, every one of which contains "visual
studio projects", so it never looked at a single source -- since it was
written. The 2026-08-24 stale library its comments describe may well have been
this lead too.

Verified with `pr-diff.sh`: an old `pr.rs` stamped just before its last
compile began is caught (6.2 s lead), cleaned, rebuilt, and the old binary's
three differences shown; restoring `pr.rs` straight after that run reproduced
the original failure by itself and was caught as well (6.8 s); a run with
nothing changed rebuilds nothing. **Never test before/after with `git
stash`:** the stash list is shared by every worktree, and a pop after a push
that saved nothing applies someone else's entry (it happened here: a
2026-08-21 ctest-fixture stash, which conflicted and so stayed in the list).
Write the old text with `git show REV:PATH > PATH` and restore with `git
checkout -- PATH`.

**Still open:**

* **The clock.** `wsl -u root hwclock -s` sets WSL's clock from the host's
  (one command; the drift comes back, so it would need repeating or a
  periodic job). `wsl --shutdown` restarts the WSL machine with a correct
  clock, but kills every WSL process -- other lanes' harnesses included.
  After either, clear the build caches once: `rm -rf
  ~/.cache/slateos-diff-target` inside WSL (regenerated on demand), because a
  lead that shrinks cannot be allowed for -- artifacts stamped under the old
  lead stay "newer" than edits made just after them.
* **`coreutils-check.sh`'s Linux half** runs `cargo +nightly clippy` and
  `cargo test` in the same cache and trusts cargo's freshness. Under the lead a
  unit edited just after its last check can be passed over, and a lint or a
  test on the edit silently skipped. The proper fix is the same allowance
  before it runs (or the clock fix); it is a shared script (A-Q11), so the
  change belongs with whoever settles that, or goes in additively.
