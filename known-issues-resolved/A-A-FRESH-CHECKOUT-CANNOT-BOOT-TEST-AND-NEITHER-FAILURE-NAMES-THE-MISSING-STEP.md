## A-A-FRESH-CHECKOUT-CANNOT-BOOT-TEST-AND-NEITHER-FAILURE-NAMES-THE-MISSING-STEP (lane A, 2026-08-18)

**Status: FIXED 2026-08-18 by lane A.** `boot-test.sh` now refuses before the
build, names the missing *step*, and can provision it. See "What actually
shipped" at the end of this entry — in particular the one place the prescribed
fix was deliberately not followed, and the third exit code that turned out to be
necessary.

**Where:** `kernel/src/main.rs:6108-6112`, `kernel/src/proc/spawn.rs` (netstack,
httpget, udpget), `kernel/src/container.rs` (hello), and
`scripts/boot-test.sh:1418`.

**Symptom.** `git worktree add` a detached checkout of this repository and run
`./scripts/boot-test.sh`, and it cannot succeed, in two stages:

1. `cargo build` fails with **15 `include_bytes!` errors**, all of the form
   `couldn't read services/<name>/target/x86_64-unknown-none/release/<name>: No
   such file or directory`. Six distinct binaries are embedded — `init`,
   `hello`, `ticker`, `netstack`, `httpget`, `udpget` — from paths that are
   `target/` directories and therefore gitignored.
2. Supply those, and staging then fails with
   `cp: cannot stat 'limine/BOOTX64.EFI': No such file or directory` —
   `limine/` is a downloaded bootloader tree, also gitignored.

**Why it is worth an entry.** Neither failure is wrong on its own terms; the
defect is that **`boot-test.sh` builds neither prerequisite and reports both as
raw file-not-found**, so the message names the missing *file* and never the
missing *step*. A reader who did not build this tree by hand has no way to get
from "couldn't read .../release/netstack" to "run `cargo build --release
--target x86_64-unknown-none` in each of `services/*`", nor from the `cp`
failure to "fetch limine". The knowledge lives only in the shell history of
whoever set the tree up first.

**How it was found.** Setting up a throwaway worktree (`os-bisect-a`) to bisect
the SHA-256 slowdown against commits from May. Both failures cost a bisect step
each before the cause was clear.

**What the proper fix looks like.** `boot-test.sh` already has a preflight
section (the free-space floor is in it). Add two checks there, in the same
style — refuse *before* building, and name the remedy:

- for each of the six embedded services, if the release binary is absent, print
  the exact `cargo build` invocation that produces it, and offer to run them
  (`--build-services`, matching the existing opt-in `--reclaim-space` shape:
  the run that noticed should not silently do minutes of extra work);
- for `limine/BOOTX64.EFI`, print the fetch command rather than letting `cp`
  fail 1400 lines later, after a full workspace build has already been spent.

The list of six should be derived, not hardcoded twice — `grep -o` over the
`include_bytes!` paths in `kernel/src` is enough and cannot drift out of date
the way a literal list would.

**Workaround until then:** copy `services/*/target/x86_64-unknown-none/release/`
and `limine/` from an already-working worktree. That is also the *better* thing
to do when bisecting, for an unrelated reason: holding the service binaries
fixed makes `kernel/` the only variable across the bisect.

### What actually shipped

`boot-test.sh` does **not** grow its own service loop and its own
`--build-services` flag, as prescribed above. Between the entry being written
and being acted on, `scripts/bootstrap-worktree.sh` had appeared, and it already
provisions all three classes. Adding a second implementation of "which artifacts
must exist" would have re-created, inside `boot-test.sh`, precisely the drift the
last paragraph of the prescription complains about. So the preflight
*delegates*: a new `check_prerequisites()` runs
`bootstrap-worktree.sh --check` before the build and before the boot lock, and
`--bootstrap` provisions and continues in one go (the opt-in shape the entry
asked for, spelled the way `--reclaim-space` is).

Four things were needed to make that delegation safe:

**1. The list of six is derived, as prescribed.** `SERVICES=(init hello ticker
netstack httpget udpget)` and its path-building `artifact_for` are gone,
replaced by a `grep -o` over the `include_bytes!` paths in `kernel/src` feeding
an associative array from service name to artifact path. A service embedded from
two different paths is now an error rather than a coin flip, since provisioning
one and leaving the other missing produces a tree that fails to build while the
checker calls it ready.

**2. A scan that finds nothing exits 2, not 0.** If the `grep` returns no
embedded artifacts at all — the shape a future refactor of how services are
embedded would produce — the script refuses instead of reporting "all
prerequisites present". A checker whose whole job is to say what is missing must
never answer "nothing" when what it means is "I could not tell".

**3. "Cannot build" and "will quietly test less" are different exit codes.** The
entry treats the prerequisites as one class; they are two. A missing service
binary or a missing `limine/` stops the build or the staging — exit 1, refuse. A
missing `rootfs.ext4` stops neither: the boot test runs, reports PASSED, and
silently skips ~58 REAL-glibc Path-Z rungs. That is exit **3**, and `boot-test.sh`
prints a warning and continues. Conflating them forces the caller to pick between
refusing a run that would have been useful and accepting a green result that
measured less than it claims — and `rootfs.ext4` needs WSL to build, so on this
host the refusing branch would have fired constantly.

**4. `--need=<classes>` scopes the question to what the run actually uses.**
`boot-test.sh --no-build --no-stage` boots the already-staged ESP image: it
compiles nothing and copies nothing, so absent service binaries and an absent
`limine/` cannot affect it. An unconditional check would have refused that run —
and a gate that blocks correct runs is a gate that gets switched off. The
preflight therefore derives its scope from `NO_BUILD`/`NO_STAGE`.

**Tests:** `scripts/test-bootstrap-worktree.py`, 37 checks over 13 fixture
worktrees, running the real script (nothing stubbed). Two are the ones no
hand-maintained list could pass: embed a service nobody has heard of, delete its
binary, and require it to be reported `MISSING`. Run against
`git show HEAD:scripts/bootstrap-worktree.sh` the suite produces 17 failures,
so it is not vacuous. All four `check_prerequisites()` branches were exercised in
throwaway worktrees rather than reasoned about, which caught a real bug: the
first version only re-checked after `--bootstrap` if the provisioner exited 0,
and the provisioner exits 1 whenever `rootfs.ext4` alone could not be built —
so a run that had successfully provisioned limine and every service was still
refused. The re-check is now unconditional, for the same reason
`check_free_space` ignores `reclaim-space.py`'s exit status: a run that could not
provision everything may well have provisioned everything that blocks a build.
