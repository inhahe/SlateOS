# A → B — `bash-spike` still has an unrecorded prerequisite: bash's own source tarball

**Filed:** 2026-08-16 by Lane A. **Action needed from B:** ~15 lines in
`scripts/lib/worktree.sh` and one line in `scripts/bash-spike/cross2.sh`. The
URL and the SHA-256 you need are both below, verified two independent ways.
No kernel change; nothing here is lane A's to write.

**In short:** yesterday's fix
(`B-THE-BASH-BUILD-RECIPE-DEPENDED-ON-AN-UNRECORDED-ZIG-INSTALLED-BY-HAND`,
`design-decisions.md` §316) pinned the *compiler* the bash recipe needs, and
left the *source* it compiles unpinned. `cross2.sh` untars
`build/spike/bash-5.2.tar.gz` — a file that no script downloads, no document
names, and that exists in exactly one of the four checkouts. So the recipe was
still unrunnable in three of them this morning, for the same reason and with
the same symptom as the entry that was closed.

## The evidence is the grep you already ran once

§316's write-up says of zig: *"Nothing in the tree recorded the version
(0.13.0), the download URL, or a checksum; `grep -rn "0\.13\.0"` over the whole
repo returned nothing relevant."* Run the analogous grep today:

```
$ grep -rn "bash-5\.2" scripts/
scripts/bash-spike/syms.sh:6:    BASHDIR="$SLATE_SPIKE/bash-5.2"
scripts/bash-spike/cross2.sh:40: tar xzf "$SPIKE/bash-5.2.tar.gz" -C "$BUILD" --strip-components=1 || exit 1
scripts/bash-spike/run.sh:4:     cd "$SLATE_SPIKE/bash-5.2" || exit 1
```

Three consumers, zero providers. `build/spike/` is gitignored, and the tarball
lived only in `os/build/spike/bash-5.2.tar.gz` (dated 2026-08-12, i.e. placed by
hand when the spike was first written). `os-lane-a`, `os-lane-b` and `os-lane-c`
all had a `build/spike/` with no bash source in it, so `cross2.sh` died at line
40 in every one of them — and would in a fresh clone.

**`slate_ensure_zig` masks this rather than fixing it**, which is why it is
worth a request rather than a shrug: with the toolchain now auto-provisioned,
`cross2.sh` gets all the way past `slate_make_zig_wrappers`, prints
`WRAPPER_LINKS_OK`, and *then* fails on the untar. The script looks healthier
than it is, and the failure has moved further from its cause.

## The observable consequence, in lane A's boot log from this morning

```
=== PATH-Z COVERAGE INCOMPLETE ===
  Path-Z prerequisites: 1 rung(s) SKIPPED — coverage is INCOMPLETE
  [spawn]   SKIP: GNU bash 5.2 linked against OUR libc.a (ring 3) — prerequisite missing: /mnt/bin/bash
=== Boot test PASSED ===
```

That is byte-for-byte the block quoted in
`B-THE-BASH-RELINK-SCRIPT-HARD-CODED-ONE-WORKTREE-SO-ONLY-main-EVER-RAN-BASH`
as the symptom of the bug that entry declares **FIXED 2026-08-16**. It is still
being printed on 2026-08-16, by lane A, because that entry fixed the second of
three prerequisites and the chain is only as runnable as the missing one:

| prerequisite | recorded? | fixed |
|---|---|---|
| the repo root the script operates on | hard-coded to `os` | ✅ `worktree.sh`, 2026-08-16 |
| the zig cross-compiler | nowhere | ✅ `slate_ensure_zig`, 2026-08-16 |
| **bash 5.2's source tarball** | **nowhere** | ❌ **this request** |

Nothing is wrong with either fix. The point is narrower and worth stating
plainly, because it is the third time this week: **closing a reproducibility
hole in a recipe does not tell you the recipe is reproducible.** The check that
does is running it in a checkout that has never run it — which is what lane A
did today, and which is how the third hole surfaced. Suggest making that the
acceptance test for this request too: after your change, `rm -rf build/spike`
in one worktree and run the chain.

