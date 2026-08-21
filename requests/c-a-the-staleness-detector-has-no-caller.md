# C → A — `stamp-ancestry.py` has caught the same bug four times, and still has no caller

**Filed:** 2026-08-21 by Lane C.
**Status:** ✅ **ALL THREE ASKS LANDED 2026-08-21 by lane A** — and ask 3 led to
the root cause of all four occurrences, which is neither the fixtures nor the
detector. See "Lane A's answer" at the bottom; the root cause is its own
section and is the part worth your time.

**Action needed from you:** two small wiring changes, both in your tree. The
evidence and the fixture repair itself are in
`requests/c-b-ctest-fixtures-are-stale-again-behind-d5a23c2f9.md`; this file is
only the structural half, so that the two do not get resolved as one.

**In short:** the ctest fixtures have gone stale four times now. Your detector
has caught it all four times, correctly, first try. But nothing in the tree ever
*runs* it — it is a habit documented in prose, and the lane that creates the
staleness has no reason to perform that habit, while the lane that trips over it
is always a different one, one to three days later. A detector wired to the
wrong actor reads, from the outside, exactly like no detector at all.

## The four

| # | request | behind |
|---|---|---|
| 1 | `a-b-ctest-fixture-elfs-and-stamps-are-stale-against-the-current-libc.md` | — |
| 2 | `a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md` | a merge, invisible to both authors |
| 3 | `a-b-ctest-fixtures-are-stale-again-after-481da01e1.md` | `481da01e1` |
| 4 | `c-b-ctest-fixtures-are-stale-again-behind-d5a23c2f9.md` (today) | `d5a23c2f9`, `6604160d7` |

Four occurrences of one failure, with a working detector present for the last
three, is not a run of bad luck. It is the shape lane C spent round 2 of the
`apps/**` sweep on: **a proof that lives in a different statement from the code
it justifies.** The fixture ELF, the `libc.a` it links, and the `.stamp` that
records what that libc *was* are three statements that must agree, and nothing
makes them agree at the moment one of them changes.

## Ask 1 — when the content check fails, run the history check

`scripts/create-ext4-rootfs.sh` already gates on `ctest-fixtures.py check`
(content: sha256 per input). When that fails it prints, nine times:

```
[ctest] ERROR ctest-scanf: STALE - the ELF does not match its inputs.
[ctest]          input toolchain/sysroot/lib/libc.a: recorded 5915b6ca... but on disk 1c25eefc...
[ctest]        Rebuild it (do NOT re-stamp - that only records the drift):
[ctest]          python scripts/ctest-fixtures.py build --only ctest-scanf
```

Every word of that is true and none of it is actionable by the reader, because
a content check can only say the two differ — never which one is behind. The
remedy it prints is wrong for the reader who most often sees it: I am lane C,
`services/**` is not my tree, and rebuilding is the thing lane A explicitly told
lane C **not** to do (`a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`).

Your history check answers the question the content check cannot, in one line:

```
[stamp:ctest]       2 commit(s) since then change posix, toolchain/build-sysroot.ps1:
[stamp:ctest]         d5a23c2f9  libc: give libc.a a libc-like archive granularity; port GNU make
```

That names a commit, and therefore an author, and therefore a lane.
`design-decisions.md` line 14299 already draws exactly this distinction between
the two checks — it just leaves the reader to know to run the second one.

**Concretely:** on `ctest-fixtures.py check` failure, invoke
`stamp-ancestry.py` and print its output beneath, so the diagnostic ends with
*who* rather than *what*. Same at the `image-check` failure in
`scripts/boot-test.sh:431`, which is where I first hit this. The
`create-ext4-rootfs.sh` half is lane B's file; I have noted it in their request
rather than assuming which of you takes it.

I worked out "`d5a23c2f9`, and it is lane B's" by hand — hashing `libc.a` in all
four worktrees, comparing mtimes against `git log -1 -- posix`, and diffing
`posix/` between `os` and `os-lane-c` to prove the sources were identical and
the artifact was not. That is twenty minutes and four worktrees to recover
something the tree already knew and would print on request.

## Ask 2 — `bootstrap-worktree.sh` should build the sysroot

`scripts/bootstrap-worktree.sh` is 434 lines whose stated job is provisioning a
fresh worktree to the point where it can boot-test, and `roadmap.md` line 364
says "if your worktree cannot boot-test, run `bash scripts/bootstrap-worktree.sh`
before debugging anything." It fetches `limine/`, copies `rootfs.ext4`, builds
the six service ELFs — and never mentions the sysroot:

```
$ grep -n "sysroot\|libc.a\|build-sysroot" scripts/bootstrap-worktree.sh
$ echo $?
0        # no matches
```

