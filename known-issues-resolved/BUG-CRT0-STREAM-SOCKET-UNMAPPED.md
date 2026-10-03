### BUG-CRT0-STREAM-SOCKET-UNMAPPED. crt0 `retrieve_initial_fds` has no STREAM_SOCKET→UnixStream mapping, so inherited unix-domain sockets are mislabeled as `File` across `posix_spawn`/`exec` fd inheritance — 2026-07-23 — ✅ RESOLVED 2026-07-27

**Where:** `posix/src/crt.rs` — `retrieve_initial_fds` maps handle_type FILE/PIPE/TCP_SOCKET/UDP_SOCKET/CONSOLE/EVENTFD but omits STREAM_SOCKET (unix-domain / `AF_UNIX` stream, handle_type 6). Any inherited unix-domain socket fd is reconstructed as a plain `File` entry in the child's userspace fd table.

**Impact:** latent — no current self-test inherits a unix-domain socket across spawn/exec, so it hasn't manifested. It would surface if/when a native process passes an `AF_UNIX` connection to a child: the child's fd would route through the File path instead of the stream-socket path (wrong `read`/`write`/`recv` semantics, likely EBADF or silent misbehavior).

**Fix:** add the STREAM_SOCKET(6) arm to `retrieve_initial_fds`, reconstructing a `UnixStream` fd entry, mirroring the TCP/UDP socket arms. Also audit `kind_to_handle_type` / `build_fd_map` on the send side to confirm unix-domain sockets serialise with handle_type 6.

**✅ RESOLVED 2026-07-27.** The send side was already correct
(`kind_to_handle_type`: `HandleKind::UnixStream => fd_handle_type::STREAM_SOCKET`),
so the defect was purely the missing receive arm.

Rather than just adding the arm, the **root cause — two hand-maintained
inverse mappings living in different files** — was removed: the reverse
direction is now a single `spawn::handle_type_to_kind()` sitting directly
beside its forward counterpart `kind_to_handle_type()`, and
`crt::retrieve_initial_fds` calls it instead of open-coding its own `match`.
A mapping can no longer be taught to one direction and not the other.

Pinned by four unit tests in `posix/src/spawn.rs`:
`round_trips_for_every_transferable_kind` (every transferable `HandleKind`
survives serialise→reconstruct), `unix_stream_is_not_rebuilt_as_a_plain_file`
(the regression itself), `tcp_listener_collapses_to_tcp_stream` (pins the one
*deliberate* lossy case — `TcpListener` shares the `TCP_SOCKET` wire type and
returns as `TcpStream`), and `unknown_handle_type_falls_back_to_file` (an
unrecognised wire type must not panic, so a newer parent can't wedge an older
crt0). Full `posix` suite green (19 996 tests) and boot green.
