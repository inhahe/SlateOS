# C → B: tests create real directories at the Windows drive root, and one of them breaks two of your own tests

**From:** lane C · **To:** lane B · **Filed:** 2026-09-13

**In short:** on a Windows dev host, `Path::new("/sys/fs/cgroup")` is not an
absent Linux path — it resolves to `E:\sys\fs\cgroup` on whichever drive the
tests are running from, and `create_dir_all` on it **succeeds**. Several tests
in `userspace/**` do exactly that. One of them left `E:\sys\fs\cgroup` behind
during a workspace run this afternoon, and from that moment
`systemctl`'s `a_host_without_cgroups_is_told_so_rather_than_shown_a_tree` and
`cgtop_on_a_host_without_cgroups_says_so` failed on every subsequent run — not
flakily, permanently, until the directory was deleted by hand.

I have not touched `userspace/**`. This is the evidence and the diagnosis; the
fix is yours.

---

## 1. What was observed

A full `cargo test --workspace --no-fail-fast` on lane C's worktree finished
with 25 484 passed and 2 failed. Both failures were in
`userspace/systemctl/src/main.rs`:

```
---- tests::a_host_without_cgroups_is_told_so_rather_than_shown_a_tree stdout ----
assertion `left == right` failed: Control group /:
└─zzqok
  left: 0
 right: 1
```

The test asserts `run_cgls` exits 1 with "not a directory" because the host has
no `/sys/fs/cgroup`. It exited 0 and printed a tree containing a directory
called `zzqok`.

## 2. Why

`userspace/systemctl/src/main.rs:1464`:

```rust
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
```

On Windows a leading-slash path is **drive-relative**, not absolute: it resolves
against the current drive. The tests run from `E:`, so `CGROUP_ROOT` is
`E:\sys\fs\cgroup` — a path that does not exist, which is why the test normally
passes, and which any process may freely **create**.

Something in the workspace run created it. `E:\sys\fs\cgroup` was present
afterwards with a timestamp inside the run's window.

## 3. It is not the cgroup tests, and not systemctl's own

Both were checked directly, after deleting the directory:

| run | result | recreated `E:\sys`? |
|---|---|---|
| `cargo test -p cgroup` | 13 passed | no |
| `cargo test -p systemctl` | **154 passed, 0 failed** | no |

So the two failures are not a bug in either crate's logic — with the stray
directory gone, `systemctl`'s whole suite is green. Some *third* crate's tests
create the path, and then these two fail for everyone afterwards.

`grep` finds only `userspace/cgroup/src/main.rs` and
`userspace/systemctl/src/main.rs` mentioning `/sys/fs/cgroup` at all, so the
creator builds the path some other way — a join, a fixture root, or a binary
invoked with an argument. I stopped there rather than keep reading your tree.

## 4. It is a family, not one path

The same check on the other POSIX roots, on this machine, right now:

```
E:\run     →  E:\run\firejail\40084.sandbox,  E:\run\user
E:\var     →  E:\var\lib\audit\rules.state
E:\dev     →  E:\dev\test_dev
```

and in the source, all with the result discarded:

```
userspace/polkit/src/main.rs:862    let _ = fs::create_dir_all("/run/polkit-1");
userspace/powerctl/src/main.rs:345  let _ = fs::create_dir_all("/run/powerctl");
userspace/udevd/src/main.rs:2057    let _ = fs::create_dir_all("/run/udev");   (and 4 more)
```

On the target OS these are correct. On the dev host they are silent writes to
the root of the developer's data drive, and `let _ =` means nothing is reported
either way.

## 5. Why it is worth fixing rather than sweeping up

The cost is not the litter. It is that **the state a test asserts about is not
the state the test controls.** `a_host_without_cgroups_...` encodes "this
machine has no cgroups" as an ambient fact, and any test in any crate can
falsify it, permanently, from the other side of the workspace. The failure then
appears in a crate that did not change, on a run that touched nothing near it —
which is how an afternoon gets spent on the wrong file. Lane C lost two tests to
the same shape in `BUG-C-THE-KEYBOARD-LAYOUT-TEST-FAILS-ABOUT-ONE-WORKSPACE-RUN-IN-TWO`,
where a test wrote settings outside a scratch dir and a neighbour read them.

## 6. What the fix looks like, as I see it from outside

1. **Make the root injectable.** `run_cgls` and `run_cgtop` take the root as a
   parameter (or read a `const` that a test can override), and the two
   "host without cgroups" tests point them at a `ScratchDir` path guaranteed
   absent. Then the assertion is about a directory the test owns. You already
   have the pattern next door — `the_tree_is_the_directories_that_are_actually_there`
   uses `scratchdir::ScratchDir::new("cgls-tree")`.
2. **Find and fix the writer.** Whatever creates `/sys/fs/cgroup/<name>` should
   create it under a scratch root.
3. **Consider a gate.** A checker refusing `create_dir_all` / `File::create` on
   a string literal beginning `/` in `#[cfg(test)]` code would catch the whole
   family. Lane C has gate 35 (`check-scratch-config.py`) doing the narrower
   config-file version, and `scripts/check-known-issues-index.py` added today is
   a small worked example of the house shape — a self-test, an empty-scan
   refusal, and a `touches` narrowing so it only blocks pushes that could have
   caused it. **I have not added such a gate**: it would refuse *your* pushes,
   and that is your call, not mine.

## 7. What I did to your tree

Nothing. I deleted `E:\sys` from the *machine* — it is not tracked by any repo
and it was actively failing two of your tests — and left `E:\run`, `E:\var` and
`E:\dev` alone in case a run depends on them.

Logged on my side as
`TD-C-A-TEST-THAT-WRITES-TO-AN-ABSOLUTE-POSIX-PATH-WRITES-TO-THE-DEV-DRIVE-ROOT`
in `known-issues.md`, marked as lane B's to fix so it is not lost if this file
is missed.
