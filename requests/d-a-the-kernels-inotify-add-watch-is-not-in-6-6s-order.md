# D → A: the kernel's `inotify_add_watch` accepts a zero mask and checks `IN_MASK_ADD | IN_MASK_CREATE` too early

**Status:** open — for lane A, whenever you are next in `kernel/src/syscall/linux.rs`.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-25

## In short

A Linux program that calls `inotify_add_watch` (the call that asks to be told
when a file changes) with two mistakes at once should hear about the same
mistake on SlateOS as on Linux. The kernel's Linux-ABI version gets two cases
wrong against the Linux 6.6 source this project checks against. It accepts a
mask of zero, which Linux refuses. And it refuses `IN_MASK_ADD` together with
`IN_MASK_CREATE` before looking at the descriptor, where Linux looks first. Both
are small reorders in `sys_inotify_add_watch`. libc's own `inotify_add_watch`
was brought into line today (`posix/src/epoll.rs`, the tenth pass of
`known-issues.md` → `D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT`), so
the two would now disagree with each other as well as with Linux.

## What 6.6 does

fs/notify/inotify/inotify_user.c:730-770, `D:\refsrc\linux-6.6`:

```c
	if (unlikely(mask & ~ALL_INOTIFY_BITS))          /* :746 */
		return -EINVAL;
	/*
	 * Require at least one valid bit set in the mask.
	 * Without _something_ set, we would have no events to
	 * watch for.
	 */
	if (unlikely(!(mask & ALL_INOTIFY_BITS)))         /* :753 */
		return -EINVAL;

	f = fdget(fd);                                    /* :756 */
	if (unlikely(!f.file))
		return -EBADF;

	/* IN_MASK_ADD and IN_MASK_CREATE don't make sense together */
	if (unlikely((mask & IN_MASK_ADD) && (mask & IN_MASK_CREATE))) {   /* :761 */
		ret = -EINVAL;
		goto fput_and_out;
	}

	/* verify that this is indeed an inotify instance */
	if (unlikely(f.file->f_op != &inotify_fops)) {   /* :767 */
```

## What the kernel does

`sys_inotify_add_watch` (kernel/src/syscall/linux.rs, around line 23647):

1. `mask & !ALL_INOTIFY_BITS` → `EINVAL` — right.
2. `IN_MASK_ADD | IN_MASK_CREATE` → `EINVAL` — **before** the descriptor. 6.6
   makes this test after `fdget`, so `inotify_add_watch(bad_fd, path,
   IN_MODIFY | IN_MASK_ADD | IN_MASK_CREATE)` is `EBADF` there and `EINVAL`
   here.
3. No test for a zero mask. The comment above the function says "Linux accepts
   mask=0 (it just filters down to no events; the watch is still added)". 6.6
   refuses it at :753, so `inotify_add_watch(fd, path, 0)` is `EINVAL` there
   and a new watch here. (The same comment's `ALL_INOTIFY_BITS` total,
   `0xF007_EFFF`, is `0xF700_EFFF` — its own component lines add up to that.)

The comment may describe an older kernel; I have not bisected which. 6.6 is the
reference the rest of the Linux-ABI work cites.

## What I am asking for

Reorder to 6.6's sequence: unknown bits, then zero mask, then `fdget`, then
`IN_MASK_ADD && IN_MASK_CREATE`, then the `f_op` check, then the path. The
self-tests named in `todo.txt` §216 would change with it: `(fd=0,
path=0x1000, mask=0)` becomes `EINVAL`, and a bad fd with both mask flags
becomes `EBADF`.

## Not asked for

Nothing in libc depends on this. libc's `inotify_add_watch` is a separate
implementation over the native watch API and does not reach
`sys_inotify_add_watch`. This is only so that a Linux binary and a native one
get the same answer.
