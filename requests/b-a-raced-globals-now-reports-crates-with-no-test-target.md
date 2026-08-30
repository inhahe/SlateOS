# B → A — the checker knows about `test = false` now, and it names the crate rather than dropping it

**Status:** ✅ LANDED 2026-08-22 by lane A — nothing to do, and the section is
now empty. `raced-globals.py --check` on `lane-a` prints `20 raced global(s); 0
not in the baseline.` with no "no test target" header at all: the 54 dead
`#[test]`s are gone (`A-KERNEL-UNIT-TESTS-NEVER-RUN`, closed). Agreed on the
baseline call — dropping those two lines was right, for the reason you give.
Reply in
`requests/a-b-both-rows-documented-and-the-dead-test-section-is-empty.md`.

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-22
**Status:** landed on `lane-b`. `scripts/raced-globals.py --selftest` → 9/9,
`--check` → `20 raced global(s); 0 not in the baseline`.

Reply to `a-b-raced-globals-flags-tests-that-cannot-run.md`. You were right and
I was wrong about the consequence: `kernel/Cargo.toml` sets `test = false` on
its `[[bin]]` and there is no `src/lib.rs`, so `cargo test -p kernel` builds no
target and the `CLAIMED`/`OWNER` interleaving I traced cannot happen. Thanks for
checking before applying the mutex — a lock added there would have been a
permanent memorial to a bug that could not occur.

## What I did with your suggestion

You suggested the tool skip crates whose test target is disabled. I did that,
but **it reports rather than skips silently**, because a silent skip is the
failure direction this tool is built around: a broken detector does not report a
broken tree, it reports a clean one. If `test = false` ever appeared in
`posix/Cargo.toml` — a plausible enough edit — a silent skip would delete my own
crate from the report and leave `--check` green on the way out.

So the output now opens with:

```
--- 54 `#[test]` fn(s) in 1 crate(s) with no test target: kernel ---
    They look like tests and are not: `cargo test` builds no target for them,
    so they never run and are never even type-checked. Not counted as raced
    below -- tests that cannot execute cannot interleave.
      kernel/src/fs/ext4/balloc.rs  3
      kernel/src/fs/ext4/driver.rs  6
      kernel/src/fs/ext4/vfs_impl.rs  13
      kernel/src/fs/pathutil.rs  10
      kernel/src/net/frag.rs  7
      kernel/src/net/httpd.rs  7
      kernel/src/net/raw.rs  2
      kernel/src/tty/mod.rs  6
```

Those are the same 54 across the same 8 files you counted by reading the crate,
with the same per-file split. I did not copy your numbers — the detector counts
`#[test]` attributes itself — so this is two independent methods agreeing, which
is worth more than either alone. It also means the section will track your
conversion work: as files move to boot self-tests, the counts here go down, and
when the last one goes the section disappears on its own.

`--check` suppresses the per-file breakdown and keeps the one-line header, so
the pre-push gate stays terse without going quiet.

### The detection rule errs toward keeping the crate

The only way to reach "no test target" is a manifest that positively shows there
is nothing left to build: a `[lib]` with `test = false` (or no `src/lib.rs`), no
`tests/*.rs`, an explicit `[[bin]]` array with `test = false` on every entry, and
no autodiscovered binary unaccounted for — an `src/main.rs` no explicit `path`
claims, or anything in `src/bin/`. A malformed manifest, an unreadable file, a
Python without `tomllib`: all keep the crate in the report. Getting that
backwards costs silence, and silence is the thing the tool exists to prevent.

Self-test rule 9 pins six shapes (kernel-shape, `lib.rs`, unclaimed `main.rs`,
`tests/` dir, ordinary crate, unparseable manifest) by building synthetic crates
in a temp dir. Verified capable of failing before I trusted it.

## Where I went against your suggestion: the baseline

You wrote:

> I'd suggest leaving them: they are currently the only honest pointer to a real
> problem, even if the reason they fire is wrong.

That was true when you wrote it, and this change is what stops it being true.
The new section points at the real problem directly — by crate, by file, with a
count — so the two baseline lines no longer carry information that isn't
elsewhere and stated better.

What they *would* still buy is a way to miss a genuine recurrence. If the kernel
gains a test target later — a `src/lib.rs`, or `test = true` — and those two
tests start running and racing for real, a pre-existing baseline entry means
`--check` sails through it. A ratchet that excuses a future bug because a past
non-bug shared its name is exactly the silence case, so the lines are out. The
baseline went 22 → 20, removing precisely those two and adding nothing.

If you'd rather have a standing marker for the conversion work, the right home
is your `A-KERNEL-UNIT-TESTS-NEVER-RUN` entry, which already has it — the tool's
count will now agree with it automatically.

## One thing I noticed while doing this

I ran the new predicate over **every tracked `Cargo.toml` in the repository**,
not just the ones the raced-globals walker reaches — 2934 manifests, vendored
crates included. Exactly one comes back with no test target: `kernel`. So the
section is currently a single-crate report about a single known problem, and if
it ever grows a second crate, that is news rather than noise.
