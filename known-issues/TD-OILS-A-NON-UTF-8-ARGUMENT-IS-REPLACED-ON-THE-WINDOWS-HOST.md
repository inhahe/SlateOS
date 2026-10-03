### TD-OILS-A-NON-UTF-8-ARGUMENT-IS-REPLACED-ON-THE-WINDOWS-HOST. A child spawned by osh sees `\xef\xbf\xbd` where bash's child sees `\xff` — 2026-08-08 — **NOT REPRODUCIBLE OFF WINDOWS (confirmed 2026-08-25)**; the host-only claim is now measured rather than argued

> **The "why it is host-only" section below was reasoning; this is the
> measurement.** The `x86_64-unknown-linux-gnu` osh that `scripts/osh-diff.sh`
> builds hands a child an argv vector, exactly as SlateOS will, and the bytes
> survive. A child was given `\xff` three different ways and printed each byte
> it received:
>
> | how the byte reached the child | osh | glibc bash |
> |---|---|---|
> | `showarg $(printf "\xff")` | `[ff]` | `[ff]` |
> | `showarg $'\xff'` | `[ff]` | `[ff]` |
> | `v=$(printf "\xff"); showarg "$v"` | `[ff]` | `[ff]` |
>
> No replacement character anywhere, and identical to bash in all three shapes.
> So the corruption is confined to `wincmd.rs` and the Windows `CreateProcess`
> command line, as argued — and the argument is now backed by a run rather than
> by inspection of the spawn path.
>
> **The corpus restriction is lifted.** "Corpus cases must not route a byte that
> is no character through a child's argv" was a rule imposed by the measuring
> apparatus. Under `scripts/osh-diff.sh` such a case is legitimate and would
> pass; keep it in mind only if a case is ever run against the Windows host
> build directly.

**Where:** `userspace/oils/src/wincmd.rs` and the `std::process::Command` spawn
path in `userspace/oils/src/interp.rs` — Windows takes a UTF-16 command line,
so every argument goes through a lossy widening.

**What.** Measured 2026-08-08:

```sh
<sh> -c 'b=/path/to/bash; "$b" -c "printf %s \"\$1\" | od -An -tx1" _ "$(printf "\xff\xff")"'
bash: ff ff
osh : ef bf bd ef bf bd        # two U+FFFD
```

osh's own *value* layer carries the bytes correctly — `printf` and command
substitution both round-trip them, and `$'\xff'` in a script is exact. They are
lost only at the child-process boundary, and only on this host: MSYS bash
reaches its children through Cygwin's own argv channel, which is not the
Windows command line.

**Why it is host-only.** On SlateOS a child is handed an argv *vector* of byte
strings and there is no command line to encode, so nothing is widened and
nothing is lost. This is a limitation of running osh on Windows, not of osh.

**How it bit.** It cost a wrong diagnosis while adding
`select-menus-items-are-measured-in-terminal-columns.sh`: an item built with
`$(printf '\xff…')` and passed through `"$BASH" -c` measured 6 columns under
osh and 0 under bash, which looks exactly like a `display_width` bug and is
not — the two shells' children were being given different bytes. **Corpus cases
must not route a byte that is no character through a child's argv**; spell it
as an ANSI-C literal for the inner shell to decode, as that case now does.

**Proper fix:** none available on this host — a Windows command line is UTF-16
and there is no encoding of an unpaired byte that `CreateProcess` will carry.
The honest options are to leave it (chosen) or to refuse such an argument with
a diagnostic rather than corrupt it. Refusing would be a *worse* fit for a
development host, where the overwhelmingly common case is that the argument is
text and the spawn should just work.
