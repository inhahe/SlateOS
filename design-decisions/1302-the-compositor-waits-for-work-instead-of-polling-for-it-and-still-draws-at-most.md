## 1302. The compositor waits for work instead of polling for it — and still draws at most once per refresh

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). Mechanism rather than policy, with no
user-visible fork, but it removes lane C's `IdleBackoff`, changes how §821's
slow key reaches its deadline, and works around a lane A bug, so it is written
down.

**In short:** between frames the compositor used to sleep for one frame and
then ask every client and device whether anything had happened. A request that
arrived just after it went to sleep waited up to a frame to be read, and a
desktop nobody was touching still woke sixty times a second to find nothing. It
now blocks until a client writes, the user does something, or something it is
waiting for comes due — and still draws at most once per screen refresh however
often it wakes. On the development host an idle desktop now wakes twice in the
time the old loop woke eighteen. On SlateOS one kernel-side gap (below) keeps
the full saving out of reach for now.

### What it waits on

`guiremote::WaitSet` (`gui/remote/src/wait.rs`, new) is the missing primitive —
the standard library can block on one socket, never on several. The server loop
puts in it the listener, every client, and whatever the display adds through
`Present::wait_on` (evdev's descriptors), and waits until the earliest of:

| deadline | from |
|---|---|
| the next frame, when one is owed (damage, or a pointer that moved) | the server: one refresh after the last frame shown, and not before `FrameStats::next_compose_at` |
| a slow key's threshold; a window's idle deadline | `Compositor::wake_at` |
| the next hotplug probe (1 s); the held key's next repeat | `Present::deadline` — DRM and evdev |

### The choices with two sides

1. **A wake knows what woke it, and the tick after reads only that.** A client
   the wait did not report is not read, and the listener is not asked for
   connections nobody is making. *For:* on SlateOS every socket call is a round
   trip to the network daemon; a loop that wakes on every mouse movement and
   reads every client each time would cost more than the polling it replaced.
   *Against:* two ways to run a tick. The narrowing lasts exactly one tick and
   defaults to "read everyone", so every caller that drives `Server::tick`
   without waiting — all the tests, both apps' real-compositor harnesses — is
   untouched.
2. **Frames are paced by the last frame shown, pointer-only frames included.**
   *For:* a 1000 Hz mouse would otherwise flip the screen a thousand times a
   second. *Against the alternative of a fixed grid*, which would not drift: the
   kernel retires a flip before the ioctl returns and sends no vblank event, so
   there is no grid to align to, and the old loop drifted the same fraction of a
   millisecond per frame.
3. **On Windows, event-select per wait rather than `WSAPoll`.** The host
   window's input is a message queue, `WSAPoll` cannot wait on one, and a GUI
   thread that does not pump for five seconds is marked not responding. So a
   wait registers each socket against one event, waits on it, a
   high-resolution timer and (for the host window) the message queue, and
   unregisters before returning — because a registered socket cannot be made
   blocking again, and `Socket::finish_write` does exactly that. Two system
   calls per socket per wait, on the development host only.
4. **A failed wait degrades to sleeping**, reported when it starts and when it
   stops, rather than stopping the desktop — the same rule as a failed client.
5. **`IdleBackoff` is deleted, not kept as a fallback.** Its whole job was
   waking less when nothing was connected; waiting subsumes it, and a desktop
   with clients — the case it deliberately did not cover — now idles too.
   §821's reasoning ("teaching the backoff to wake at a specific deadline is
   more machinery for the same result") no longer applies: the loop wakes at
   the slow key's exact deadline, which is `Compositor::wake_at`'s job.

### What SlateOS does not give it yet

- **A listening socket is never reported ready** -- *fixed 2026-09-26 by lane
  A's `851d9165b`; the workaround below is gone, and the compositor waits on
  its listener on SlateOS as everywhere.* The kernel asked the network
  daemon (`OP_POLL` on the listener id), the daemon answered only for
  connections, and `-1` became "nothing waiting". A server that trusted the
  wait would never accept anyone. So `guiremote::LISTENER_READINESS` is `false`
  on SlateOS, and there the loop asks its listener every tick and never waits
  longer than a frame — which keeps an idle SlateOS desktop waking at the frame
  rate, as before, while requests on existing connections are already read at
  once. `requests/f-a-poll-never-reports-a-connection-waiting-on-a-listening-socket.md`;
  `known-issues.md` → `[F]` 2026-09-25.
- **Sockets and evdev are poll-only in the kernel**
  (`kernel/src/ipc/multiwait.rs`): a set holding them is re-scanned on a
  0.5 → 20 ms backoff rather than parked. Correct, cheaper than the old loop,
  and fixed for every caller the day those families push readiness — which is
  the argument for declaring a wait at all.

### How it is held

`an_idle_desktop_does_not_wake_until_something_happens` (two ticks in 300 ms,
the old loop eighteen), `a_request_wakes_the_loop_and_is_answered_without_a_timer`,
`the_loop_shows_a_frame_when_only_the_pointer_moved` (the pointer frame held for
its slot is shown without any further input),
`a_fast_pointer_is_shown_once_per_refresh_at_its_latest_position`, the
`next_wake` tests one deadline at a time, and `WaitSet`'s own suite on both
Windows and Linux, including that a window message wakes a GUI thread and that
a socket can be made blocking again after a wait.

### How to reverse

`Server::run_with` is one loop; putting `std::thread::sleep(interval)` back in
place of `wait_for_work` restores polling, and nothing else depends on the wait.
