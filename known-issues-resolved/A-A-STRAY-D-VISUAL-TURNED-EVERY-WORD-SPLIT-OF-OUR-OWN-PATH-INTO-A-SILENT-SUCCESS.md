## `A-A-STRAY-D-VISUAL-TURNED-EVERY-WORD-SPLIT-OF-OUR-OWN-PATH-INTO-A-SILENT-SUCCESS` (lane A, 2026-08-27) -- **FIXED 2026-08-27**, environment

**In short:** our checkout lives under `D:\visual studio projects\`, a path with
spaces in it. Any command that splits that path on whitespace -- an unquoted
variable, `xargs` without `-d '\n'` -- turns one filename into three words, the
first of which is `D:\visual`. That should fail loudly. It did not, because a
stray 88-byte *file* named `D:\visual` was sitting in the root of the drive, so
the split path **opened successfully** and the tool read 88 bytes of nonsense
instead of reporting a missing file. The file is now renamed, so the failure is
loud again.

**How the landmine got there.** On 2026-07-19 a throwaway probe of `osh` (lane
C's shell port) interpolated a scratch directory into the shell string it was
testing, without quoting it:

```sh
D="D:/visual studio projects/os/target/tmptest"
"$OSH" -c "echo one > $D/e.txt 2>&1; echo two >> $D/e.txt; ..."
```

The shell under test word-split `$D`, so the redirect target became `D:/visual`
and the remainder became extra `echo` arguments. `D:\visual` was created with
exactly the two lines it still held today:

```
one studio projects/os/target/tmptest/e.txt
two studio projects/os/target/tmptest/e.txt
```

Birth and last write were both 2026-07-19 08:35 -- it was written once and never
again. Everything after that was a *read*.

**What it cost, twice.**

- **2026-08-08** the operator reported "something keeps trying to write to and
  launch `visual`, hundreds of times". Lane C found and killed a genuine
  14-hour fork storm (two orphaned `bash /tmp/g4.sh` job-control probes) and
  correctly identified the quoting bug as its own -- but treated the file as
  spent and left it on disk. That is why the report came back.
- **2026-08-22/23** a sweep in an unrelated project ran
  `xargs -a /tmp/allftsl.txt -n 400 ./harness_intree.exe` over 2595
  space-containing absolute paths. Without `-d '\n'`, `xargs` split every one,
  the harness opened `D:\visual` on every batch, and both parsers under test
  reported *identical* parse errors quoting
  `projects/os/target/tmptest/e.txt`. Identical failures across two independent
  parsers read as a grammar bug; it was an invocation bug wearing a costume,
  and it invalidated the whole 2595-file sweep. Re-running with `-d '\n'` gave
  `MATCH 2595/2595`.

Both times the diagnosis cost real operator attention, and both times the actual
defect was one missing pair of quotes.

**Fixed.** `D:\visual` renamed to
`D:\visual.stray-from-unquoted-path-2026-07-19.txt` -- content preserved, but
nothing resolves `D:\visual` any more, so the next word-split fails at `open`
with a name that says what went wrong. Verified: no other extensionless
single-word file at `D:\` root shadows a prefix of our path, and nothing on the
machine had written the file since July.

**Standing rules this leaves behind.** Neither is enforced by a gate; both are
cheap and both were violated by an expert-looking one-liner.

1. **Quote every interpolation of a repo path**, including inside the command
   string handed to a shell *under test* -- that string is parsed a second time,
   so one level of quoting is not enough.
2. **`xargs` over our paths needs `-d '\n'` or `-0`.** Bare `xargs` splits on
   whitespace, and our paths all contain it.
