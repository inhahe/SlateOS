## TD-A-A-FIXTURE-THAT-WAS-NEVER-BUILT-IS-INVISIBLE-TO-THE-IMAGE-CHECK (lane A, 2026-09-10)

**Status: FIXED the same day, by lane B, in `scripts/ctest-fixtures.py`.**
`cmd_image_check` now calls `_unbuilt_recipes()` and refuses when a recipe has
a `build.py` and no `.elf`, before the hash comparison, with the two-step
remedy in the right order (build, then repack) — a message naming only the
repack would send the reader to a script that refuses.

Verified rather than assumed: a synthetic `services/ctest-zzsynthetic/` holding
only a `build.py` makes it exit 1 with
`services/ctest-zzsynthetic: has a build.py and no .elf`, and the tree was left
clean afterwards.

**A note on how this entry was nearly reimplemented.** Lane A filed it, sent
lane B a notice describing the mechanism, merged `origin/main` — which brought
their fix — and then, an hour later, began writing the same fix again from this
entry without re-reading the file. The duplicate was caught only because the
anchor for the edit no longer matched, and the block above it turned out to say
*"it happened on 2026-09-10 and cost a boot test its meaning"*.

That is worth recording next to the defect, because it is the same shape: a
note is a claim about the tree at the moment it was written, and acting on the
note instead of the tree is how a fixed thing gets fixed twice — or, in the
other direction, how a fixed thing stays filed as open. Both happened in this
file today.

**In short:** a ring-3 test fixture can arrive in the tree, never be compiled on
this machine, and the boot test will pass without ever running it — reporting
green for a test that did not happen. It happened today with the fixture for the
hostname syscalls, on the same day both lanes were writing to each other about
exactly this failure mode.

### The mechanism, precisely

`scripts/ctest-fixtures.py image-check` is the pre-boot verifier. It compares two
sets:

* `recorded` — what `rootfs.ext4.manifest` says was staged into the image;
* `actual` — `_staged_artifacts()`, the fixture **ELFs that exist in the tree**.

A fixture *recipe* (`services/ctest-<name>/build.py` plus its source) whose ELF has
never been built appears in **neither set**, so it cannot drift and is not
reported. `image-check` said `ok rootfs.ext4 (78 staged artifacts match the tree)`,
which was true and said nothing about the 79th the tree defines.

The only thing that *does* notice is `scripts/create-ext4-rootfs.sh:1886`
(`ERROR: N of M fixture ELFs have not been built here`) — and that runs only when
the image is repacked. The boot test had found `Prerequisites OK
(limine,services,rootfs)` and skipped the repack, so nothing asked.

The rung then did exactly what it is designed to do and said so:

    [spawn]   SKIP: ctest-hostname — prerequisite missing: /mnt/tests/ctest-hostname.elf

A loud skip, not a silent pass — `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` was fixed
for this reason. But one line in a 3,200-line serial log does not stop
`=== Boot test PASSED ===` from being printed, and the run is recorded as green.

### Why it is worth a gate rather than vigilance

This is the third member of a family already in this file: a check that cannot see
the thing it is about. `check-gates-are-wired` counts a call site, not a call site
that works; `check-unreachable-mutators` exited 0 and nothing ran it; and here the
image verifier compares what was built against what was staged, with no term for
what should have been built. In each case the absent thing is absent from the
evidence too.

### The fix

`cmd_image_check` should enumerate fixture *recipes* — the same
`services/ctest-*/build.py` and `services/fastpy-*/build.py` glob
`create-ext4-rootfs.sh` uses — and report any whose `<dir>/<dirname>.elf` does not
exist, as its own class of drift rather than folded into the hash comparison. The
wording matters: the remedy is two steps, `ctest-fixtures.py build` and then the
repack, and a message naming only the repack sends the reader to a script that
will refuse.

Scoped to the case where an image exists. A tree with no image already returns
early and says so, which is correct — a fresh clone with no toolchain has no ELFs
and nothing to verify.

### Reproduction

Land a new `services/ctest-*/` with a source and no ELF, then run
`python scripts/ctest-fixtures.py image-check` against an existing image. It
reports OK. Build the ELF and it correctly reports the image STALE, which is the
asymmetry: the check sees a fixture that is *newer* than the image but not one that
is *missing* from it.
