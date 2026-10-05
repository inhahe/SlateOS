## §87 — Reduce embedded-ELF kernel bloat before promoting fastpy coreutils to /bin

**Date:** 2026-07-23
**Decided by:** Operator (Claude recommended A; operator leaned B — "I lean towards B")

**Decision.** For the next phase of initiative F, do **option B — reduce the
embedded-ELF kernel bloat (TD-KERNEL-EMBED-BLOAT) first** — before **option A**
(promoting fastpy-compiled coreutils to real `/bin` commands driven by the
shell). The ~48 fastpy self-test ELFs are currently `include_bytes!`'d into the
kernel's `.rodata` (~3.5 MiB each; kernel image ~202 MB). Move them (and future
fastpy binaries) onto the **rootfs disk** and load-from-disk.

**Why.** The initiative-F `os.*` self-test line has *saturated* (identity,
scheduling nice, umask+create-mode, metadata, content I/O, namespace, query,
timekeeping, sleep, pipes, the whole pkg suite) — further self-tests are low-value
churn. Both A and B are the real "native executables for OS components" payoff.
The operator leaned B because it is real, growing tech debt (slow builds/boot from
a 202 MB image) and is *prerequisite-ish* for a `/bin` that lives on disk anyway —
so doing B first avoids building the `/bin` install pipeline twice.

**Alternatives considered.**
- *A — promote fastpy coreutils to real `/bin` commands (Claude's recommendation,
  highest end-value)*: deferred, not rejected — it is the actual roadmap goal and
  the natural follow-on once `/bin` lives on disk (B). Its verification fork
  (non-interactive boot-test; osh command-resolution surface) remains to be
  settled when A is taken up.
- *C — declare the self-test phase done and move to an unrelated roadmap area*:
  rejected — leaves the "native executables for OS components" payoff unrealized.

**Tradeoffs / open sub-questions (to resolve while implementing B).** Boot-ordering:
the self-tests currently run *before* the rootfs (Path-Z glibc `rootfs.ext4`, vdb)
is guaranteed mounted, so either the self-tests must move after mount, or a small
early-mount / dedicated self-test partition is needed. Also a mechanism sub-choice
(plain disk files vs a compressed archive vs a dedicated fastpy-bin image). These
are implementation decisions for B, not re-openings of the A/B/C fork.

**Where it lives.** `kernel/src/proc/spawn.rs` (the `include_bytes!` self-test
harness — every `self_test_fastpy_slateos_*`), the boot-image assembly, the rootfs
build, and `scripts/boot-test.sh` (non-interactive verification). Tracked as
TD-KERNEL-EMBED-BLOAT in `known-issues.md`.

**Sub-design resolved (2026-07-23) — implementing B.** *Decided by:* Claude
(operator-approved scope). The two open sub-questions flagged above are now
settled from reading the code:
- *Boot-ordering: no reordering needed.* The rootfs (`rootfs.ext4`, vdb) is
  mounted at `/mnt` in `kmain` at ~line 1180 (`fs::ext4::mount(ext4_dev,"/mnt")`),
  which runs **long before** the Path-Z self-test block (~line 2320+) where the
  fastpy tests live. So the fixtures are already on a mounted fs by the time the
  tests run — no early-mount and no dedicated self-test partition required.
- *Mechanism: plain disk files on the existing rootfs, not a compressed archive
  or a separate image.* Stage the 49 distinct `services/fastpy-*/*.elf` (~3.3 MiB
  each) flat into `/tests/<name>.elf` on `rootfs.ext4` at rootfs-build time
  (`scripts/create-ext4-rootfs.sh`, bump `IMG_SIZE` 48M→256M). Load each at
  runtime with `load_test_elf(name) -> Option<Vec<u8>>`, a thin wrapper over the
  existing `crate::fs::Vfs::read_file("/mnt/tests/<name>.elf")` primitive already
  used throughout the Path-Z tests; `None` (file/disk absent) makes the affected
  self-test cleanly *self-skip* rather than fail, so a lean production build (no
  fixture disk) still boots green.
