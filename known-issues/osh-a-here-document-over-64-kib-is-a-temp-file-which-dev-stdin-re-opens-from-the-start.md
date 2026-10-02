## osh: a here-document over 64 KiB is a temp file, which `/dev/stdin` re-opens from the start

**Status:** open (found 2026-08-26, same session; a corner of the
special-redirection-filename work that was left unmatched deliberately).

Since bash 5.1 a here-document is **not** always a temp file. bash writes it
down a *pipe* when it fits in the pipe buffer and spills to a temp file only
when it does not — and the two re-open differently, because a pipe cannot
rewind and a file can. Measured, bash 5.2.21, with
`{ read a; read b < /dev/stdin; echo "a=${#a} b=${#b}"; }` over a document
whose first line is `n` bytes and whose second is `second`:

```text
document total   bash              osh
<= 65536         b=6   (no rewind) b=6   — matches
>  65536         b=n   (rewinds)   b=6   — diverges
```

The boundary is exact and was bisected: a total of 65536 bytes is a pipe, 65537
is a temp file. That is the pipe capacity, not a bash constant to guess at.

osh models every here-document as an `InputSrc::Bytes` snapshot and always
continues from the cursor, so it is right below the boundary and wrong above
it. The false comment that used to sit on the `InputSrc::Bytes` arm of
`Shell::input_special_src` — "bash's here-documents are temp files, which a
re-open reads from the start" — asserted the opposite of the measurement and
has been replaced with it.

**Fix.** Split on the snapshot's length at the point the re-open is resolved:
above the pipe capacity hand back the whole buffer (a rewind), at or below it
hand back the remainder. The threshold should be read from the host rather than
hardcoded, since it is the pipe capacity.

**Why it was not fixed with the rest.** It is a behaviour of *here-document
storage*, not of the special filenames, and touching how here-documents are
held is a change with its own blast radius. The measurement is recorded here so
the fix does not have to rediscover the boundary.
