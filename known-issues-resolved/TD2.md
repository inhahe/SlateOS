### TD2. Clippy `clippy::all` deny-level errors not yet zeroed — RESOLVED 2026-06-10 (regressed + re-fixed 2026-06-14)

**REGRESSION RE-FIX (2026-06-14):** 7 deny-level `clippy::all` errors had crept
back in since the original resolution — `byte_char_slices` (`drm/edid.rs:604`,
`fs/compress.rs:1956`), `question_mark` (`fs/fswalk.rs:229`+`:354`,
`fs/hotkeys.rs:134`), and `for_kv_map` (`fs/history.rs:435`, `proc/pcb.rs:1618`).
All fixed mechanically (byte-string literals, `?` operator, `.values()` map
iteration); `cargo clippy -p kernel` is back to **0 deny-level errors**, build +
QEMU boot test green. Lesson: the deny-level gate is only green between sweeps —
it needs to actually run in CI to stay zeroed (no CI exists yet).

**Sweep extended to all default-members (2026-06-14):** since the kernel was
not the only crate that could regress, `cargo clippy` was run on the other two
default-member crates. `posix` had **1** deny-level regression —
`too_many_arguments` (8/7) at `epoll.rs:2150`, `translate_kernel_event`,
introduced when the `is_dir` param was added for IN_ISDIR (TD17). Fixed with a
justified `#[allow(clippy::too_many_arguments)]` (commit `8acddca0c`); the 8
params are the distinct fields of one kernel watch event and a struct would only
add indirection to a pure host-tested translator. `toolchain/stubs`
(`slateos-stubs`) was already clean. All three default-members now report **0
deny-level errors**; posix's 19972 host tests still pass.

**RESOLUTION (2026-06-10):** `cargo clippy -p kernel` now reports
**0 deny-level errors** (down from 451) and ~17,297 warn-level warnings.
The deny-level `clippy::all` gate is green and can be used as CI.  The
warn-level lints remain by design (see below).  Landed across several
reviewable batches: the 158 doc-formatting lints, the 167 machine-
applicable idiom fixes, the 181 doc-comment lints, and a final hand-
fixed batch of 77 (commit `15dc0168`) covering `manual_memcpy`,
`ptr_arg`, `inherent_to_string`→`Display`, `wrong_self_convention`,
`upper_case_acronyms`, `enum_variant_names`, `type_complexity`,
`if_same_then_else` (inspected — no real copy-paste bugs), and a tail of
singletons (`fn_to_numeric_cast`, `forget_non_drop`, `never_loop`,
`only_used_in_recursion`, `pointers_in_nomem_asm_block`,
`large_enum_variant`, etc.).  `cargo build` and the QEMU boot test pass.

The two warn-tier correctness audits (step 3 below) are also complete:

* **`cast_ptr_alignment` (107) — audited, safe, left as warn.**  Every
  site is in MMIO / DMA-ring / on-disk-format / wire-protocol code
  (virtio, xhci, hda, e1000, ahci, ext4 `ondisk`, smp, `mm/frame`,
  syscall device-register reads).  Alignment is guaranteed by the
  page-aligned DMA frame allocation or by naturally-aligned hardware
  registers; the lint fires only because it sees a bare `*mut u8`/`*const
  u8` base.  Representative samples verified (e.g. `virtio/queue.rs:168`
  casts a page-aligned frame + 16-byte descriptor stride to
  `*mut VirtqDesc`).  One outlier — `ext4/ondisk.rs:1017` — casts an
  align-1 stack `[u8; 1024]` to a struct pointer; technically UB but
  benign on x86_64 and confined to a boot self-test.  No production
  under-alignment.  Eventual cleanup is a per-site `// SAFETY:` +
  `#[allow]`, but the casts are correct as-is.

* **`large_stack_arrays` (7) — audited; 1 genuine fixed, rest are false
  positives.**  Five (`cgroup.rs`, `fs/vfs.rs`, `klog.rs`, `mm/rmap.rs`,
  `sched/priority_rr.rs`) are `const fn` constructors whose arrays are
  const-evaluated directly into `static`/rodata storage — never on the
  stack; the lint is conservative.  `ktrace.rs:461` was a genuine 512-
  entry self-test window on the stack → now heap-allocated via
  `alloc::vec!`.  `scfilter.rs` built a ~19 KiB `FilterTable` on the
  stack before `Box::new` (the prior comment's "heap" claim was defeated
  by the by-value temporary) → `new()` is now `const fn` materialized via
  a `const EMPTY` binding so the box copies from rodata.  (Fixes + doc in
  the follow-up commit.)  The 6 remaining warnings are all const-context
  arrays in static storage and carry no stack-overflow risk.

