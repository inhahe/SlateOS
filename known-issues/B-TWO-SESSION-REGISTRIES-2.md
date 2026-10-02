## B-TWO-SESSION-REGISTRIES (lane B, 2026-09-10) — original entry, 2026-09-10

**In short:** this system keeps two separate lists of who is logged in. The
programs that *show* you the list read one of them; the daemon whose job is to
*manage* sessions keeps the other. Neither knows the other exists, so
`loginctl list-sessions` cannot show you a session `logind` is managing.

### The two

| | writes | reads |
|---|---|---|
| `/run/sessions/<pid>` | `userspace/su` (main.rs:333, removed at :359) | `userspace/who` (main.rs:462), `userspace/loginctl` (main.rs:143) |
| `Daemon::sessions`, in memory | `userspace/logind`, served over the service bus | nothing outside the daemon |

`su` writes `user=`/`tty=`/`host=`/`time=`/`pid=` lines, assembled as bytes
because a tty name is bytes. `logind` holds a `HashMap<String, Session>` and
answers over `libservicebus`. It persists nothing.

### What it costs

`loginctl` is nominally logind's client and never speaks to it. It lists the
contents of a directory that only `su` populates, so:

* a session `logind` created is invisible to `loginctl` and to `who`;
* a session `su` created is invisible to `logind`, so its `max_sessions` limit,
  its idle timeout and its inhibitor logic all see a machine with fewer
  sessions on it than there are;
* `su` removes its file by pid on exit, so a `su` that dies without running its
  cleanup leaves a session record that outlives the process, and nothing
  reconciles it.

### The near-miss, which is the reason this entry exists

`loginctl` said `/run/sessions` and `logind` said `/run/systemd/sessions`, and
the obvious reading of that is "the client is pointing at the wrong directory,
make it agree with the session manager". **That fix would have broken
`loginctl` completely.** `logind`'s `SESSION_DIR` was used exactly once, to
`create_dir_all` it at start-up, and never written to — so pointing `loginctl`
there would have moved it from a directory with sessions in it to one that is
always empty, and `loginctl list-sessions` would have gone from partly right to
always empty. The divergence was real and the direction was the opposite of what it
looked like.

Fixed in the same change as this entry: `logind` no longer creates those five
empty directories. An empty `/run/systemd/sessions` answers "no sessions" to
anyone who looks, which is worse than its absence.

### What the fix looks like

One of the two has to become the source. Both are defensible and it is not a
decision to make from inside `loginctl`:

1. **`loginctl` asks `logind` over the bus**, as it does on Linux. Correct, and
   the largest change: `loginctl` currently has no bus client at all.
2. **`logind` persists its sessions to `/run/sessions`**, the path the existing
   readers already use, and `su` registers with `logind` instead of writing the
   file itself. Smaller, and makes `who` correct for free.

Either way `su` must stop being a session registry of its own. Until then the
two lists drift, and each is right about a different half of the machine.
