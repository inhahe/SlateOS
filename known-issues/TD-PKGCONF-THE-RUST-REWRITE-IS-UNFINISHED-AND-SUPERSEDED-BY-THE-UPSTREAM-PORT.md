### TD-PKGCONF-THE-RUST-REWRITE-IS-UNFINISHED-AND-SUPERSEDED-BY-THE-UPSTREAM-PORT. 34 of upstream's 62 options, never run on target — kept as reference only — 2026-08-14 (clippy cleared 2026-08-16; still parked)

**Where:** `userspace/pkgconf/src/` — `main.rs` (1,075 lines), `flags.rs` (454),
`pcfile.rs` (667), `store.rs` (646), `version.rs` (301); 3,143 lines total.
Archived complete on branch `wip/pkgconf-rust-parked`.

**⚠️ Do not "finish" this without asking the operator first.** It is not a
half-done task waiting to be completed — it is work that was **superseded while
in progress**, and completing it would spend hours re-solving a problem that is
already solved. Read `design-decisions.md` §307 and `scripts/pkgconf-spike/`
before touching it.

**What happened.** This crate was a from-scratch Rust reimplementation of
pkgconf, started without first checking whether upstream pkgconf builds for
SlateOS. It does. Upstream pkgconf 2.3.0 cross-compiles and links against
`toolchain/sysroot/lib/libc.a` with zero source changes, zero shims and **zero
missing symbols** on the first attempt (`scripts/pkgconf-spike/`). Per §307 and
`roadmap-detailed.md`'s "Porting vs. Reimplementing" policy, the port wins.

**What actually works** (all measured, not assumed; 2026-08-14 unless marked):

- **119/119 unit tests pass** (2026-08-16; was 112/112). `cargo test -p pkgconf
  --target x86_64-pc-windows-gnu` **from the repo root** — 119 passed, 0 failed,
  0.08s.
- **Clippy is clean** as of 2026-08-16, `--all-targets`, and so is rustfmt. See
  "What was cleared" below for why that was worth doing to a parked crate.
- **It builds and links for the real target.** `cargo +nightly build-slateos -p
  pkgconf` produces a 21 MB static `ET_EXEC` for `x86_64-slateos`. An earlier
  note in the wip commit message claiming it was "never built for the
  x86_64-slateos target" was wrong; it builds.
- The `rpmvercmp` implementation in `version.rs` is genuinely complete and
  well-tested, including the `~` pre-release rule that pkg-config 0.29 itself
  gets wrong. If any part of this is worth salvaging, it is that file.

**What does not work:**

- **34 of upstream's 62 long options are implemented** — unchanged by the
  2026-08-16 pass, which added behaviour behind options that already existed
  and no new options at all. Missing: `about`,
  `define-prefix`, `digraph`, `dont-relocate-paths`, `dump-personality`, `env`,
  `env-only`, `exists-cflags`, `fragment-filter`, `ignore-conflicts`,
  `internal-cflags`, `license`, `list-package-names`, `log-file`,
  `maximum-traverse-depth`, `msvc-syntax`, `no-cache`, `no-provides`,
  `no-uninstalled`, `path`, `personality`, `prefix-variable`, `pure`,
  `relocate`, `shared`, `simulate`, `solution`, `verbose` — 28 in all.
  (`--frobnicate` and `--weird-name` appear in the source but are test
  fixtures for the unknown-option and `--` paths, not features.)
- **It has never been executed under the SlateOS kernel.** No on-target
  self-test exists. This, not the option count, is the thing that would have to
  change before anyone could claim it works.

**What was cleared, 2026-08-16, and why on a parked crate.** The entry
originally listed "clippy red: 9 errors, 2 warnings", and called the two
warnings "the real tell that it is unfinished". Both are now gone. Doing that
to a parked crate is worth an explanation, because "parked" and "leave it
broken" are not the same thing: a parked crate is still *read* — by whoever
salvages `version.rs`, and by anyone running `cargo clippy` across the
workspace — and a reader cannot distinguish a warning that means "unfinished"
from one that means "defect". A warning used as a status marker is a status
marker nobody can act on.

The two dead-code warnings were resolved in opposite directions, on the
evidence rather than uniformly:

- `Store::dirs()` was scaffolding for nothing, so it was **deleted**.
- `PcFile::path` was scaffolding for a **real gap**, so it was wired up.
  `pcfiledir` resolved inside `${...}` substitution but not for
  `--variable=pcfiledir`, so a relocatable package's `prefix=${pcfiledir}/../..`
  worked while the build system asking for `pcfiledir` outright got `""`.
  It now answers both ways, and `--print-variables` lists it (pkgconf does,
  pkg-config 0.29 does not — a name that `--variable=` answers and
  `--print-variables` hides is worse than either behaviour on its own).

That pulled in the **virtual `pkg-config` package**, which both reference
implementations synthesise rather than read from disk, and which is what makes
`PcFile::path` legitimately optional — a package with no file must not claim a
`pcfiledir`. It supplies `pc_path`, `pc_system_includedirs` and
`pc_system_libdirs` from the *compile-time* defaults and deliberately not from
the active search path: a cross build sets `PKG_CONFIG_LIBDIR` to the target
sysroot, and if `pc_path` followed it, every package built in that environment
would install its own `.pc` file into the sysroot and vanish from the host's
view of the system. Seven tests cover this, including that shape specifically.

The nine clippy errors are fixed rather than allowed, with one exception:
`struct_excessive_bools` on `Options` is allowed **with its reasoning in the
source** — pkg-config's twenty switches are not mutually exclusive (`--cflags
--libs --static` is the ordinary invocation), so an enum is wrong, and a
bitfield would trade twenty self-describing field names for twenty mask
constants. `format_push_string` is handled by a single `wr!` macro that confines
the infallible-sink `Result` discard to one audited place instead of three
`let _ =`.

**None of this changes the decision.** §307 stands: upstream pkgconf 2.3.0 is
what `/bin/pkgconf` and `/bin/pkg-config` are staged from
(`scripts/create-ext4-rootfs.sh` ~line 990), this crate ships nothing, and it
stays parked. The work above is maintenance on an archive, not progress toward
finishing it — 28 options are still missing and it has still never run under
the kernel.

**A hazard in the working tree.** `main.rs` is *tracked and modified*, while
`flags.rs`, `pcfile.rs`, `store.rs` and `version.rs` are *untracked*. The
committed `main.rs` is the older ~200-line standalone version with no `mod`
declarations. So committing `main.rs` alone would break the crate — it would
reference four modules that do not exist in the index. Commit all five or none.
The `wip/pkgconf-rust-parked` branch holds a consistent snapshot of all five;
it was advanced to the 2026-08-16 state in `a3c5cb306`, so that branch — not
this working tree — is what to read if the two ever drift.

**The proper fix, in the operator's preferred order:**

1. Ship upstream pkgconf. Needs the on-target ring-3 self-test described in
   `scripts/pkgconf-spike/README.md` ("What this spike does *not* establish") —
   `realpath`/`lstat`/`opendir` behaviour on our VFS and the 0x1000/0x2000
   segment alignment against our 16 KiB pages are the open questions.
2. Delete this crate, keeping `wip/pkgconf-rust-parked` as the archive, and
   drop `roadmap.md`'s `[x] pkgconf/pkg-config … ~200 lines` claim to point at
   the port instead.
3. Only if the operator wants a native Rust pkgconf for a reason the port
   cannot serve: finish the 28 options, clear clippy, wire up `path`/`dirs`,
   and add the on-target test. Record the reason under §307's "Where it flips".
