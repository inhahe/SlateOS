### TD-OILS-READ-T0-POLL. `read -t 0` readiness on a live pipe/tty is exact only on Windows — MINOR 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `input_available_now` and the
platform probes `stdin_readable_now` / `pipe_reader_readable_now` /
`handle_readable_now` (Windows FFI: `PeekNamedPipe`/`WaitForSingleObject`).

**What:** `read -t 0` (non-consuming availability poll, exit 0 iff a read would
not block) is now implemented. Deterministic sources — here-strings/here-docs,
file redirects, fd cursors (`-u N` / `<&N`), `/dev/null`, and a persistent
`exec <` cursor — return the correct answer on all targets. A *live OS pipe or
interactive terminal* is probed exactly only on the **Windows host** (via
`PeekNamedPipe` for pipes and a zero-timeout wait for the console). On
**non-Windows targets (including SlateOS)** the fallback treats a non-tty
inherited stdin as ready and only counts already-buffered pipe bytes — so a
bare OS-pipe peek and an interactive-tty keystroke poll are approximated.

**Proper fix:** wire a `poll(2)`/`select(2)` (or SlateOS-native readiness
syscall) probe into `stdin_readable_now`/`pipe_reader_readable_now` for
`cfg(not(windows))`, mirroring the Windows path. `oils` is currently std-only
(no `libc`/`nix` dep); this needs either raw `poll` FFI or a small dependency.

**Note (inherent race, not a bug):** on a *pipeline* (`printf … | { read -t 0; … }`)
the upstream stage writes concurrently, so `read -t 0` in the downstream stage
may legitimately poll before the bytes arrive and report "would block" — a
correct outcome of point-in-time poll semantics. bash exhibits the same race
but its tiny builtin writes usually land first. Not fixable without changing
pipeline scheduling, and arguably should not be.
