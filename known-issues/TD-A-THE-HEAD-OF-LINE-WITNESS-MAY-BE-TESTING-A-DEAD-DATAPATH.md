## TD-A-THE-HEAD-OF-LINE-WITNESS-MAY-BE-TESTING-A-DEAD-DATAPATH (lane A, 2026-09-14) -- SEE CORRECTION

**In short:** the witness written for `D-NETSOCK-SYNC` fails at its first
`accept`, 64 retries, `InternalError` every time. That is not a race and the
retry added for one was the wrong fix. The likely cause is that
`net::socket`'s datapath does not work at all in the default configuration,
which would make it the wrong layer to build this witness on.

**What the diagnostics bought.** The first failure was a bare `InternalError`
with no indication which of six setup calls produced it -- six bare `?`s. With
per-step reporting it reads:

    [netsock]   FAIL: first accept never became ready in 64 spins: InternalError

`InternalError`, not `WouldBlock`. A connection that is merely not ready yet
returns `WouldBlock`; this fails for a reason, persistently. The retry fix
addressed a race that was not happening.

**Why the layer is suspect.**

* `net.userspace` now defaults **on** (lane A flipped it), so the persistent
  userspace netstack daemon claims the NIC -- confirmed this boot:
  `[spawn] persistent netstack daemon registered net.stack (pid 459)`.
