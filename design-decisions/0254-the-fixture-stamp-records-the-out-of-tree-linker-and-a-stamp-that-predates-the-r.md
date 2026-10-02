## §254 — The fixture stamp records the out-of-tree linker, and a stamp that predates the record is a note rather than a failure

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Each `services/ctest-*` test program is checked into git twice —
once as C source, once as the compiled binary the boot test runs — and a
"stamp" file records a fingerprint of everything that went into the binary, so
that editing the source without rebuilding the binary is caught. The stamp
listed only files from this repository. But the compiler and the linker that
actually produce the binary live in a *different* project (fastpy) that is
being developed at the same time, and nothing recorded which version of them
was used. Change the linker and every committed binary quietly stops matching
its sources, while every stamp still reports "ok" — because the thing that
moved was never on the list. The stamp now records those two tools as well. The
awkward part is that adding a field invalidates all nine existing stamps at
once, and repairing them means rebuilding binaries that belong to a different
lane; so an old-format stamp prints a note and keeps going, instead of failing
the build for everyone.

### The omission

`services/ctest-*/build.py` does two things: it runs `zig cc` to compile
`main.c` to an object, then calls `compiler.toolchain._link_slateos` — a
function in fastpy — to link that object against the sysroot `libc.a`. That
function chooses the entry symbol, the lld flavor, `-static`,
`--no-dynamic-linker`, and the archive search order. `_find_zig_cc` and
`_find_rust_lld` choose the binaries.

None of that was in the stamp, which listed `build.py`, `main.c` and `libc.a`.
`build.py` was included because it "*is* the compile and link flags" — true of
the `zig cc` half, and false of the link half, which is one function call whose
body is in another repository.

Lane C named the shape exactly (`requests/c-a-the-staleness-detector-has-no-
caller.md`): *a proof that lives in a different statement from the code it
justifies*. The stamp is not merely stated separately from the artifact; it is
stated in terms of a set of inputs it also chose, and nothing audits that set.

### The decision

Stamp format **v3** adds three `builder` lines:

```
builder compiler.toolchain sha256 <sha256 of fastpy's toolchain.py, CRLF-folded>
builder rust-lld version <self-reported version, whitespace-collapsed>
builder zig version <self-reported version, whitespace-collapsed>
```

Three sub-decisions inside that, each with a real alternative:

**A hash of `toolchain.py`, not fastpy's version.** Lane C ranked a version pin
second. fastpy's standing rule is that its version bumps on every observable
change, which makes it a *stable* identifier — but the rule is prose, enforced
by whoever remembers it, not by the build. A pin reading `0.1.0` proves that
somebody bumped, not that the file is unchanged. For a stamp whose entire
purpose is "prove this artifact matches its inputs", a hash is the honest
answer and a version string is a promise about a hash. *Cost, accepted
explicitly:* the gate goes STALE whenever fastpy edits `toolchain.py` for any
reason, including reasons nowhere near `_link_slateos`, and fastpy is under
active development so that will not be rare. That is correct anyway — if the
recipe changed, the committed ELF genuinely is no longer reproducible from
current inputs, and "rebuild" genuinely is the right answer.

**Versions, not hashes, for the two binaries.** `rust-lld` and `zig` are
installed rather than committed, tens of megabytes each, and their bytes differ
between machines running the same release. Hashing them would report drift on
every second worktree — which is the `sha256_text` mistake (§ the v2 CRLF
finding) repeated at toolchain scale, and a false STALE here blocks the image
build on every machine except the one that wrote the stamp. The self-reported
version is the coarsest identifier that still moves when behaviour does.

**The `version` field states what the stamp contains, not what wrote it.**
`check` and `stamp` are documented to work on a machine with no zig and no
fastpy — that is why the ELFs are tracked in git at all — so the record cannot
always be taken. Rather than write a v3 stamp with the `builder` lines missing,
`compute()` writes `version 2` when it could not look. A v3 stamp therefore
*always* carries the linker identity. The alternative (v3 with optional fields)
makes an absent `builder` line ambiguous between "old format" and "written
somewhere that could not look" — and an ambiguous field that defaults to "fine"
is the silent pass this entire mechanism exists to remove.

