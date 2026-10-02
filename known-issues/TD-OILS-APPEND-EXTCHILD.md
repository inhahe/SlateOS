### TD-OILS-APPEND-EXTCHILD. External (MSYS) children fail to write to an `>>file` append redirect on the Windows host (`echo: write error: Bad file descriptor`) — OPEN (host-only artifact) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `open_out` (~12073) uses
`OpenOptions::write(true).append(true)`. On Windows Rust maps `write+append`
to `FILE_GENERIC_WRITE & !FILE_WRITE_DATA` (append-only access). osh's own
builtins write to such a handle fine, but when that handle is inherited by an
**external MSYS/Cygwin child** as its stdout/stderr, the Cygwin fd layer rejects
it (it probes for `FILE_WRITE_DATA`) → the child prints `write error: Bad file
descriptor` and produces no output. Affects `extcmd >>f`, `extcmd >>f 2>&1`,
`extcmd &>>f`, `extcmd 2>>f`, i.e. any external command whose target is an
append redirect. Truncating redirects (`>f`, `>f 2>&1`, `&>f`) are unaffected.

**What:** bash appends the child's output to the file normally. osh loses it on
the Windows host only.

**Why host-only:** on the SlateOS (unix/musl) target, `OpenOptions::append`
yields an ordinary writable, inheritable fd and children append correctly — this
divergence cannot reproduce there. The failure is purely the Windows
append-only-handle × MSYS-child-inheritance interaction. Reproducing bash's
behavior on the host would mean opening append targets with full `FILE_WRITE_DATA`
+ manual seek-to-end, which sacrifices atomic-append semantics for a test-only
platform. Parked; comparison testing of append+external cases must run on the
target, not the host.
