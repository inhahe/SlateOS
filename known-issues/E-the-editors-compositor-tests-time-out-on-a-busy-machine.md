### [E] The editor's compositor tests time out on a busy machine -- 2026-10-17

**Status:** OPEN

**In short:** four of the editor's tests start a real compositor on a thread
of their own and wait at most ten seconds for it to say which port it is
listening on. On 2026-10-17, while other lanes ran eight cargo builds, all four
gave up waiting and failed; run again a few minutes later on a quieter machine,
all four passed and the whole suite took 6 s. Nothing is wrong with the editor;
the failure says only that the machine was busy, which is the kind of
failure a test should not give.

**Where.** `apps/editor/src/main.rs`, module `against_the_real_compositor`:
`Desktop::start` waits `PATIENCE` (10 s) on `addr_rx.recv_timeout` for the
thread to send its address, and the thread first runs
`Compositor::new(1280, 800, 60)`, then `Server::bind("127.0.0.1:0")`. The
four tests (`the_editor_gets_a_window_from_the_real_compositor_over_a_real_socket`,
`a_frame_the_editor_draws_reaches_the_compositor`,
`a_keystroke_typed_at_the_compositor_arrives_in_the_editors_document`,
`when_the_editor_exits_the_compositor_reclaims_its_window`) each start one, at
once, as libtest runs them in parallel. `PATIENCE`'s comment says it is only
reached when something is wrong; under that load it was reached by a
compositor that was merely slow to build.

**Reproduce.** `cargo test -p editor --target x86_64-pc-windows-gnu` while
several other workspace builds run: the four fail with "the compositor thread
should have bound a port: Timeout" at `main.rs:3559`, and the run takes about
20 s instead of 6 s.

**The proper fix.** Find out what in `Compositor::new` takes seconds in a debug
build (four at once, in parallel), and either make it cheap -- the likely
candidates are font loading and the frame buffers -- or give the startup its
own bound, separate from the one-tick steps `PATIENCE` is for, loose enough
that only a hang trips it (design-decisions 855: a test asserts on the clock
only where it cannot count, and then loosely).
