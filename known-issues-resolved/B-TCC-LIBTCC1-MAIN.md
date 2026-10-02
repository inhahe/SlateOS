### B-TCC-LIBTCC1-MAIN. On-target tcc one-shot compile+link spuriously fails with `unresolved reference to 'main'` (exit 1) when the source emits one extra undefined symbol (e.g. the `memset` a struct/aggregate brace-initialiser synthesises) — ON-TARGET-ONLY, **COULD NOT REPRODUCE (22 on-target compiles) — DOWNGRADED TO WATCH**, REGRESSION-GUARDED 2026-07-16

**UPDATE 2026-07-16 (could not reproduce; downgraded WATCH; regression
guard added).** On-target instrumentation was built and run to reproduce
this live: a boot self-test (`self_test_tcc_diag_brace_init`, since
removed) compiled **four distinct `memset`/`memcpy`-emitting constructs**
(constant brace-init, runtime-value brace-init, a 256-byte zero-init
array, and a struct-to-struct copy) **five times each = 20 on-target
`tcc -vv` compiles**, plus two earlier single shots = **22 on-target
compiles that all carried the extra undefined `memset`/`memcpy` symbol.
Every one linked and ran cleanly (exit 0, valid dynamic ELF).** The
documented deterministic trigger — "one extra undefined symbol makes the
on-target link lose `main`" — is therefore **disproven**: `memset`
presence is *not* sufficient to reproduce the failure. The original Part
47 failure was thus either genuinely **intermittent/rare** (timing- or
heap/VFS-state-dependent, like the sibling `B-WAITQ-IDLEPARK` lost-wakeup
family) or was **already fixed** by an unrelated change since Part 47.
Because no root cause could be pinned and no deterministic repro exists,
the entry is downgraded from OPEN to **WATCH**.

A permanent **regression guard** now exists:
`self_test_linux_real_glibc_cc_brace_memset` (Path Z Part 56,
`kernel/src/proc/spawn.rs`), wired into the boot self-tests, compiles +
glibc-links + runs in ring 3 a program with a genuine runtime-`memset`
aggregate brace-initialiser and asserts output `42\n`. If tcc ever
regresses to losing `main` when a synthesised `memset` is present, that
rung fails and emits a `self-test failed` WARNING the boot-test scans
for. The field-init workaround in the other Path Z rungs is no longer
strictly required (brace-init is proven reliable) but is harmless and
left in place. The original OPEN analysis is retained below for history.

**Symptom.** A hosted compile+link in a *single* on-target tcc invocation
(`tcc -o /prog /prog.c`, the shape `run_hosted_cc_case` uses) fails with
`tcc: error: unresolved reference to 'main'` (exit 1) — even though the
source plainly defines `int main(void)`. The trigger observed live was an
aggregate **brace initialiser** (`struct s x = {…};`), which tcc lowers to
a synthesised `memset` reference; the *field-wise* version of the same
program (one fewer undefined symbol: only `write`) links and runs cleanly.

**IMPORTANT — earlier mechanism guess was wrong.** The first draft of this
entry blamed `libtcc1.a` (claiming tcc resolves the synthesised
`memset`/`memcpy` from its runtime archive and that perturbs the link).
That is **incorrect**: `ar t`/`nm --print-armap` on the staged
`libtcc1.a` show it defines the soft-float/atomic/alloca/va_list helpers
but **not** `memset`/`memcpy` — those resolve from glibc (`libc.so.6`).
So `libtcc1.a` is not pulled in by the brace-init program at all. The real
differentiator is simply the *one extra undefined symbol* (`memset`), and
the breakage is **on-target-specific**.

**Reproduction / diagnosis (what was actually done).** Extracted the whole
staged toolchain from `rootfs.ext4` via `debugfs -R "dump …"` (tcc, crt1/
crti/crtn.o, the `libc.so` GNU-ld GROUP script, `libc_nonshared.a`,
`libtcc1.a`, `libc.so.6`, `ld-linux`) and re-ran the *extracted target
tcc* under WSL:
  - `tcc -c prog.c -o prog.o` → OK; `nm` shows good `T main`, plus
    `U memset` for the brace-init variant vs. only `U write` for field-init.
  - Full `tcc -o prog prog.c` (one-shot compile+link) → **exit 0 for BOTH
    variants**, both with WSL's native crt/libc and with the OS's staged
    crt + `libc.so` GROUP script + `libc_nonshared.a` + `libtcc1.a` forced
    in explicitly via `-nostdlib`.
So the `unresolved 'main'` failure **does not reproduce off-target** — it
only happens when tcc runs *inside the OS* (under our Linux-syscall
translation + VFS). That points at an OS-side interaction (tcc's file
reads of the large `libc.so.6` / GROUP-script / archive parsing under our
syscall+VFS layer, or a heap/symbol-table quirk in tcc keyed to the extra
symbol), **not** an archive-index or link-ordering defect in the staged
files themselves.

**Why it matters.** The on-target C toolchain can currently mis-link (in
one step) programs that carry an extra compiler-synthesised undefined
symbol — most commonly aggregate brace initialisers (a lot of ordinary C).
Path Z rungs sidestep it (hand-rolled field init); coreutils/real projects
may hit it. Workaround: compile `-c` then link separately, or avoid the
construct.

**Where it lives.** On-target `tcc` (`/bin/tcc`, 0.9.28rc mob) running via
the Linux-ABI syscall translation + VFS; the staging is in
`stage_hosted_cc_support` (`kernel/src/proc/spawn.rs`). The self-test that
first exposed it: `self_test_linux_real_glibc_cc_struct` (Path Z Part 47).

**Proper fix (open — needs on-target instrumentation).** Because it only
reproduces inside the OS, the next step is to capture what tcc actually
does there: strace-equivalent of the failing link (there is already
`scripts/extract-tcc-strace.sh` / `scripts/probe-tcc-hosted.sh`) to see
whether a file read of `libc.so.6` / the GROUP script / `libc_nonshared.a`
returns short/EOF-early, or whether tcc's dynamic-symbol lookup for
`memset` walks into a region our VFS serves incorrectly. If a specific
syscall/VFS read is returning wrong data for large files under tcc's
access pattern, fix that; otherwise it may be a genuine tcc bug worth
patching in the port. Until then the entry stays WATCH/OPEN with the
field-init workaround in place.