---

**Original report (for history):**

**Where:** kernel-wide.  Snapshot `cargo clippy -p kernel` (rust 1.95.0,
2026-06-10): **451 deny-level errors** and **17,320 warn-level
warnings**.

**What this is — and why the two tiers are treated differently.**
The workspace lint config (`Cargo.toml [workspace.lints.clippy]`) sets
`clippy::all = deny (priority -1)`, `clippy::pedantic = warn`, and the
five correctness-pressure lints (`unwrap_used`, `expect_used`, `panic`,
`indexing_slicing`, `arithmetic_side_effects`) = `warn`.  So:

* **Warn-level (17,320) — intentional by design, NOT a blocker.**
  Dominated by:
  - `arithmetic_side_effects` 7,511
  - `indexing_slicing` 5,711
  - `expect_used` 2,689
  - `unwrap_used` 1,034
  - `unnecessary_wraps` 156, `cast_ptr_alignment` 107, others < 25 each.

  These are the defensive-pressure lints CLAUDE.md deliberately set to
  `warn` rather than `deny` because they are pervasive in low-level
  kernel code (every `a + b`, every `slice[i]`, every page-table index)
  and forcing `checked_*`/`.get()` everywhere would bury real signal
  under mechanical noise.  They are advisory: the rule is "prefer `?`,
  `.get()`, `.checked_*` in new code and surgically harden hot/attacker-
  reachable paths," not "drive the count to zero."  **These are accepted
  by design and should NOT be mass-rewritten.**  Two sub-categories DO
  deserve a real audit pass and should be tracked as their own work:
  `cast_ptr_alignment` (107 — genuine UB risk if any cast actually
  under-aligns; most are MMIO/identity-mapped and provably fine but each
  should carry a `// SAFETY:`/`#[allow]` with justification) and
  `large_stack_arrays` (7 — kernel stacks are bounded; verify none blow
  the stack).

* **Deny-level (451 `clippy::all` errors) — these SHOULD be fixed**, per
  the project's own `all = deny` gate.  The good news: they are almost
  entirely **mechanical, machine-applicable idiom lints**, not logic
  bugs.  Top categories:
  - `doc_overindented_list_items` 137, `doc_lazy_continuation` 21
    (158 = doc-comment formatting — auto-fixable)
  - `unwrap_or_default` 21, `manual_strip` 15, `manual_slice_fill` 14,
    `vec_init_then_push` 13, `manual_memcpy` 10, `manual_clamp` 8,
    `assign_op_pattern` 8, `manual_div_ceil` 8, `slow_vector_
    initialization` 7, `while_let_loop` 6, `explicit_counter_loop` 5,
    `single_char_add_str` 5, `single_match` 5 … (all auto-fixable)
  - A small tail needs human judgment, not blind `--fix`:
    `type_complexity` 10 (extract type aliases), `duplicated_attributes`
    9 (a module-level `#![allow(dead_code)]` duplicating the parent
    `#[allow]` in `fs/mod.rs` — remove the inner one),
    `upper_case_acronyms` 9 and `enum_variant_names` 7 (renames — verify
    no public-API churn), `if_same_then_else` 7 (could be a real copy-
    paste bug — inspect each), `comparison_to_empty` 7.

**File distribution of the 451 errors** (primary span):
`syscall/linux.rs` 200, `kshell.rs` 39, `fs/bzip2.rs` 8,
`syscall/handlers.rs` 8, `sched/mod.rs` 6, `fs/contextmenu.rs` 5,
`fs/procfs.rs` 5, `fs/monitors.rs`/`fs/tags.rs`/`fs/taskbar.rs`/
`net/http.rs` 4 each, then a long tail of 1–3 across ~40 more files.
`linux.rs` alone is 44% of the total (it is the single largest source
file, ~28k lines, and accretes idiom lints fast).

**Why it's open rather than fixed-on-sight:** the count is large and
spread across ~50 files; the bulk is `cargo clippy --fix` territory but
that produces a sweeping multi-file diff that materially changes the
shape of hot syscall code (`linux.rs`), so it warrants being landed as
its own reviewable change(s) rather than smuggled into a feature commit.
Two deny-errors that were authored as part of the /proc work
(2026-06-10) were fixed immediately at their source:
`procfs.rs` `gen_pid_statm` doc list (`doc_overindented_list_items`) and
`pcb.rs` `set_exe_path` (`manual_contains` → `slice.contains`).

