## §366 — `ls` is reproduced against coreutils **9.5**, not the 9.4 that ships in WSL, because 9.5 changed every width measurement and both binaries are on hand

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)

**In short:** GNU `ls` lines up its columns by asking "how many columns wide is
this text?" Version 9.4 and version 9.5 answer that question differently for
text containing a byte that cannot be displayed — a control character in a
group name, say. 9.4 says "that byte is worth zero columns, the rest is worth
one each"; 9.5 says "I cannot measure this at all", and then pads it by
nothing. The two produce visibly different output. We had to pick one to copy,
and picked 9.5.

### What the difference actually is

gnulib's `mbsnwidth (text, len, flags)` takes a flags word. With
`MBSW_REJECT_INVALID | MBSW_REJECT_UNPRINTABLE` set it returns **-1** for the
whole string the moment it meets an invalid UTF-8 byte, a truncated sequence at
the end, or a character with no width. With flags `0` it cannot fail: an
invalid byte counts one column, a truncated tail counts one and swallows the
rest, and an unprintable character counts one unless it is a control character,
in which case zero.

`ls` calls it in nine places — the two directory-wide width accumulations, the
owner/group name width and its padding, the block-size and file-size padding,
the timestamp fallback width, and the quoted name's own width.

* **9.4 passes `0` at every one of the nine.** There is no strict measurement
  anywhere in 9.4's `ls`.
* **9.5 passes `MBSWIDTH_FLAGS` at every one of the nine**, and each caller
  handles the -1 itself: the accumulations skip it (`if (width < len)` never
  fires for -1), the padding sites emit no padding
  (`name_width < 0 ? 0 : width - name_width`), and the two remaining sites
  clamp with `MAX (0, …)`.

Measured, with a group named `g\002bad` (five characters, one of them a control
character) alongside `gwide一` (seven columns) and `root` (four), so that the
group column is seven wide:

```text
$ ls -l --time-style=+T | od -c        # 9.4
  ... r o o t  sp  g 002 b a d  sp sp sp sp  0 ...     <- padded as width 4
$ .../coreutils-9.5/src/ls -l --time-style=+T | od -c  # 9.5
  ... r o o t  sp  g 002 b a d  sp  0 ...              <- padded as width 0
```

### Why 9.5

* **It is measurable.** This was the whole objection to targeting a version we
  do not have: every claim would become a source-reading rather than an
  observation, against a standing rule that says measure. It turned out not to
  apply — a 9.5 tree was already configured and built at
  `/tmp/coreutils-spike-os-lane-b/coreutils-9.5/src/ls` for the earlier `du`
  work, so both binaries can be run side by side, and the table above is an
  observation of both.
* **It is the newer behaviour**, and the change was deliberate: 9.5 moved all
  nine call sites in one go, which is a decision and not a drift.
* **Half of our `ls` had already been written to it.** The quoting pass reads
  9.5's `quote_name_buf`, including its `MAX (0, displayed_width)` clamp. The
  alternative was to rewrite that half backwards.

### Why not 9.4

It is what `/usr/bin/ls` is in this WSL image, so it is the version a
carelessly-written diff harness would compare against. That is a property of
the harness, not a reason — `scripts/ls-diff.sh` names the 9.5 binary
explicitly, which it should do regardless so that the reference cannot change
under an `apt upgrade`.

### What it costs

The difference is only visible for text that cannot be measured, which in
practice means a file, user or group name holding an unprintable or invalid
byte. For *file* names `ls` defaults to `-q` on a terminal, which replaces such
bytes with `?` before any of this is reached, so the divergence is confined to
piped output and to owner/group names — which come from `/etc/passwd` and
`/etc/group`, where such bytes are already pathological.

### The general form

Two GNU versions that differ in one shared helper's *flags* will not differ
anywhere obvious; they differ in nine scattered call sites at once. Reading one
version's source and measuring another version's binary is then a way to
produce two correct-looking answers that contradict each other — which is
exactly what happened here, across two commits, before the contradiction
surfaced. Pin the version, name the binary, and read the source of the version
you named.