### Why an old stamp is a note and not an error

The pre-existing rule was that a version mismatch is a hard error, on the
sound reasoning that v1 and v2 hashes were computed under different rules and
comparing them "could only produce an unfounded accusation."

That reasoning does not extend to v2 → v3. v3 changes no existing hash; it only
adds fields. The two formats are comparable on everything they both describe.
So `COMPARABLE_FROM = 2` now bounds the error, and a v2 stamp gets its in-tree
inputs verified in full, with the builder lines elided from both sides and one
`NOTE` printed per run naming the fixtures that were not fully checked.

The alternative was a flag day: bump, accept that `check` fails, and coordinate
a simultaneous re-stamp. Rejected on two grounds.

1. **It reds the gate in all three lanes for a record nobody has had a chance
   to write.** `scripts/create-ext4-rootfs.sh` exits 1 on a stamp mismatch, so
   every lane's image build breaks until the rebuild lands — and the only
   escape hatch, `ALLOW_STALE_FIXTURES=1`, switches off the genuine check
   alongside the spurious one.
2. **The repair is not mine to make.** Re-stamping means rebuilding nine ELFs
   under `services/**`, which is lane B's tree, and `requests/a-c-fixture-
   rebuild-was-correct-on-lane-c-and-wrong-on-main.md` is the standing rule
   that the wrong lane must not do that rebuild. A gate that fails the build
   for something its reader is *forbidden* to fix is the same defect as no
   gate at all, one step over.

With the note instead, the change lands green, the gap is visible on every
`check`, and lane B's next rebuild upgrades each fixture to v3 with no
coordination at all.

### One version per format

Bumping `STAMP_VERSION` also silently broke the *sysroot* stamp, which shared
the constant. `sysroot_staleness()` diffs the on-disk sysroot stamp against a
fresh `compute_sysroot()` through `_describe_drift`, which compares every field
including `version` — so the bump reported `libc.a` as built from changed
inputs, a hard error raised *before* any per-fixture verdict, on every machine
whose stamp predated the bump. The image manifest escaped only by accident: its
reader ignores the version field.

Three formats now have three constants. Recorded here rather than only in a
comment because the failure is invisible until it fires, and because "one
version constant per format" is the kind of rule that gets undone by the next
person who sees three constants with the same value.

### Addendum: the rule was undone at the very next merge, exactly as predicted

The paragraph above guessed how this would be lost, and it took one merge. Lane
B's `0f0d436df` ("rootfs: stage CPython, and cover non-ELF payload in the image
manifest") rewrote `cmd_image_stamp` on `main` — correctly widening
`_staged_elfs()` to `_staged_artifacts()`, since the image now carries a stdlib
zip as well as ELFs — and, working from a tree that had never seen this entry,
wrote `version {STAMP_VERSION}` back into the image header.

Git could not have caught it. Both branches rewrote the same two lines, so it
surfaced as an ordinary text conflict, and either side taken whole is wrong:
ours loses the non-ELF payload, theirs loses the version split. The merge
(`d5d94ac1a`) takes `_staged_artifacts()` from `main` and `IMAGE_STAMP_VERSION`
from here, which is what neither branch could produce alone.

Worth stating as its own lesson, since this arrangement guarantees more of
them: **a conflict is not a request to choose a side.** When two lanes changed
the same lines for unrelated reasons, "ours" and "theirs" are both regressions,
and the merge is the only place the union can be written. The same merge had a
second instance, and the harder kind — lane A's `_fastpy_toolchain()` demanded
`PYTHONPATH` while lane B's new `_fastpy_dir()` already located the sibling
checkout automatically. They never touched the same lines, so there was no
conflict, and *because* there was no conflict nothing prompted anyone to connect
them. `check` had been printing "this machine cannot resolve them" on worktrees
that could resolve them all along. Textual conflicts are the merges git can
find; these are the ones it cannot.
