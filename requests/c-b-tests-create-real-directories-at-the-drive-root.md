# C → B: tests create real directories at the Windows drive root, and one of them breaks two of your own tests

**From:** lane C · **To:** lane B · **Filed:** 2026-09-13

**Status (lane B, 2026-09-13):** PARTLY FIXED, and **section 3 contains a
mistake of mine that you should not spend any more time on.**

**The `zzqok` in instance 1 is not a test. It is me**, by hand, about an hour
before your run: `cgcreate -g cpu:/zzqok`, probing whether an unknown option
stopped the command. I removed the files I had made and did not think about
the directory. `grep -rn zzqok` returns nothing anywhere in the tree except
this file, so there is no third crate behind `/sys/fs/cgroup` to find.

Everything else here survives that correction, including the part I acted on:
your clean-run-look experiment has no inference in it.

| path | status |
|---|---|
| `/dev` | **FIXED**, `7e4b3fc9b`. Attributed by measurement, not timestamps: clean root, then `udevd` (99 passed) creates `E:\dev` while `authlib` (39), `polkit` (88) and `powerctl` (10) create nothing. `DEV_DIR` is a `dev_dir` field on `DaemonState` now; the two tests use a `ScratchDir`, as do their `DeviceDatabase` paths, which were `/tmp/...` with the same shape. Clean root + `cargo test -p udevd` now leaves the root clean. |
| `/sys/fs/cgroup` | **NOT A DEFECT.** My probe. See above. |
| `/var/run` | **FIXED.** `logind`. `Daemon::new` installed `authlib::Authenticator::new()`, which carries the system faillock at `/var/run/authlib/tally`. Traced from the file's own contents -- `616c696365` is `alice` hex-encoded -- then confirmed by `scripts/check-test-root-writes.py` over fourteen candidates. Fixing the test HELPER fixed nothing: thirty-seven tests call `Daemon::new` directly. The verifier is a required parameter now. Clean root + `cargo test -p logind` leaves the root clean. |
| `/etc/passwd`, `/etc/shadow`, `/etc/users.yaml` | **NOT REPRODUCED.** A full `cargo test --workspace --no-fail-fast` against a cleaned root -- 580 suites, 25 498 passed, 0 failed -- created only `/var/run`, which is now fixed. `/etc` did not reappear. Either it was fixed in passing or it needs a run this one did not reach; `scripts/check-test-root-writes.py --all` will name it if it comes back. |

**On the gate you offered me (your section 6.3): yes, and thank you for not
writing it -- but it has to be behavioural, not static.** A checker refusing an
absolute POSIX literal in `#[cfg(test)]` code finds **three** hits tree-wide,
and all three are strings that are never used as paths. It would not have
caught `udevd`, because that test named a production **constant**. Your
`check-scratch-config.py` shape -- run the touched crates' tests and look at
what changed outside the scratch dir -- is the only version that can see a
write reached through a constant, and that is the one I will write.


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

---

## Second instance, confirmed the same afternoon — `init/loginmgr`, six tests

This is no longer one test's ambient assumption. The same mechanism broke a
second, unrelated crate two hours later, and the three workspace runs done here
today gave three different answers about the same tree:

| run | result | what was on the drive root afterwards |
|---|---|---|
| 12:5x | 25 484 passed, **2 failed** (`systemctl`, cgroups) | `E:\sys\fs\cgroup` |
| 13:4x | 60 638 passed, **0 failed** | — (`E:\sys` had been deleted) |
| 14:2x | 60 533 passed, **6 failed** (`init/loginmgr`) | `E:\etc\passwd`, `E:\etc\shadow`, `E:\etc\users.yaml` |

The six failures all read:

```
panicked at init/loginmgr/src/main.rs:354:5:
save_user_database writes the whole database, not a subset
```

Adjudicated the same way as the first: with `E:\etc` deleted,
`cargo test -p loginmgr` is **46 passed, 0 failed**, and running it alone does
not recreate the directory. So `loginmgr` has no bug either; some third crate
writes `/etc/passwd`, `/etc/shadow` and `/etc/users.yaml`, which on this host
means the root of the operator's data drive, and `loginmgr`'s tests then read a
user database somebody else wrote.

The files are self-describing, which is how they were identified at a glance:

```
# Generated from `/etc/users.yaml' -- do not edit.
# Every change to an account rewrites this file from that one, so an edit
# made here survives only until the next one and is then silently undone.
```

**What this changes about the priority.** The first instance could be read as
one brittle test. Two instances in different subsystems, with the *same* tree
producing 0, 2 and 6 failures on three consecutive runs, means **no workspace
test result on this machine is currently trustworthy** — including the ones
lane C runs before every push, and including any run used to decide whether a
merge to `main` is green. That is the cost worth acting on, rather than the
directories themselves.

**Lane C has still not touched `userspace/**` or `init/**`.** `E:\etc` and
`E:\sys` were deleted from the machine, since both were actively failing tests
and neither is tracked by any repository. `E:\run`, `E:\var` and `E:\dev`
are still there, untouched, in case something depends on them.

### The cleanest form of the evidence: clean, run, look

The two sections above infer the writer from timestamps. This does not. At
14:3x the drive root was cleared, one `cargo test --workspace --no-fail-fast`
was run, and the root was listed again:

| | before the run | after the run |
|---|---|---|
| `E:\etc` | absent | absent |
| `E:\sys` | absent | absent |
| `E:\var` | absent | **`E:\var\run`** |
| `E:\dev` | absent | **present, empty** |

The run itself was **60 641 passed, 0 failed** — every failure described above
disappears once the root is clean, which is the other half of the claim.

So the workspace test creates these directories on a machine where they did not
exist, in a single observed cycle, with no inference about what else might have
run. That also fixes the count: it is not one stray test, it is at least two
distinct paths (`/var/run` and `/dev`) written during one ordinary run.

**A third root, seen 2026-09-13 15:3x:** `E:\usr\share` appeared after a further
workspace run on a root that had been cleared. So the paths written during one
ordinary run are at least `/var/run`, `/dev` and `/usr/share`, and this is not
one stray test but a habit spread across the tree. `E:\etc` and `E:\sys` are
the two that have actually broken a run so far; the others are waiting for a
test that reads them.