* `net::socket`'s own self-tests are a stream-socket state-machine check and a
  server-socket test covering `bind`/`listen`/**empty-backlog** `accept`, port
  reporting, and `connect`-on-a-listener rejection. **None of them completes a
  real connect-and-accept carrying data.**

So there is no evidence that a real handshake through `net::socket` has ever
worked under the daemon-owned-NIC default, and this witness is the first thing
to ask for one.

**The tension this creates, which is the part to decide rather than patch.**
932 wants a second witness for the head-of-line fix. The scoping note for that
fix says the lock in question is `socket.rs`'s `with_stream_conn` over
`SessionRef::Shared`, reachable only through the fd layer -- so a witness built
on `NetstackConn` would bypass the mutex it exists to exercise and pass either
way. But if the fd layer has no datapath under the default config, a witness
there cannot run at all.

Both cannot be true and useful at once. Either the fd layer does work and
something narrower is wrong with this test, or it does not, and the property
needs proving somewhere the code actually runs -- which may mean proving it
about `with_stream_conn` directly rather than end-to-end through sockets.

**Do not "fix" this by moving the witness to `NetstackConn`** without settling
that: it would produce a green line about a lock the test never takes, which is
937 substitution and strictly worse than the current honest red.

**CORRECTION, same day, before anyone acts on the above.** The premise of this
entry -- that the daemon owning the NIC leaves `net::socket` without a datapath
-- is **wrong**, and I published it twenty minutes after forming it.

`net::socket` is not an in-kernel stack competing with the daemon. Its own module
doc says it is built on `NetstackConn`, "one SHM ring + one daemon TCP
connection", and that socket creation is **only offered when `net.userspace` is
set**. It is the daemon client. The switch being on is the condition that makes
that layer live, not the thing that kills it.

So the datapath is not dead and the earlier reasoning was backwards: I saw
"daemon claimed the NIC" and "net::socket fails" and supplied a mechanism
connecting them without reading what `net::socket` is.

**What the evidence actually supports, stated narrowly this time.**

* `netstack_client`'s own test completes listen/accept/data over loopback using
  the raw `NetstackConn` API with explicit ids -- so the daemon's accept works.
* `net::socket::accept` is a different wrapper over that, carrying the
  `SOCKET_TABLE` and per-socket `Arc<Mutex<SocketInner>>` discipline this
  witness exists to exercise.
* Every `net::socket` self-test that passes covers a state machine, port
  reporting, or an **empty-backlog** accept. None completes a real accept.

So the narrow claim that survives is: **`net::socket::accept` has never been
shown to complete a real connection, while `NetstackConn::accept` has.** That is
a much smaller statement than the one above and it points at the wrapper rather
than at the transport.

**What this does NOT change.** Moving the witness to `NetstackConn` is still
wrong, and now for a sharper reason: `NetstackConn::accept` demonstrably works,
so a witness there would pass -- while taking none of the locks the head-of-line
fix is about. It would be a green line about the one layer known to be fine.

**Method note, since this is the second wrong diagnosis in one evening.** Both
came from reasoning about a subsystem I had not read the first page of. The
module doc that settles it is sixteen lines long and was three minutes away.

**Narrowed by reading, 2026-09-14 (third pass, and the first two were wrong).**

`net::socket::accept` maps the daemon's reply directly:

    if res == ERR_WOULD_BLOCK { return Err(WouldBlock); }   // empty backlog
    if res != 0 { return Err(InternalError); }              // -1 unknown listener / id install failed

So `InternalError` is the daemon answering **"unknown listener"** -- a permanent
condition, which is exactly why 64 retries failed identically and why the retry
fix could never have helped.

Four facts, each checked rather than inferred:

1. `accept` does take the locks this witness exists to exercise -- `inner.lock()`
   and then `arc.lock()` on the shared session, held across the daemon call. The
   layer is right; only my two explanations were wrong.
2. `listen` **succeeded**. The witness's per-step macro names any failing setup
   call, and it printed nothing for `listen` -- which is the first thing those
   diagnostics have actually settled.
3. `listen` issues `OP_LISTEN` with a **constant**, `LISTENER_ID`, not a
   per-socket id (`socket.rs` line ~1327).
4. `netstack_client`'s own loopback test also uses `LISTENER_ID`, on its own
   session, and runs **earlier in the same boot** -- its success line appears
   about ten lines above this witness's failure.

**Hypothesis, explicitly untested:** the daemon keys listeners by id, the id is a
constant shared by every session, and a listener registered by the earlier test
collides with or supersedes this one -- so `listen` returns success while
`accept` finds no listener under that id on this session.

**Deliberately not acted on tonight.** Two diagnoses have already been published
and withdrawn this evening, both formed from symptoms plus a plausible mechanism
rather than from reading. The next step is to read the daemon's listener table
and confirm or kill the hypothesis before changing anything -- not to try a third
fix and see whether the boot goes green.

**Fourth pass, and this one is evidenced rather than hypothesised.** The
constant-id idea was right for the wrong reason; here is the structure.

| fact | where |
|---|---|
| the daemon holds **one** `RingSession`, not a table of them | `services/netstack/src/main.rs:2270` |
| listeners live **inside** that session, keyed by the `OP_LISTEN` `conn_id` | same file, 3167 and 3328 |
| there are **4** listener slots | `MAX_LISTENERS`, 3069 |
| `net::socket::listen` issues `OP_LISTEN` with a **constant** id, 100, for *every* listening socket | `socket.rs:65`, `socket.rs:1327` |
| the boot self-test uses 100 for its own listener, earlier in the same boot | `netstack_client.rs:1971` |

So "keyed per session" and "keyed globally" are **the same thing here**, because
there is only ever one session. My previous note killed the collision hypothesis
on the grounds that the table is per-session; that was reading the type and not
the instance.

**What this makes suspect, in lane A's own code.** `socket.rs:64` documents the
id as "unique within that session". That is literally true and practically
misleading: with one session for the whole machine, a *constant* id means **two
listening sockets cannot coexist**, and any listener the boot self-tests leave
behind occupies the same slot. A second `listen()` is not a second listener.

**Still not fixed tonight, and the reason has changed.** Earlier it was "stop
guessing"; now it is ownership. The daemon half is `services/netstack`, which
lane A must not write. The kernel half -- a constant where a per-socket id
belongs -- is lane A's and is a real defect independent of this witness: it is
why the witness cannot get a listener, and it would equally stop any two
programs listening at once.

**Next step, concretely:** confirm whether the earlier self-test releases
listener 100, and whether `OP_LISTEN` on an occupied id replaces, rejects, or
silently succeeds -- `listen()` returned success here while `accept` found no
listener, and exactly one of those is lying.

**ROOT CAUSE, 2026-09-14, and it is not the listener id.** `net::socket`
supports exactly **one socket at a time**. The constant id below is real but
secondary: unique ids would be wiped just the same.

The chain, each link read rather than inferred:

1. `create_kind` calls `NetstackConn::open()` for **every** socket
   (`socket.rs:356`).
2. `NetstackConn::open()` calls `shm::create(...)` -- a **new SHM ring per
   socket** (`netstack_client.rs:158`).
3. The daemon holds **one** `RingSession` (`services/netstack/src/main.rs:2270`),
   whose doc calls `handle` "the SHM handle of the **currently-mapped** ring".
4. On seeing a different handle it resets everything (same file, 2594):

        if session.handle != handle {
            session.teardown(me);
            session.conns = RingConns::new();
            session.listeners = Listeners::new();
            ...
        }

   The comment above it states the intent plainly: *"A different handle (or a
   fresh start) opens a new session: tear down any prior mapping, map the new
   region, and reset the connection table."*

**So the witness's failure is fully explained.** `create(srv)` takes ring A;
`listen(srv)` registers listener 100 on A and returns 0 -- genuinely, which is
why no FAIL line printed. `create(c1)` takes ring B; the first daemon round-trip
on B wipes A's listeners. `accept(srv)` arrives on A, the daemon switches back,
wipes again, and answers *unknown listener*. Exactly the observed
`InternalError`, 64 times, permanently.

**Why no existing test caught it.** Every `net::socket` self-test uses one
socket, or two that never both need daemon state alive at once. The server-socket
test accepts from an **empty backlog** -- which needs no live peer. This witness
is the first thing in the tree to require two sockets' daemon state
simultaneously, and it is the first to fail.

**What this means for `D-NETSOCK-SYNC`.** The head-of-line fix is about two
accepted connections sharing a listener's session progressing independently.
That property cannot be exercised through `net::socket` at all while a second
socket destroys the first's session -- not because of a race or an id, but
because the transport underneath does not hold two sockets' state. The witness
was not wrong to be written; it was the first probe of a layer nobody had
probed.

**Ownership.** The daemon half is `services/netstack` (not lane A's). The kernel
half -- one ring per socket, against a daemon that holds one -- is
`net::socket`'s, and is lane A's. Neither is a small change, and the right shape
(one shared ring for all sockets, or a daemon that maps several) is a design
decision rather than a patch.

**`D-NETSOCK-SYNC` still has ONE witness.** Three boots have now failed on this
test and none of them said anything about the property.
