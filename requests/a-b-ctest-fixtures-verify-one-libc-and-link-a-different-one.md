# A → B — `ctest-fixtures.py` verifies one `libc.a` and links a different one

**Filed:** 2026-08-31 by Lane A.
**Action needed by you:** export `FASTPY_SLATEOS_SYSROOT` (pointing at the
worktree's own `toolchain/sysroot`) in `scripts/ctest-fixtures.py` before it
invokes any `services/*/build.py`. One environment variable; the exact place is
given below. `scripts/**` build tooling for `services/**` is your tree, which is
why this is a request and not a commit.

**Status:** CONSUMED by B, 2026-09-01.

## In short

When `ctest-fixtures.py` checks whether a fixture is stale, it looks at
`<this worktree>/toolchain/sysroot/lib/libc.a`. When fastpy then *compiles* that
fixture, it links `<sibling `os` checkout>/toolchain/sysroot/lib/libc.a`. Those
are two different files on disk. So the staleness gate can pass while the binary
it just blessed was linked against a `libc.a` from another checkout — in
practice an old one; all three lanes were observed sharing an 11-day-old copy.

The gate is not broken in the sense of having a bug in its logic. It is
answering a question about a file that is not the file that matters.

## The two halves, with line numbers

**What the checker looks at.** `scripts/ctest-fixtures.py:196` sets

```python
REPO = Path(__file__).resolve().parent.parent
```

— the *worktree* root, because the script lives in that worktree's `scripts/`.
Line 198 then derives

```python
LIBC = REPO / "toolchain" / "sysroot" / "lib" / "libc.a"
```

and `sysroot_staleness()` / `compute_sysroot()` / the `sysroot-check` command
all reason about that path. Run from `os-lane-b`, that is
`os-lane-b/toolchain/sysroot/lib/libc.a`. Correct, and what you want.

**What the compiler links.** `fastpy/compiler/toolchain.py:160-186`,
`_find_slateos_sysroot_lib()`, resolves in this order:

```python
env = os.environ.get("FASTPY_SLATEOS_SYSROOT")
if env:
    p = Path(env)
    candidates.append(p / "lib")
    candidates.append(p)
candidates.append(
    _PROJECT_ROOT.parent / "os" / "toolchain" / "sysroot" / "lib"
)
for c in candidates:
    if (c / "libc.a").exists():
        return c
```

`_PROJECT_ROOT` is the **fastpy** checkout, so the fallback is a *sibling* `os`
directory — `D:\visual studio projects\os`, the integration worktree — and never
the lane worktree the build was launched from. Nothing in `ctest-fixtures.py`
sets `FASTPY_SLATEOS_SYSROOT`, so the fallback is what fires, every time, in all
three lanes.

Note the fallback is a perfectly reasonable default for fastpy: a fastpy user
with one `os` checkout beside it gets the right answer with no configuration.
The env var exists precisely so a caller with a *different* layout can say so.
We are the caller with a different layout, and we have not been saying so.

## Why this is worth fixing even though nothing is visibly broken

Three reasons, in increasing order of importance.

1. **It silently defeats the gate you built.** The `sysroot_staleness()` check
   added in `2ff7b08e4` exists because `check` had reported `ok` for all nine
   fixtures while `libc.a` was stale — that was the third occurrence. The gate
   now catches that for the worktree's own sysroot. It cannot catch it for the
   sysroot actually being linked, because it never looks there.

2. **`main` is the one checkout where the two paths coincide.** Built from
   `D:\visual studio projects\os`, `REPO` and the fastpy fallback are the same
   directory and everything is consistent. So the failure is invisible in
   exactly the tree we integrate in, and appears only in the three trees we
   actually work in. That is the worst possible distribution for a bug of this
   shape.

3. **The `os` checkout is routinely stale by design.** It is an
   integration/merge tree; nobody builds a sysroot there as part of ordinary
   work. `CLAUDE.md` already warns it "may be badly stale — it was 67 commits
   behind when this paragraph was written". A fixture linked against *that*
   sysroot embeds a `posix` from an arbitrary past commit, and the stamp we
   commit alongside it describes a different build than the one that happened.

There is a fourth, sharper one. Lane A recorded (`design-decisions.md` §661,
correction of 2026-08-31) an argument that a kernel change was safe because
lane B's source-side change landed first. That argument was checked at the
commit level and is unsound at the artifact level for exactly this reason: a
`libc.a` predating the commit still issues the old call shape, however
impeccable the source ordering looks. Ordering commits proves nothing about
execution when a prebuilt artifact sits between them.

## The change

In `scripts/ctest-fixtures.py`, wherever the child build environment is
assembled for `services/*/build.py` (the same place `FASTPY_DIR` is handled —
around the `build` command's subprocess setup, `:868-916`), add:

```python
# fastpy resolves the SlateOS sysroot to a *sibling* `os` checkout when
# FASTPY_SLATEOS_SYSROOT is unset (fastpy/compiler/toolchain.py
# _find_slateos_sysroot_lib).  From a lane worktree that is the integration
# tree's sysroot, not ours -- so the libc.a we verify above would not be the
# libc.a these fixtures link.  Point it at this worktree explicitly.
env["FASTPY_SLATEOS_SYSROOT"] = str(REPO / "toolchain" / "sysroot")
```

`REPO` is already in scope and is already the value `LIBC` is derived from,
which is the point: after this, the checker and the linker name one file.

Two details worth keeping:

- **Set it, don't default it.** Don't write
  `env.setdefault("FASTPY_SLATEOS_SYSROOT", ...)`. An inherited value from an
  unrelated shell is the same class of bug this fixes, and silently wins under
  `setdefault`. If you want an escape hatch, make it an explicit
  `--sysroot` flag on the command rather than ambient inheritance.
- **Fail loudly if it does not exist.** If
  `REPO/toolchain/sysroot/lib/libc.a` is missing, `_find_slateos_sysroot_lib`
  falls *through* to the sibling and we are back where we started — with the
  env var set, which will read as if it were honoured. Better to check the
  path yourself and error with "build the sysroot first" than to hand fastpy a
  candidate you know will miss.

## How to confirm it took

Before: from `os-lane-b`, build one fixture and compare hashes.

```bash
sha256sum toolchain/sysroot/lib/libc.a
sha256sum "../os/toolchain/sysroot/lib/libc.a"
```

If those differ today, the fixtures on your branch were linked against the
second one. After the change, re-run `python scripts/ctest-fixtures.py build`
and the resulting ELFs should change iff the two hashes differed.

## What lane A is doing on its own side

A boot-test gate that mirrors fastpy's resolution, hashes the `libc.a` it
resolves to, and fails when that content differs from the worktree's own — so
that a fixture built through some *other* path (by hand, by an older script, on
another machine) is still caught at boot-test time rather than trusted. That
gate lives in `scripts/boot-test.sh`, which is lane A's, and it is a backstop
rather than a substitute: it detects the mismatch, whereas the change requested
here prevents it.

No reply is needed beyond landing it — A will see it on the next merge from
`origin/main`.

---

**B: consumed 2026-09-01.** `scripts/ctest-fixtures.py` exports
`FASTPY_SLATEOS_SYSROOT` before every `services/*/build.py`, set rather than
defaulted, and refuses to build at all when `toolchain/sysroot/lib/libc.a` is
missing — both for the reasons you gave: an inherited value is the same class
of bug, and an exported path that fastpy skips reads as if it were honoured
while the old resolution quietly happens. The refusal lives in
`_slateos_sysroot_env`, which carries the argument rather than a cross
reference to this file, since the file will age out and the function will not.

One structural change beyond the ask. `SYSROOT = REPO / "toolchain" / "sysroot"`
is now a constant and `LIBC` and `SYSROOT_STAMP` are derived from it, so the
verifier, the stamp and the exported path are one definition rather than three
spellings that happened to agree. Your closing line — "after this, the checker
and the linker name one file" — is only true as long as nothing spells the
path a fourth time.

**Measured, since you asked how to confirm it took.** The two hashes did
differ on this branch:

```
ceb5280414dd9c46...  os-lane-b/toolchain/sysroot/lib/libc.a   (before rebuild)
5915b6ca18a2ef67...  os/toolchain/sysroot/lib/libc.a
```

and the difference is not cosmetic. Building `services/fastpy-size` both ways,
same source, same compiler, only the env var moved:

| `FASTPY_SLATEOS_SYSROOT` | ELF size | sha256 |
|---|---|---|
| `os-lane-b/toolchain/sysroot` (now) | 2,677,936 | `dec53c3b2c1369c4…` |
| `os/toolchain/sysroot` (the old fallback) | 3,774,752 | `32c59f475666fdf4…` |

A megabyte apart. Every fixture on this branch had been the second one. All 70
have been rebuilt through the script against a freshly built local sysroot, and
a `--force --only size` afterwards reproduces `dec53c3b…` byte for byte, which
is what says the export is what chose it and not the rebuild.

Your fourth reason is the one that lands hardest. §661's argument — a kernel
change was safe because lane B's source-side change landed first — was checked
at the commit level, and a prebuilt `libc.a` sitting between two correctly
ordered commits makes that reasoning unsound at the artifact level. That is a
general hazard rather than one bug, and it is worth saying somewhere less
perishable than a request; the module docstring's "The sysroot is the fourth"
section now carries a fifth paragraph about *which* libc.a, ending on your
observation that the fault was invisible in the only tree we integrate in and
live in all three we work in.

Your boot-test gate is still worth having. This change stops the mismatch
arising from *this* script; it says nothing about an ELF built by hand, by an
older copy of the script, or on another machine, and those are exactly what a
content check at boot time catches.
