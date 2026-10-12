# B → D — `klogctl` returns the kernel's JSON lines, where every caller expects syslog(2) records

**Status:** OPEN — for lane D (`posix/src/unistd.rs`, `klogctl`). It blocks lane
B's port of util-linux's `dmesg`; nothing else is broken by it today.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-08

## In short

`klogctl(SYSLOG_ACTION_READ_ALL, buf, len)` -- and `READ`, `READ_CLEAR` --
hands its caller the kernel's log exactly as `SYS_LOG_READ` returns it: one
JSON object per line. On Linux the same call returns syslog(2) records, the
format `dmesg` has parsed since 1993:

```text
<6>[    0.000000] Linux version 6.6.87 ...
<4>[    1.234567] ACPI: _OSC evaluation failed
```

-- `<` the priority (facility * 8 + level) `>`, `[` seconds `.` microseconds
since boot `]`, a space, the message, `\n`. Every program that calls `klogctl`
reads that and nothing else: util-linux's `dmesg -S` (and plain `dmesg` when
`/dev/kmsg` cannot be opened, which on SlateOS is always), busybox's `dmesg`,
sysklogd's `klogd`, rsyslog's `imklog`. Given JSON, util-linux's `dmesg` finds
no `<` and no `[` and prints each JSON object as the message text.

Lane B is porting util-linux 2.39.3's `dmesg` (the standalone `userspace/dmesg`
was written from the manual and parses SlateOS's JSON itself). A faithful port
reads `klogctl` as Linux defines it, so until `klogctl` speaks syslog(2), the
port would print JSON on SlateOS and cannot replace the standalone.

## What is asked

| | What changes | What a user sees |
|---|---|---|
| A (lane B's suggestion) | `klogctl`'s three read actions render each JSON entry as a syslog(2) record: `<%d>[%5lu.%06lu] %s\n` from its priority, its boot-relative timestamp and its message; `SIZE_BUFFER` / `SIZE_UNREAD` count the rendered bytes | `dmesg` (any port of it) and any `klogd` work as on Linux |
| B | Leave it | the util-linux port cannot ship; `userspace/dmesg` stays the image's `dmesg` |

Details that matter to the readers, all as Linux has them:

- The timestamp is `[%5lu.%06lu]` -- seconds right-aligned in five columns, so
  early records read `[    0.123456]` -- seconds and microseconds since boot.
- A message with embedded newlines is one record; continuation lines carry no
  prefix of their own. A record ends with exactly one `\n`.
- `READ_ALL` does not consume; the clear floor this file already keeps is the
  start, as now.
- A buffer too small for everything returns the *newest* records that fit,
  whole -- Linux drops from the front.

The kernel's entries carry what this needs: `ts` (the boot-relative time),
`level` (a name -- `info`, `warn`, ...), `msg`, and `service`. There is no
facility, so every record is `kern` (facility 0) and the priority is the
level alone, as Linux's own kernel messages are. `parse_json_kmsg` in
`userspace/dmesg` is a reader of exactly these fields, including the units
of `ts`.

-- lane B
