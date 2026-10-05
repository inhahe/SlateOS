## B-THE-BASH-BUILD-RECIPE-DEPENDED-ON-AN-UNRECORDED-ZIG-INSTALLED-BY-HAND

**Status:** FIXED 2026-08-16 (Lane B). See `design-decisions.md` §316.

`design-decisions.md` §305 makes `bash-slateos.elf` a shipped product and
`scripts/bash-spike/` its build recipe — the README there says "keep them
working". The recipe had a prerequisite recorded nowhere at all: the `zig`
cross-compiler.

**Symptom.** `scripts/bash-spike/*` and `scripts/pkgconf-spike/run.sh` could not
run in `os-lane-a`, `os-lane-b` or `os-lane-c`, or in a fresh clone. Only the
`os` integration checkout had a usable toolchain, because at some point someone
extracted zig by hand into `os/build/spike/zig/` — a **gitignored** directory.
Nothing in the tree recorded the version (0.13.0), the download URL, or a
checksum; `grep -rn "0\.13\.0"` over the whole repo returned nothing relevant.

**Why it went unnoticed for ~2 weeks.** The one checkout anybody ran the spikes
in happened to be carrying the missing piece, so the recipe looked healthy. This
is the same shape as the four other entries around it — *a step that appears to
work because of undocumented local state* — and it compounded the hard-coded
path bug directly above: while `slatelink.sh` still named `os` outright, it
found `os`'s zig too, so the two defects concealed each other.

**Fix.** `scripts/lib/worktree.sh` gained `slate_ensure_zig`, which resolves the
toolchain in three steps: an existing per-worktree `build/spike/zig/zig` if
present (so no existing checkout re-downloads or silently changes compiler
mid-build), else a shared cache at `~/.cache/slateos/`, else download the pinned
tarball and **verify its SHA-256 before extracting** — the archive's first use
is being executed as a compiler whose output we ship, so checking afterwards
would be checking too late. `slate_make_zig_wrappers` calls it, so the spikes
just work. Version and hash are pinned in `worktree.sh`;
`scripts/bash-spike/README.md` gained the Prerequisites section it never had.

**Standing lesson.** A build recipe is only as reproducible as its least
documented prerequisite, and an undocumented one is invisible precisely in the
tree where it is satisfied. Prefer a pinned, verified, automatic fetch over a
documented manual step: the manual kind fails silently and only in checkouts
nobody is looking at.
