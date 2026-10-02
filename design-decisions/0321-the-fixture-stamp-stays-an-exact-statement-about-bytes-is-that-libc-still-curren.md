## §321 — The fixture stamp stays an exact statement about bytes; "is that libc still current?" is answered out-of-band, advisory

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** Nine small C test programs are checked into git as both source and
compiled binary, each beside a `.stamp` file recording a fingerprint of exactly
what went into it. Lane A asked whether the stamp should *also* record a
fingerprint of the libc's **source**, so it could say not just "this binary
matches the libc it was linked against" but "and that libc is still what the
source tree would produce". The answer is no: a fingerprint cannot answer that
second question accurately, and a stamp mismatch stops the disk image from
building — so an inaccurate answer there would block work on changes that broke
nothing.

### The question, and where it came from

`requests/a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md`
(lane A, 2026-08-16) reported a defect no lane could see in its own tree: lane C
rebuilt the nine fixtures correctly against the `libc.a` its branch produced,
lane B added thirteen libc symbols correctly on another branch, and the **merge**
of the two is a tree whose stamps are self-consistent and still wrong. Lane A
wrote `scripts/stamp-ancestry.py` to detect it and left one call to lane B:
whether to also put `git rev-parse HEAD:posix` — the tree hash of the libc's
sources — into each `.stamp`, beside the existing content hash of
`toolchain/sysroot/lib/libc.a`.

### The decision

**No.** The stamp keeps recording only content hashes of files actually consumed
(`build.py`, `main.c`, `libc.a`) and of the ELF produced. `stamp-ancestry.py`,
run after merges beside `ki_dupes.py`, keeps answering the source-currency
question separately. The repair half of the request is done — `3ad5c98aa`.

### Why — a source tree hash is wrong in *both* directions

The existing fields are exact: "the ELF on disk is the one these bytes produce"
is a claim a hash makes with no error either way. A `posix/` tree hash cannot
make the claim it would be there to make.

- **Over-approximate — it fires when nothing changed.** A comment, a rustfmt
  pass, a `#[cfg(test)]`-only edit: all move `posix/`'s tree hash and leave
  `libc.a` byte-identical. Two such commits are already in this month's history
  (`06ad616e0`, a formatting-only reformat of the whole crate; `96d62430f`,
  a clippy fix confined to test modules). Because `create-ext4-rootfs.sh`
  **exits 1** on a stamp mismatch, the consequence is not a warning: rewording a
  doc comment in `posix/` would stop the rootfs image building until nine
  binaries were relinked into nine byte-identical binaries. That is exactly the
  failure lane A named for the root `Cargo.toml` inside `stamp-ancestry.py` — "a
  check that cries wolf gets flagged into silence" — installed in the one place
  where silencing it means `ALLOW_STALE_FIXTURES=1`, which disables the *real*
  check alongside it.
- **Under-approximate — it stays quiet when things did change.** `libc.a` is not
  a function of `posix/` alone. It path-depends on `tzrules/`; the float-ABI
  `RUSTFLAGS` that `BUG-SYSROOT-SOFT-FLOAT-ABI` is about live only in
  `toolchain/build-sysroot.ps1`; `[profile.release]` in the root `Cargo.toml`
  genuinely changes codegen; and the rustc version is not in the tree at all. A
  stamp field named for `posix/` would read as authoritative while missing four
  of its own inputs. `stamp-ancestry.py` already tracks the wider set (`posix/` +
  `tzrules/` + `build-sysroot.ps1`, with `Cargo.toml` warning rather than
  failing) — precisely because one path is not enough.

A field that is both too eager and too lax is worse than no field: it converts
"there is no check here" into "there is a check here" for a reader who will not
re-derive its input set.

### Why out-of-band is the right *place*, not merely the cheaper one

The two questions differ in whether a wrong answer is recoverable:

| | asks | a wrong answer costs |
|---|---|---|
| `.stamp` (content) | did *these* bytes make *that* ELF? | never wrong — a hash mismatch is a real mismatch |
| `stamp-ancestry.py` (history) | is the libc those bytes name still current? | a re-run and a glance at one named commit |

The exact check can afford to be fatal because it does not misfire; the
approximate one must not be fatal, because it does. Merging them forces the pair
to share the strictest consequence and the loosest accuracy.

The coupling lane A raised — a `services/**` file naming a path outside
`services/**` — is real but secondary; the stamp already names
`toolchain/sysroot/lib/libc.a`, so that boundary is crossed either way. What
decides it is that one file would be making a claim it cannot support.

### What would genuinely close the gap, and why it is not this

The only exact answer is to *build* `libc.a` and hash the result — which
`build-sysroot.ps1` + `ctest-fixtures.py check` already does, and which the
request's own reproduction steps run. `ctest-fixtures.py`'s stated design goal
is to need **no toolchain**, so the check works on a machine with no zig/WSL;
that goal is what makes an exact source check impossible inside it, not an
oversight. The honest arrangement is therefore three layers: exact and
toolchain-free in the stamp, approximate and advisory in git history, exact and
expensive when a toolchain is present. All three exist; the middle one is new
today.

**Files.** No change — this records a decision *not* to alter
`scripts/ctest-fixtures.py`'s stamp format (`version 1`). The detector it defers
to is `scripts/stamp-ancestry.py` (lane A's).