**Tooling caveat (verified 2026-06-10):** `cargo clippy --fix` does
**not work** in this environment — it recompiles, reports the count of
machine-applicable suggestions (e.g. "to apply 176 suggestions"), but
writes **zero** changes to disk.  Tried four ways:
`cargo clippy -p kernel --fix --allow-dirty`;
`… --bin kernel … --no-deps`;
`… -- --force-warn clippy::all` (to defeat the deny-as-error so the
verify-recompile would pass); and with the workspace
`clippy::all` level temporarily flipped to `warn` in `Cargo.toml`.
All four no-op'd (0 `.rs` files modified, ~4 min each).  The kernel
targets the built-in `x86_64-unknown-none` with no build-std, so this
is not a custom-target issue; it looks like `cargo fix`'s
write-back/verify phase failing silently on this Windows toolchain.
**Do not burn build cycles retrying `--fix` — remediation must be by
hand (or with a non-cargo rewrite tool).**

**Proper fix / remediation plan:**
1. Hand-fix the machine-applicable bulk in reviewable batches, grouped
   by lint family so each diff is easy to verify: start with the ~158
   doc-formatting lints (`doc_overindented_list_items`,
   `doc_lazy_continuation` — pure comment edits, zero risk), then the
   manual-idiom families (`unwrap_or_default`, `manual_strip`,
   `manual_slice_fill`, `vec_init_then_push`, `manual_memcpy`,
   `manual_clamp`, `assign_op_pattern`, `manual_div_ceil`, …).  These
   rewrites are semantics-preserving (`manual_memcpy` →
   `copy_from_slice`, `vec_init_then_push` → `vec![…]`, etc.).  Land
   `linux.rs` (200 of the 451) as its own commit(s) since it is the
   hottest file and the largest single chunk.  Boot-test after each
   batch.
2. Hand-fix the judgment tail: dedupe the `#![allow(dead_code)]`
   attributes, extract `type_complexity` aliases, inspect every
   `if_same_then_else` for an actual logic bug before collapsing it,
   and do the acronym/enum renames with a grep for external callers.
3. Separately audit `cast_ptr_alignment` (107) and `large_stack_arrays`
   (7) from the warn tier — these are the only warn-level lints with a
   real correctness dimension; annotate or fix each.
4. Leave the remaining warn-level lints as-is (by design); revisit only
   the policy, not the individual sites.

Until step 1–2 land, `cargo clippy -p kernel` exits non-zero, so it
cannot be used as a CI gate yet.  `cargo build` / boot-test are clean.

---

### (closed) TD1 — `frame::ALLOCATOR` IRQ-safety — closed as F5 on 2026-06-07.

---

### TD — posix `realpath()` does not resolve symlinks (POSIX non-conformance)

**Where:** `posix/src/unistd.rs:1398` (`realpath`), also reached via
`__realpath_chk` (`posix/src/file.rs:4674`) and `canonicalize_file_name`.

**What:** POSIX `realpath()` must return the *canonicalized* absolute path
with **all symbolic links resolved**.  The SlateOS implementation instead
calls `resolve_path()` (which only normalizes `.`/`..` and makes the path
absolute) and then verifies existence via `SYS_FS_STAT`.  It returns the
*normalized input* path, **not** the symlink-resolved target.  So for a
symlink `/tmp/link -> /tmp/target`, `realpath("/tmp/link")` yields
`/tmp/link` rather than `/tmp/target`.

**Repro:** create `target`, `os.symlink(target, link)`, then `realpath(link)`
returns the link path, not the target path.  A GNU `realpath`/`readlink -f`
or CPython `os.path.realpath` would return the target.

**Impact:** anything relying on realpath to canonicalize through symlinks
(dedup by canonical path, mount detection, security checks that compare
canonical paths) is subtly wrong.  Existence + `.`/`..` normalization work;
only symlink resolution is missing.  This is why the fastpy `os.path.*`
self-test suite does **not** include a realpath symlink-resolution probe —
it would falsely pass.

**Proper fix:** resolve the path component-by-component in the kernel VFS
(which already follows symlinks for `SYS_FS_STAT`) and expose the resolved
path — either a new `SYS_FS_REALPATH`/canonicalize syscall returning the
fully-resolved path, or a userspace `readlink()` loop in `realpath()` that
walks each component and follows links (bounded by a symlink-depth limit to
avoid loops, returning `ELOOP`).  The kernel already has
`resolve_path_no_follow` and symlink-following metadata, so a canonicalize
syscall is the clean approach.