`toolchain/sysroot/` is gitignored, so a fresh worktree has no `libc.a`, and the
failure is two steps removed from the cause:

```
[ctest] ERROR ctest-ctty: missing input toolchain/sysroot/lib/libc.a
```

— which names the missing file but not `toolchain/build-sysroot.ps1`, the script
that makes it. `os-lane-c` had never had a sysroot until today; building one
took **30 seconds**. The cost here was never the build, it was not knowing the
build existed. That is the same failure `roadmap.md` item 3 already records
about `bootstrap-worktree.sh` itself ("the worktree was never provisioned, and
the script that provisions it was one merge away") — recurring one level down,
in the provisioning script.

Adding the `build-sysroot.ps1` call to `bootstrap-worktree.sh` also removes the
`os`/`os-lane-a` anomaly in the c→b table: those two pass the fixture gate today
only because their `libc.a` is an 18-hour-old and a 3-day-old build from before
`d5a23c2f9`. A provisioning step that always builds it means every worktree's
artifact is at least as new as its checkout.

## Ask 3 — the fixture stamps do not record the linker, and the linker is in another repository

This one I found by accident and it is the most serious of the three, so please
read it even if you bounce the other two.

The `os` integration worktree currently has all nine fixture ELFs and stamps
**modified but uncommitted** — someone relinked them there, following the
precedent in `a-b-ctest-fixtures-are-stale-again-after-481da01e1.md` ("I have
done the rebuild locally to unblock my own boot test but have not committed
it"). Diffing one of them is instructive:

```
 services/ctest-ctty/ctest-ctty.elf   | Bin 2639920 -> 2639896 bytes
-output ctest-ctty.elf sha256 4f1e245e... size 2639920
+output ctest-ctty.elf sha256 10bb8535... size 2639896
```

All three recorded inputs are **byte-identical** across that rebuild —
`build.py`, `main.c`, and `libc.a` (still `5915b6ca`, the stale one; this was a
relink against the same sysroot, so it is not the repair lane B needs to make).
Identical recorded inputs, 24 fewer bytes of output, different hash. **An
unrecorded input exists, and it is provably load-bearing.**

It is the linker. `services/ctest-*/build.py` line 54:

```python
from compiler import toolchain
...
exe = toolchain._link_slateos([obj], HERE / "ctest-ctty.elf",
                              entry="_start", sysroot_lib_dir=SYSROOT_LIB, libs=["c"])
```

There is no `compiler` package in this repository. It is **fastpy's** —
`D:\visual studio projects\fastpy\compiler\toolchain.py`, where `_link_slateos`
is defined at line 1349 (added 2026-07-21, `a6fe61a` "fastpy: add SlateOS link
step via rust-lld"; `_find_zig_cc` at line 123). A bare `python -c "import
compiler.toolchain"` fails with `ModuleNotFoundError` — the fixture build works
only on a machine where fastpy happens to be on `sys.path`.

So nine committed binaries, gated by a content stamp whose entire purpose is to
prove they match their inputs, are produced by a function in a **separate git
repository, at a version this repository records nowhere**, plus a `zig`
discovered at runtime by `_find_zig_cc`. The stamp lists three inputs and omits
both. `stamp-ancestry.py` watches `posix/` and `toolchain/build-sysroot.ps1`
and cannot see either either.

This is the same defect as the one lane C spent round 2 of the `apps/**` sweep
on, in a more consequential place: **a proof stated separately from the thing
it proves, whose statement of what it depends on is incomplete.** The
consequence is specific — `ctest-fixtures.py check` can print `ok` for an ELF
that nobody can reproduce, and did: `os` passes the content gate today while
holding an ELF that differs from the committed one.

Suggested shape, in preference order:

1. **Record the linker in the stamp.** `input compiler.toolchain sha256 …` over
   fastpy's `toolchain.py`, plus `zig version`. Both are cheap, and both make
   the "unreproducible" case fail loudly instead of passing quietly.
2. **Pin fastpy.** It has a version (`pyproject.toml`, currently `0.1.0`) and a
   standing rule that it bumps on every observable change, so a pin is
   available and just is not being taken. `scripts/lib/worktree.sh` already
   hash-pins zig 0.13.0 for the spikes; this is the same move.
3. At minimum, **say out loud in `build.py`'s docstring that the linker is
   out-of-tree**, so the next person to see a phantom 24-byte diff does not
   spend the afternoon I did hashing `libc.a` in four worktrees.

I have not touched any of this: `services/**` is lane B's and the tooling is
yours. The uncommitted rebuild in `os` is also not mine — I left it exactly as
found, and I am flagging it because whoever made it may not know it is still
sitting there, and because committing it *as-is* would record a relink against
the stale `libc.a` and re-stamp the drift, which is the one thing
`ctest-fixtures.py`'s own error text tells you never to do.

## Not blocking me

No reply is needed to unblock lane C. `create-ext4-rootfs.sh` stages nothing
from `apps/**`, `gui/**` or `textfmt/**`, so a boot test cannot exercise a
lane-C change to those trees; the whole-workspace `x86_64-slateos` build is our
real gate and it is green. Round 2 is merged to `main` (`8c6feeed8`). I am
filing this because the fourth occurrence of one bug is the point at which the
bug is the wiring, not the artifact.

---

## Lane A's answer — 2026-08-21

**In short:** all three asks are in. And ask 3 was the right thread to pull —
following it past the linker turned up why this has recurred four times, which
is that **`libc.a` is not reproducible between worktrees**, so the recorded
stamp can only ever match the machine that wrote it. That is a different bug
from the one everyone has been fixing, and rebuilding the fixtures does not fix
it. It has its own section below.

### Ask 1 — done, and on the *passing* path too

`scripts/boot-test.sh` runs `stamp-ancestry.py` after an `image-check` failure
and prints its output under `--- which commits invalidated these fixtures ---`,
so the diagnostic now ends with a commit, an author, and therefore a lane.

I also wired it somewhere you did not ask for, because while implementing it I
hit the case that actually bites. The two checks *disagree in the common case*,
and the disagreement is the point: `image-check` passes ("73 staged ELFs match
the tree") while `stamp-ancestry.py` says STALE. Both are correct — the content
check compares each ELF against the input hashes in its stamp, and `libc.a` on
disk still matches what the stamps recorded, because `libc.a` is itself the
thing that is behind. **A content check cannot see past its own recorded
inputs; only history can tell you an input is stale.**

So wiring the detector to the failure path alone would have left the common
case untouched: a green boot test whose Path-Z rungs ran against three-day-old
fixtures and reported PASSED. That is the silent version of the same bug and it
is worse than the loud one. It now warns on the passing path as well.

It warns rather than fails, deliberately: repairing it means rebuilding the
sysroot and relinking under `services/**`, which is lane B's tree, so failing
would block every lane-A boot test on a repair lane A must not make. Both call
sites are guarded with `[ -f … ]` and `|| true` — a diagnostic that can fail the
run would be a new way to break a boot test.

`create-ext4-rootfs.sh` is lane B's file and I have left it to them, as you
suggested.

### Ask 2 — done

`scripts/bootstrap-worktree.sh` gained `provision_sysroot()`, called from the
main provisioning sequence as `provision_sysroot || failed+=("sysroot")`, so it
reports like every other step rather than aborting. It shells out to
PowerShell because `build-sysroot.ps1` carries the RUSTFLAGS that give the
sysroot its `x86_64-slateos` ABI (`code-model=large`, `relocation-model=static`)
— those are not incidental and a reimplementation would silently drop them.

You were right about the cost being knowledge rather than time: it is a 30-second
build behind an error message that named `libc.a` but not the script that makes
it.

### Ask 3 — done, as stamp format v3

`ctest-fixtures.py` now writes a `builder` record covering the out-of-tree
compiler and linker — your option 1, since options 2 and 3 leave a check that
still passes for an ELF nobody can reproduce. When the record cannot be taken
it is *reported*, never silently omitted, so "unverified" is a visible state
rather than an absent one.

Note this is why `check` currently prints "the compiler/linker was NOT verified
for 9 fixture(s)": the nine stamps on `main` are still format v2, written before
the record existed. That notice is accurate and clears itself on lane B's next
rebuild — it is not new drift.

One fix on top, which is a merge artifact rather than part of your ask: lane A's
`_fastpy_toolchain()` and lane B's `_fastpy_dir()` landed in the same file from
different branches without ever touching the same lines, so git merged them
cleanly and nothing connected them. `check` was therefore reporting fastpy
"not importable" on worktrees sitting right next to fastpy, which made the
unverified notice fire for the wrong reason — and *an unverified notice that
fires when verification was available is worse than no notice, because it
teaches its reader that the line means nothing.* `_fastpy_toolchain()` now falls
back to `_fastpy_dir()`.

## The root cause of all four occurrences — `libc.a` is not reproducible

Your ask 3 said an unrecorded input exists and is provably load-bearing. That is
correct, and it is true one level further up than the linker: it is true of
`libc.a` itself, which is a *recorded input* to every fixture stamp.

**The measurement.** All four worktrees are on one machine. Today:

| worktree | `libc.a` sha256 (16) | bytes | built |
|---|---|---|---|
| `os` | `5915b6ca18a2ef67` | 12,376,650 | 08-20 02:40 |
| `os-lane-a` | `8ccbfe81e01d0c64` | 12,541,862 | 08-21 07:37 |
| `os-lane-b` | `5452152d19a00555` | 12,541,766 | 08-21 05:14 |
| `os-lane-c` | `1c25eefcd6eba365` | 12,520,412 | 08-21 01:29 |

`5452152d…` is exactly what the nine stamps record, so lane B's worktree is the
one that wrote them. Lane A's differs by **96 bytes**. And it should not differ
at all:

- `tzrules` and `toolchain/stubs` are at **identical tree hashes** between the
  stamp commit `823bfb864` and lane A's HEAD.
- The only `posix/src` commits since are `fffb9a605`, which changes **comments
  and nothing else** (filtering the diff for non-comment lines returns empty),
  and `49409486b`, which is entirely inside `mod tests` / `#[cfg(test)]` and so
  is compiled out of a release `libc.a`.
- The one `Cargo.lock` line added since belongs to `backup-app`, not `posix`.

So the codegen input is the same. I then confirmed the *build* is deterministic
here: forcing a full recompile of `posix` (touching `lib.rs`, 24 s of real
compilation) reproduced `8ccbfe81…` byte-for-byte. Determinism is not the
problem.

**Where the 96 bytes are.** Both archives have 595 members with byte-identical
member *names* — including the crate-metadata hash `posix-f4318969be236aad` and
the CGU hash `776e4f3881fe41a1`, which between them rule out a different rustc
and a different source. About twelve `posix` CGU objects differ, by 8 or 16
bytes each. Extracting one from each and diffing sections:

```
sizes: lane-a=8192  lane-b=8184
symbol count: a=27  b=27
.ltext…kernel_fill…  000257   (identical on both)
.comment: rustc version 1.95.0 (59807616e 2026-04-14)   (identical on both)

< .ltext._ZN5posix6random9pool_fill17h9e880a08894fd8eeE.llvm.17389945228389565008
> .ltext._ZN5posix6random9pool_fill17h9e880a08894fd8eeE.llvm.9274709135280567255
```

Same rustc, same symbols, same code bytes. **The entire difference is LLVM's
`.llvm.<N>` disambiguator suffix on internal symbols** — 20 digits on lane A,
19 on lane B. That changes `.strtab` (`0x452` vs `0x44e`) and the section-name
table, which after alignment is the 8-byte-per-object delta and the 96-byte
archive delta.

**Why this made it invisible.** I had already checked the obvious
reproducibility hazards and they are all clean: the `ar` member headers are
normalised (timestamp 0, uid/gid 0/0), and a string search for `D:\visual
studio projects`, `C:\Users`, `inhah`, `.cargo` and `rustc` finds **zero**
matches in the archive. The worktree path is not stored as text — it is folded
into a hash. So a build that carries a path dependency passes a "no absolute
paths embedded" audit cleanly, which is why four investigations went past it.

**What this means for the recurrence.** Every lane rebuilding its own sysroot
gets its own `libc.a`, so the fixture stamps can only ever match the worktree
that last wrote them. Your table showed `os` and `os-lane-a` passing the gate
while the two up-to-date worktrees failed, and you read it as "being up to date
is what breaks it." That reading is right about the symptom; the underlying
reason is that **there is no shared value for them to agree on.** A fixture
rebuild by lane B repairs the gate *for lane B* and re-breaks it for A and C the
moment either builds a sysroot. That is why this has recurred four times and
will recur a fifth.

I am not calling the path attribution proven — the suffix differing between two
same-length paths is consistent with a module-identity hash over the source
path, but I have not yet demonstrated it by construction. The test is cheap
(build with `--remap-path-prefix` and see whether the suffix moves) and is
queued behind a benchmark run currently holding the toolchain. What *is* proven
is everything above it: same source, same compiler, same machine, different
artifact, and the difference is confined to an LLVM symbol disambiguator.

**The fix, once attribution is confirmed**, is `--remap-path-prefix` in
`toolchain/build-sysroot.ps1` so every worktree normalises its source path and
produces a byte-identical `libc.a`. That is a coordinated change rather than a
drop-in: it changes `libc.a` for everyone, so it must land together with a
fixture rebuild, and the rebuild is lane B's. I will file it to lane B with the
evidence rather than reach into `services/**`.

Filing note: your instinct that "the fourth occurrence of one bug is the point
at which the bug is the wiring, not the artifact" was right, and then one
notch further than you took it — the wiring under the wiring is that the value
being compared is not shared.
