### [D] TD-D-INOTIFY-SHIM-IGNORES-ITS-CONTROL-FLAGS — 2026-09-25 — PARTIAL 2026-09-26 (four of six done)

**Done 2026-09-26.** `IN_ONLYDIR` refuses a non-directory with `ENOTDIR`, after
the missing-path `ENOENT`, as `LOOKUP_DIRECTORY` does. `IN_MASK_CREATE` refuses
an existing watch with `EEXIST`, and `IN_MASK_ADD` ORs the new mask (and
oneshot) into the old — `watch_after_add`, a port of
`inotify_update_existing_watch`, tested on the host. `IN_ONESHOT` retires the
watch after its first event with an `IN_IGNORED`, in the event pump
(`retire_after_first_event`, tested on the host; upstream is
inotify_fsnotify.c:132). **Still open:** `IN_DONT_FOLLOW` and `IN_EXCL_UNLINK`,
as the proper fix below says, and a ring-3 check of the four done ones — the
host cannot reach `inotify_add_watch`'s existing-watch branch at all, because
`stat_self` has no host double and every path is missing there.

**Where:** `posix/src/epoll.rs`, `inotify_add_watch` and the event pump
(`IN_KNOWN_EVENTS`'s doc comment says so outright).

**What.** The mask is validated as Linux 6.6 validates it (tenth NULL-pointer
pass), but of the control flags only the validation is real. `IN_MASK_ADD`
should OR the new mask into an existing watch's and replaces it instead;
`IN_MASK_CREATE` should refuse an existing watch with `EEXIST`; `IN_ONLYDIR`
should refuse a non-directory with `ENOTDIR`; `IN_ONESHOT` should remove the
watch (queuing `IN_IGNORED`) after its first event; `IN_DONT_FOLLOW` should
not follow a final symlink; `IN_EXCL_UNLINK` should drop events for unlinked
children. All six are accepted and ignored, which is a silent failure: a
caller asking for "only if it is a directory" gets a watch on a file.

**Proper fix.** The first four are libc's to do: the existing-watch search in
`inotify_add_watch` already finds the slot `IN_MASK_ADD`/`IN_MASK_CREATE`
need, `stat_self` already returns whether the path is a directory, and the pump
can retire a oneshot watch as it queues the event. `IN_DONT_FOLLOW` needs an
`lstat`-based resolution. `IN_EXCL_UNLINK` needs the kernel's watch to know
which children are unlinked; check what lane A's `fs::notify` records before
deciding whose change that is.