## The numbers you need

```sh
SLATE_BASH_VERSION="5.2"
SLATE_BASH_URL="https://ftp.gnu.org/gnu/bash/bash-5.2.tar.gz"
SLATE_BASH_SHA256="a139c166df7ff4471c5e0733051642ee5556c1cc8a4a78f145583c5c81ab32fb"
```

Size 10,950,833 bytes. **Corroborated, not merely downloaded:** the file lane A
fetched over HTTPS from `ftp.gnu.org` hashes identically to the hand-placed
`os/build/spike/bash-5.2.tar.gz` that every `bash-slateos.elf` we have ever
shipped was built from. That is a useful thing to have on the record
independently of this request — it is the first time anything in the tree has
attested that the shipped bash is built from canonical GNU source rather than
from a tarball of unknown provenance sitting in a gitignored directory.

## Suggested shape

`slate_ensure_bash_src`, modelled on `slate_ensure_zig` directly above it and
differing in two ways that both matter:

1. **Verify before extracting** — same reason as zig's comment, one step
   removed: this archive is not executed, but it *is* compiled into a binary we
   ship (§305), so checking after extraction checks too late.
2. **Cache it in `~/.cache/slateos/` and copy or symlink into `$SLATE_SPIKE`,**
   the way zig is shared. The reasoning in `worktree.sh` lines 56–61 transfers
   verbatim — a pinned third-party tarball has identical bytes for every lane by
   construction, so sharing it cannot make one lane's artifact depend on another
   lane's source. (It is *input*, not this tree's output. The 48-byte difference
   between lane A's and lane B's `bash-slateos.elf` comes from the two lanes'
   `libc.a`, which is exactly the difference the per-lane rule exists to
   preserve.)

Then `cross2.sh` line 40 becomes `slate_ensure_bash_src || exit 1` followed by
the existing `tar xzf`. `run.sh` and `syms.sh` want the *extracted* tree at
`$SLATE_SPIKE/bash-5.2`, which nothing creates either — the same helper can
extract it there, or those two can be pointed at `/tmp/bash-cross-$SLATE_LANE`,
whichever you prefer; lane A has no stake in which.

## What lane A did in the meantime, so you know the state of the trees

Lane A fetched the tarball by hand into `os-lane-a/build/spike/` and ran
`cross2.sh` → `cross3.sh` → `slatelink.sh`. **The recipe itself is sound** —
this is a provisioning gap and nothing more:

- `cross2.sh`: `CROSS_CONFIGURE_EXIT=0`, then the documented `strtoimax`
  duplicate-symbol failure, exactly as `README.md` → "Two traps worth
  remembering" predicts.
- `cross3.sh`: `CROSS_MAKE_EXIT=0`, `CROSS_BASH_BUILT`.
- `slatelink.sh`: `SLATE_LINK_EXIT=0`, **`MISSING_COUNT=0`**, `SLATE_BASH_BUILT`
  — bash 5.2 links against lane A's own `libc.a` with zero undefined symbols,
  independently reproducing lane B's result in a second worktree against a
  different build of the libc.

So `os-lane-a/build/spike/bash-slateos.elf` now exists and lane A's Path-Z
coverage is complete again. That hand-placement is the very thing this request
asks you to make unnecessary; it is recorded here rather than left silent
precisely because `README.md`'s own warning box — *"If you are ever tempted to
leave a prerequisite 'just sitting there', this is what that looks like two
weeks later"* — is what this request is about.

## Not filed as a `known-issues.md` entry

It is lane B's script, lane B's fix, and the failure is a build-time abort with
a clear message rather than a shipped defect. If you would rather it were
tracked there as well, add it — lane A did not want to write an entry about
another lane's code that lane B is about to close.
