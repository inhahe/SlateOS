## §316 — The zig cross-toolchain is version- and hash-pinned, and cached once per machine rather than per worktree

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** To build the copy of GNU bash we ship, this repo needs a C
compiler that can target our OS. That compiler is `zig`, and until today
nothing in the repo said which version of it was needed or where to download it
— someone had simply put a copy in one of the four working directories by hand,
in a folder git does not track. So the build worked in that one directory and
was impossible in the other three, or in a fresh download of the project, and
nothing announced this. The fix is a small function that fetches a *specific*
zig version, checks it against a known fingerprint, and keeps it in one shared
place on the machine instead of four copies.

**What was wrong.** `design-decisions.md` §305 makes `bash-slateos.elf` a
shipped product and `scripts/bash-spike/` its build recipe — the README there
says in as many words, "keep them working." But the recipe had an unwritten
prerequisite:

| | Before | After |
|---|---|---|
| zig version required | recorded nowhere | `SLATE_ZIG_VERSION=0.13.0` in `scripts/lib/worktree.sh` |
| where to get it | recorded nowhere | fetched from `ziglang.org` by `slate_ensure_zig` |
| integrity check | none | SHA-256 pinned, verified *before* extraction |
| worktrees able to run the spikes | 1 of 4 (`os` only) | all, plus fresh clones |

This is the same failure this session has now hit six times: **a step that
appears to work because of undocumented local state.** It is close kin to the
hard-coded worktree paths fixed in the same commit range — there, a lane read
another lane's `libc.a`; here, three lanes could not build at all, and the one
that could did so on a toolchain nobody had pinned.

**The decision, and the line it draws.** `scripts/lib/worktree.sh` exists
precisely to stop one worktree touching another's files, so putting a *shared*
path back into it deserves justification. The distinction drawn:

- **Build outputs** — `libc.a`, bash's objects, the ELFs — stay strictly
  per-worktree. They encode *this tree's source*, so reading another lane's is
  the bug §B-THE-BASH-RELINK-SCRIPT… documents.
- **Pinned third-party input** — the zig tarball — is shared. Pinned by version
  *and* hash, it has identical bytes for every lane by construction, so sharing
  it cannot make one lane's artifact depend on another lane's source. That is
  the whole of the hazard, and it is absent here.

The cache lives at `~/.cache/slateos/` — outside every worktree. Deliberately
not inside `os/build/spike/`, which would have "fixed" lane B by making it
depend on the `os` checkout existing, i.e. the exact bug being fixed, wearing a
different hat.

**Alternatives rejected.**

1. *Copy zig into each worktree's `build/spike/`.* Works, but four ~200 MB
   copies of a byte-identical read-only input, and it leaves the real defect —
   nothing records the version — completely untouched. A fresh clone still
   cannot build.
2. *Document the download in the README and leave provisioning manual.* Better
   than nothing, and the README does now explain it. But a documented manual
   step is still a step that silently did not happen; the artifact it produces
   is shipped, so "did you remember to install the right zig?" should not be a
   question anyone has to answer correctly from memory.
3. *Use whatever `zig` is on `$PATH`.* Floating the compiler version under a
   shipped binary. The linked result would change with the host's package
   manager, and `self_test_bash_on_slateos_libc` would keep reporting OK about
   whichever toolchain happened to be installed.

**Why verify before extracting rather than after.** The extracted archive's
first use is *being executed as a compiler*, and its output is a binary we put
in the image. Checking afterwards would mean the untrusted archive had already
been unpacked and run. Cost is one `sha256sum` of a 47 MB file, once per
machine.

**Standing lesson.** A build recipe is only as reproducible as its least
documented prerequisite. When a recipe "works on my machine", check first
whether the machine is carrying an unrecorded input — and prefer a pinned,
verified, automatic fetch to a documented manual one, because the second kind
fails silently and the first kind cannot.
