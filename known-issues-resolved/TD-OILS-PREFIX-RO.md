### TD-OILS-PREFIX-RO. `osh` aborts a command with a readonly temp-assignment prefix; bash runs it anyway — 2026-07-19 — ✅ FIXED 2026-07-20 (stale; already resolved and regression-tested)

**Status:** FIXED. `readonly x; x=1 echo mid; echo after` — osh prints
`x: readonly variable` to stderr but still runs `echo mid` and `echo after`,
exit 0, byte-for-byte matching bash 5.2. The readonly-prefix rejection now emits
the warning and drops the failed assignment from the temporary environment
without aborting the command. Regression-guarded by
`readonly_temporary_prefix_reports_but_runs_command`.

**Where (original):** `userspace/oils/src/interp.rs`, the simple-command execution path
(~line 3009), the loop that rejects a readonly variable used as a temporary
assignment prefix:
```rust
for (k, _) in &assigns {
    let target = self.resolve_ref_name(k);
    if self.readonly.contains(&target) {
        self.emit_stderr(format!("osh: {target}: readonly variable\n").as_bytes());
        self.last_status = 1;
        return Flow::Next;   // ← wrong: bash still runs the command
    }
}
```

**What:** When a readonly variable appears as a *temporary assignment prefix*
to a command (`readonly x; x=1 echo mid; echo after`), bash prints
`x: readonly variable` to stderr but **still runs the command** (`echo mid`)
and continues (`echo after`), with the command's own exit status. osh instead
treats it as fatal-ish: it prints the error and returns without running the
command at all (prints nothing).

Note this is the *opposite* fatality direction from a **bare** readonly
reassignment (`readonly x=1; x=2`), which *is* fatal in a non-interactive
shell (shell exits, status 1) — that case was made correct on 2026-07-19
(see the arith/readonly assignment-fatality fix). Only the temp-*prefix*
case is wrong.

**Bash truth:** `readonly x; x=1 echo mid; echo after` → stdout `mid\nafter`,
rc 0 (stderr warning suppressed). osh → stdout empty.

**Proper fix:** on a readonly-prefix rejection, emit the warning to stderr but
skip *only* that failed assignment (do not export/set it) and continue
executing the command with the remaining prefixes and default status handling,
rather than short-circuiting with `Flow::Next`. Requires threading the
"this prefix failed" signal through to the command dispatch so the variable is
not applied while the command still runs. Add a test:
`readonly x; x=1 echo mid; echo after` → `("mid\nafter", 0)`.
