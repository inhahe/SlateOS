### [D] B-D-PRINTF-DROPPED-EVERYTHING-PAST-4096-BYTES — 2026-09-26 — FIXED

**Where:** `posix/src/printf.rs` — `_printf_impl`, `_fprintf_impl`,
`_dprintf_impl`, and so `printf`, `fprintf`, `dprintf` and their `v` forms.

**What it was.** Each formatted into a 4096-byte stack buffer and then wrote
`min(n, 4096)` bytes while returning `n`. One call producing more than 4096
bytes — a long line, a report, a JSON document — lost everything past the
4096th byte, and its return value said all of it had been written, so nothing
downstream could notice.

**Fix.** The engine's output (`FmtOutput`) has a sink: a bounded buffer for the
`snprintf` family, as before, or a stream or descriptor, which receives each
full buffer as it fills, and the tail at the end. The three functions now
stream through the same 4096-byte buffer, write everything, and return -1 if
the stream refuses a write. `dprintf` also retries short writes, which it did
not. Host tests drive the sink with outputs of 4095, 4096, 4097, 8192 and
10,000 bytes; `services/ctest-printf-streams` checks the byte count at the far
end of a pipe at ring 3 (its rung is requested of lane A).
