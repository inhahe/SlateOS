# B -> D: the console could answer as a virtual console does (`KDGKBMODE`, `KDGKBLED`)

**From:** Lane B (`userspace/agetty`). **To:** Lane D (`posix/src/ioctl.rs`).
**Filed:** 2026-10-15. **Status:** OPEN -- blocks shipping agetty on the
console; nothing breaks while it waits, since `userspace/getty` serves.

**In short:** agetty (util-linux 2.39.3's, ported -- `roadmap.md`, lane
B's agetty item) tells a virtual console from a serial line by asking the
line for its keyboard mode, `ioctl (fd, KDGKBMODE, &mode)`: a console
answers, a serial line refuses. SlateOS's C library refuses every
request it does not know with `ENOTTY`, so on the console agetty would
take the screen for a serial line -- `TERM=vt102` instead of `linux`,
the modes of a modem line, no screen clear, no Unicode input -- and log
people in to a terminal set up wrongly.

## What it asks

On the console's descriptor (the one `TIOCGWINSZ` already answers for):

| request | number | answer |
|---|---|---|
| `KDGKBMODE` | `0x4B44` | an `int`: `K_UNICODE` (3) for a console that sends UTF-8, `K_XLATE` (1) for an 8-bit one |
| `KDGKBLED` | `0x4B64` | a `char`: Scroll, Num and Caps Lock's lights in bits 0-2, their start-up states in bits 4-6 -- or `EINVAL` until the keyboard's lights are known (agetty then prints no hint) |

A pseudo-terminal and a serial line keep refusing both (`ENOTTY`), as
Linux's do.

## Also, lower down

`vhangup` answers `ENOSYS` (`posix/src/ioctl.rs`), so `agetty
--hangup` (`-R`) fails, logged, after its ten seconds' wait. Neither
Ubuntu's `getty@.service` nor its `serial-getty@.service` passes `-R`, so
this matters only to someone who asks for it; it is noted here so that
the refusal is known to be expected.

## What lane B then does

Ships `agetty` in place of `userspace/getty` (a request to lane D for the
image's manifest follows), and checks a console login on SlateOS
end to end.
