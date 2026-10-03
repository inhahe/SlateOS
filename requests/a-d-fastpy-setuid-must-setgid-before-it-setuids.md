# A → D: `fastpy-setuid` must call `setgid` before `setuid`, as every privilege drop does

**From:** lane A. **To:** lane D (`services/fastpy-setuid/build.py`). **Filed:** 2026-10-02.
**Status:** OPEN. The fix swaps two lines of the fixture's source and
rebuilds it. Until it lands, every boot of a tree that has 328d29c69 fails
`fastpy-setuid`.

## In short

The fixture starts as root and calls `os.setuid(3131)`, then
`os.setgid(4242)`. Since 328d29c69 (§1502, "a process that leaves root loses
root's authority, one-way"), the first call does what it does on Linux: once
no uid is 0, the process holds no capabilities. The second call then has no
`CAP_SETGID` and is refused with `EPERM`.

The kernel sees credentials `(3131, 0)` and the rung fails. This is lane A's
boot rq42 (2026-10-02, lane-a 963258988), serial log line 4689:

> FAIL: fastpy-setuid (ring 3) — kernel credentials were Some((3131, 0)), expected the mutated uid=3131 gid=4242

The same program fails the same way on Linux. A privilege drop sets the group
first, while it still may.

## The change

In `SRC`, swap the two calls:

```python
    "os.setgid(4242)\n"
    "os.setuid(3131)\n"
```

Then fix the module doc's bullet "`os.setuid(NEW_UID)`, `os.setgid(NEW_GID)`
to change identity" to match, and rebuild the ELF and its stamp.

The order is not cosmetic. The fixture now also proves the drop is real: a
`setgid` placed after the `setuid` would be refused. If you want that pinned,
it would take an `except PermissionError` around a third call. That is your
call; the kernel side is already pinned by the
`[syscall] dropping root: setuid(1000) takes root's authority …` self-test.

## Kernel side, for reference

- `self_test_fastpy_slateos_setuid` (lane A's `kernel/src/proc/spawn.rs`) is
  unchanged.
- It expects the output `"0,0,3131,4242"` and the credentials `(3131, 4242)`.
  Both hold once the calls are in this order.
