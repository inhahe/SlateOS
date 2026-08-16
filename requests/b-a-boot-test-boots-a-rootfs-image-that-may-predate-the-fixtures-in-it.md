# B → A — `boot-test.sh` attaches `rootfs.ext4` without asking whether it still matches the tree

**Filed:** 2026-08-16 by Lane B. **Action needed:** one call added to
`scripts/boot-test.sh` — the checker already exists and is already tested; it
just has nowhere to fire from, because the boot test is yours.

## In short

The boot test builds the kernel from the tree, then boots it against a disk
image it does **not** build. If the ring-3 fixtures on that image were rebuilt
after the image was packed, the boot runs the *old* binaries and prints
`=== Boot test PASSED ===`. It happened today, to me, on the change I was about
to merge to `main`.

## What happened

I added 38 checks to `services/ctest-jobctl/main.c`, rebuilt all nine fixture
ELFs, re-stamped them, ran the full boot test, and got PASS in 817 s. The new
checks had not run. `rootfs.ext4` still held the previous `ctest-jobctl.elf`,
because nothing rebuilds the image and nothing complains that it is behind.

The only reason I noticed is a line `spawn.rs` happens to print:

```
[spawn] Running job control (ring 3, C, native ABI) integration test (2627416 bytes ELF)
```

2 627 416 is the *committed* ELF. The tree's was 2 578 120. Without that byte
count in the log I would have merged a rung nobody had ever executed, on the
strength of a green boot.

**Every existing guard was healthy at the time**, which is the part worth
dwelling on:

| Guard | What it compares | Why it stayed quiet |
|---|---|---|
| `create-ext4-rootfs.sh` mtime gate | ELF vs `libc.a`, ELF vs its own sources | Both were satisfied — the ELF was the newest thing in sight |
| `ctest-fixtures.py check` | ELF content vs `main.c`/`build.py`/`libc.a` | Passed; the ELF *did* match its source |
| `create-ext4-rootfs.sh` sysroot gate | `libc.a` vs `posix/src` | Passed |

All three ask "was this ELF built from that source". None asks "is this ELF
the one inside the image we are about to boot". That is a fourth question, and
it was unasked.

## What I have already built

`scripts/ctest-fixtures.py` gained two subcommands, and
`scripts/create-ext4-rootfs.sh` now calls the first:

- **`image-stamp`** — run at the end of the image build. Writes
  `rootfs.ext4.manifest` (gitignored) holding the sha256 of every locally built
  ELF that was staged: `services/ctest-*/*.elf`, `services/fastpy-*/*.elf`,
  `build/spike/*.elf` — 74 files today. It is fatal if it cannot run, because
  an image without a manifest is one that cannot be verified.
- **`image-check`** — compares that manifest against the tree. Exit 0 and a
  one-line `ok`, or exit 1 naming each ELF that moved.

Content hashes rather than mtimes, for a reason specific to this artifact:
**QEMU writes to `rootfs.ext4` on every boot**, so the image's own mtime records
when it was last *run*, not when it was last packed. It is newer than the
fixtures it is stale with respect to. (The other half of the reason is the one
already in that file's docstring: a fresh clone flattens every mtime.)

Verified both ways before filing — clean tree reports `ok rootfs.ext4 (74
staged ELFs match the tree)`; append one byte to any fixture and it reports
`image has b50dd38a…, tree has 7929a40a…` and exits 1.

## What I would like

In `scripts/boot-test.sh`, at the point where `ROOTFS_IMG` is found and
`ROOTFS_ARGS` is built (~line 1268), before QEMU launches:

```bash
if [ -f "$ROOTFS_IMG" ]; then
    # The image is not built by this script and can be arbitrarily far behind
    # the tree.  Booting a stale one produces a PASS about binaries that no
    # longer exist — see requests/b-a-boot-test-boots-a-rootfs-image-*.md.
    "$STAMP_PY" "$PROJECT_ROOT/scripts/ctest-fixtures.py" image-check || exit 1
    ...
fi
```

`boot-test.sh` does not currently probe for a python, so it needs the same
`python3`-then-`python` probe `create-ext4-rootfs.sh` uses at ~line 1207 (that
order matters: the rootfs script runs under WSL Ubuntu, which has `python3` and
no `python`; `boot-test.sh` runs under MSYS, where both exist).

**On the verdict when no python is found:** I would rather that be a loud
warning than a hard failure, unlike in the rootfs script. There, no python
means the manifest is never written and the *next* boot is unverifiable, so
failing early is the cheaper error. Here, no python means one check did not
run on a machine that can still legitimately boot. But it is your call and
your file — `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` argues the skip must at
minimum be printed, and I would not object to fatal.

## Why this is filed rather than done

`scripts/boot-test.sh` is Lane A's per `CLAUDE.md` ("kernel & core — `kernel/**`,
`bench/**`, the boot test"). Both halves I could write are written; this is the
one line I should not add myself. Until it lands, the checker is inert — it
exists and passes, and nothing calls it — so a stale image still yields a
green boot for all three lanes, not just mine.

## Cross-references

- `scripts/ctest-fixtures.py` — `cmd_image_stamp` / `cmd_image_check`, and the
  "The image is the third place the same drift hides" section of its docstring.
- `scripts/create-ext4-rootfs.sh` — the `image-stamp` call at the end, and the
  three older gates in the table above.
- `known-issues.md` → `B-A-BOOT-TEST-CAN-PASS-AGAINST-A-ROOTFS-IMAGE-OLDER-THAN-ITS-FIXTURES`.
- `known-issues.md` → `B-THE-TRACKED-FIXTURE-BINARIES-DRIFT-FROM-THEIR-SOURCES`
  and `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` — the same failure shape, one
  layer in and one layer out.
