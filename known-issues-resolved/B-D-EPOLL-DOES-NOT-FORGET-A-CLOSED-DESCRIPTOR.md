### [D] B-D-EPOLL-DOES-NOT-FORGET-A-CLOSED-DESCRIPTOR — 2026-09-25 — FIXED 2026-09-26

**Fix.** As the proper fix below describes. An entry records the target's
`(kind, handle)` at `EPOLL_CTL_ADD`; readiness comes from that; ADD, MOD and
DEL find an entry by `(descriptor, kind, handle)`, as upstream's `ep_find` does
by `(file, fd)`; and `close()` and `dup2()`'s eviction call
`epoll::forget_file` once a file's last descriptor is gone, as upstream's
`eventpoll_release` does. `EPOLL_CTL_DEL` of a closed descriptor is `EBADF`
again, as upstream — the deviation that let it succeed existed only for this
bug. Tests: `test_epoll_forgets_a_closed_descriptor`,
`test_epoll_entry_follows_the_file_not_the_number` (the dup case, the reused
number, and DEL's `EBADF`), `test_epoll_forgets_a_file_dup2_evicts`.

**Where:** `posix/src/epoll.rs` — `EpollEntry` is keyed by descriptor number,
and `compute_revents` looks the number up at wait time; `posix/src/file.rs`
`close()` does not tell epoll.

**What.** Linux removes a file from every epoll interest list when its last
reference is closed (`eventpoll_release`, fs/eventpoll.c). Here the entry
stays, with two results:

- `epoll_wait` reports it as `EPOLLERR | EPOLLHUP` on every call from then
  on, with the caller's `data` — typically a pointer to the state the caller
  freed along with the descriptor. An event loop that closes before it
  deletes (legal, and common, on Linux) is handed a dangling pointer.
- If the number is reused by a later `open`, the stale entry reports the
  *new* file's readiness under the old registration's `data` and mask, and
  `EPOLL_CTL_ADD` of the new file is refused with `EEXIST` because the number
  is "already present". Linux keys an entry by (file, descriptor), so the new
  file is a new entry.

`epoll_ctl`'s one kept deviation — `EPOLL_CTL_DEL` of a closed descriptor
succeeds, where Linux says `EBADF` — exists only to let a caller clean up after
this.

**Reproduce.** `e = eventfd(0,0); ADD e with data p; close(e); epoll_wait` →
1 event, `EPOLLERR|EPOLLHUP`, data `p` (Linux: none). Then `f = open(...)`
returning the same number, `ADD f` → `EEXIST` (Linux: 0).

**Proper fix.** Key entries by the open file: record the target's
`(kind, handle)` at `EPOLL_CTL_ADD`, compute readiness from that rather than
from the number, and treat `(descriptor, kind, handle)` as the entry's
identity, as upstream's `epitem` is `(file, fd)`. In `close()`, once the
handle has no descriptor left (`fdtable::is_handle_referenced`), purge every
entry naming it from every instance. A `dup` then keeps the entry alive as it
does upstream, and the `EPOLL_CTL_DEL` deviation can go.
