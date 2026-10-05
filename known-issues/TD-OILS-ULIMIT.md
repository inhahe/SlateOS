### TD-OILS-ULIMIT. `ulimit` tracks limits at the shell level but does not query/enforce the real kernel `getrlimit`/`setrlimit` — MINOR 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_ulimit`, `ulimit_print_all`,
the module-level `RLIMIT_SPECS` / `default_rlimits` / `ulimit_line` helpers, and
the `Shell::rlimits` field (`(soft, hard)` per option letter).

**Symptom:** `ulimit` was previously entirely missing (`osh: ulimit: command not
found`, exit 127), which hard-failed any script probing or setting limits. It is
now a real builtin: it models a per-shell `(soft, hard)` table (defaults mirror
Linux bash — `-n` 1024, `-s` 8192, `-c` 0, `-p` 8, most others unlimited),
supports the standard option letters, `-a`, `-H`/`-S`, and the
`unlimited`/`hard`/`soft` operands, and honours get/set within the shell (and
across subshell clones, like bash's per-process inheritance). **But it does not
touch the actual kernel:** on slateos it does not call `getrlimit(2)`/
`setrlimit(2)`, so `ulimit -n` reports osh's modelled default (1024) rather than
the real fd limit the kernel would enforce, and `ulimit -n 512` does not actually
constrain descendants. On the Windows host build there is no rlimit concept at
all, so the table is the only possible answer there.

**Why deferred:** oils is std-only (no libc/nix), and Rust std exposes no rlimit
API, so a real integration needs raw `getrlimit`/`setrlimit` syscall FFI gated to
the unix/slateos target (the kernel already has the plumbing — see
`kernel/src/fs/rlimit.rs`, `kernel/src/syscall/linux.rs`, `posix/src/ulimit.rs`).
That path cannot be exercised from the Windows dev host, so it was split out. The
shell-level model is honest (bash's `ulimit` is itself a thin wrapper whose
"set" only affects descendants) and eliminates the hard command-not-found failure.

**Proper fix:** add a `#[cfg(unix)]` raw-syscall shim (`getrlimit`/`setrlimit`,
or `prlimit64`) with a `// SAFETY:` note, seed `Shell::rlimits` from the live
kernel values at startup, and route set operations through `setrlimit` so
descendants inherit the enforced limit. Keep the current table as the Windows-host
and pre-seed fallback. Unit conversions (bash's block/kbyte display units vs. the
kernel's byte-granular `rlim_t`) must be applied at the FFI boundary.