- *Why plain files over an archive/dedicated image:* zero new machinery (reuses
  the mounted ext4 + `Vfs::read_file`), the fixtures are individually inspectable,
  and it mirrors exactly how the glibc Path-Z fixtures are already staged. A
  compressed archive would need an in-kernel decompressor on the test path; a
  dedicated fastpy-bin image would add a second disk + mount for no benefit at
  this stage. When A (promote to real `/bin`) is later taken up, the staging
  simply retargets `/bin` and the self-tests spawn the installed binaries — the
  loader indirection introduced here is the seam that makes that trivial.
- *Scope correction:* only the **61 fastpy sites in `spawn.rs`** are the 164 MiB
  bloat. The 3 `main.rs` + 6 `container.rs` `include_bytes!` sites are tiny
  hand-written no_std services (`init`/`hello`/`ticker`, tens of KB) and stay
  embedded.

**Implemented & verified (2026-07-23).** Done exactly as designed above. 54
`static FASTPY_*_ELF` decls in `spawn.rs` converted to `load_test_elf()` disk
loads (self-skip on absence); all 49 fastpy `*.elf` staged into `/tests` by
`create-ext4-rootfs.sh` (image 48M→256M). **Debug kernel binary 361.7 MiB →
181.8 MiB (−180 MiB / ~50 %).** All 55 fastpy ring-3 self-tests pass loading
from `/mnt/tests`, green boot. One surprise: the sparse fastpy ELFs were the
*first* sparse files ever read through the ext4 extent path, which exposed a
latent extent-reader bug (holes collapsed → data shifted) — fixed as
BUG-EXT4-SPARSE-READ (`kernel/src/fs/ext4/driver.rs`, `block_copy_placement`).
The loader indirection (`load_test_elf`) is now the seam for the follow-on
option A (promote fastpy coreutils to real `/bin`).

**Follow-on — first /bin promotion (2026-07-23).** Option A started: `fastpy-cat`
is the first fastpy binary promoted from a `/tests` self-test fixture to a real
installed command. `create-ext4-rootfs.sh` now maps a curated
`PROMOTED[fastpy-cat]=cat` set — the binary is installed at `/bin/cat` (and
*not* also under `/tests`, so no ~3.5 MiB duplication). The kernel gained a
`resolve_command(name, PATH)` helper (`spawn.rs`) that searches `COMMAND_PATH`
(`["/mnt/bin"]`, where the rootfs `/bin` is mounted) — the resolve+load step a
shell/`init` performs before `exec`. `self_test_fastpy_slateos_cat` now resolves
`cat` **by command name** through that PATH and spawns it with `argv[0]="cat"`,
so the test exercises the real installed-command execution path rather than a
fixture load. Additive and reversible: no existing Rust coreutil is touched, and
the promotion is a per-command opt-in via the `PROMOTED` map. The file-reading
coreutils `wc`, `head`, and `tail` were promoted next the same way (their
self-tests now `resolve_command(...)` and spawn `argv[0]="wc"|"head"|"tail"`), so
`/bin` holds four fastpy commands and `/tests` dropped to 45 fixtures. A second
batch — `grep`, `sort`, `uniq`, `ls` — followed the same way, bringing `/bin` to
eight promoted fastpy commands and `/tests` to 41 fixtures. A third batch — the
filesystem-mutating coreutils `rm`, `mv`, `mkdir`, `rmdir`, `chmod`, `chown` —
was promoted the same way, bringing `/bin` to 14 promoted fastpy commands and
`/tests` to 35 fixtures. (The remaining `fastpy-*` self-tests are OS-API probes,
not user-facing commands, so they stay under `/tests`.) Whether these minimal
utilities should ever *replace* the mature Rust coreutils in a shipping `/bin` is
deferred to the operator as `open-questions.md` Q35 (recommendation: keep the
promotions additive; never swap a Rust coreutil silently).
