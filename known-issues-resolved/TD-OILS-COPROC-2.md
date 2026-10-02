### TD-OILS-COPROC. `osh` has no `coproc` support at all — RESOLVED 2026-07-19

**RESOLVED 2026-07-19.** Implemented — see the resolved TD-OILS-COPROC
entry near the top of this file for the shipped design and remaining minor
limitations. Note the "why deferred" reasoning below turned out to be
over-cautious on one point: a full `Box<dyn Read>`/`Box<dyn Write>`
generalisation of *both* fd tables was **not** needed. `std::io::pipe()`
(stable 1.87, cross-platform) plus a *dedicated* live read table
(`coproc_read_fds`) and reuse of `open_write_fds` for the write end kept
the change to ~4 read-resolution points; the body runs on an OS thread
with its own `Shell` (not an OS process), so no `Child`/reaping subsystem
was required. The concurrency concern was valid and is satisfied by the
real OS pipes + background thread. Original analysis retained below.

**Where (original):** would touch `lexer.rs`/`parser.rs`/`ast.rs` (recognise the `coproc`
reserved word and parse both `coproc [NAME] simple-command` and
`coproc [NAME] { compound }` forms), `interp.rs` (a new async-process launch +
the `open_fds`/`open_write_fds` fd tables + `COPROC`/`NAME` array and
`COPROC_PID`/`NAME_PID` variables), `unparse.rs`.

**What:** `coproc` is not implemented. `coproc cat` runs `coproc` as an external
command (`osh: coproc: command not found`); the block form `coproc NAME { … }`
is a hard parse error (`unexpected reserved word '}'`) because the lexer/parser
do not treat `coproc` as a keyword. bash creates an asynchronous coprocess whose
stdin/stdout are connected to the shell by two pipe fds exposed as an array
(`${COPROC[0]}` = read end, `${COPROC[1]}` = write end, or `${NAME[…]}` for the
named form), plus `COPROC_PID`. The shell and the coprocess run **concurrently**
and exchange data by streaming through those live pipe fds.

**Why deferred (root cause):** osh's host-interp fd model is not backed by live
OS pipe fds. Input fds are in-memory `RefCell<Cursor<Vec<u8>>>` buffers
(`open_fds`), write fds are `Arc<File>` (`open_write_fds`), and the closest
async construct — process substitution — is a **temp-file approximation that
runs each side to completion** rather than streaming (see TD-OILS22). A coproc's
defining semantics are *concurrent bidirectional streaming*: you write a line to
`${COPROC[1]}` and read the response from `${COPROC[0]}` while the coprocess is
still alive. A temp-file/cursor model cannot express this without deadlocking or
serialising the two sides — so a temp-file "coproc" would be a broken hack, not
a fix. Implementing it correctly requires generalising both fd tables to hold
live readable/writable handles (`Box<dyn Read>` / `Box<dyn Write>`, e.g. a
`ChildStdout`/`ChildStdin` from `std::process` with `Stdio::piped()`), threading
those through the `read`/`write`/redirect paths, and tracking the `Child` for
`COPROC_PID` and reaping — the same live-pipe-fd subsystem TD-OILS22 needs.

**Proper fix:** build the live-pipe-fd subsystem (shared with TD-OILS22). Then
for the external-command form, spawn the command with `Stdio::piped()` on stdin
and stdout, register the two pipe ends as live fds in the generalised fd tables,
publish their fd numbers into the `COPROC`/`NAME` array and the pid into
`COPROC_PID`/`NAME_PID`, and let subsequent `read`/`echo` on those fds stream to
the live child. The compound-body form (`coproc NAME { … }`) additionally needs
osh's own interpreter to run the body concurrently (a subshell process or an
OS-thread with its own `Shell`), which is the harder half. `coproc` is one of
the least-used bash constructs, so this is low priority relative to the
live-pipe-fd work it depends on.
