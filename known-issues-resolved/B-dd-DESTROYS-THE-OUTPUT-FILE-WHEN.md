## B-dd-DESTROYS-THE-OUTPUT-FILE-WHEN-`seek=`-IS-GIVEN (lane B, 2026-08-22) — FIXED 2026-08-22

> **Fixed the same day.** All five defects below are corrected in
> `userspace/coreutils/src/bin/dd.rs`, with 30/30 unit tests and an end-to-end
> check that a `seek=` write into a 4096-byte file leaves all 4096 bytes with
> only the addressed block changed. Kept here because it is the sharpest example
> in the tree of the pattern the entries above are about — a comment that
> correctly describes the right behaviour, sitting above code that does the
> opposite, with tests that pin the wrong answer.

**In short:** `dd` copies raw blocks, and `seek=N` means "leave the first N
blocks of the destination alone and write after them" — it is how you patch a
bootloader into a disk image, or a partition into a slot, without disturbing the
rest. Ours **erased the entire destination file first** and then wrote at the
offset. Anyone using `dd seek=` the way it is meant to be used lost everything
already in the file, silently, with `dd` reporting success. A second bug made a
plausible typo — `bs=1MB` instead of `bs=1M` — mean "block size zero", which
truncated the destination and copied nothing, also reporting success.

**Where.** `userspace/coreutils/src/bin/dd.rs`, and the tell is that the code
carried a comment stating the correct rule while doing the opposite:

```rust
let mut writer: Box<dyn Write> = match &ops.output_file {
    Some(path) => match OpenOptions::new()
        .write(true).create(true).truncate(true)      // <-- file emptied HERE
        .open(path) { ... },
    ...
};

if ops.seek > 0 {
    // dd with `seek=` preserves the existing tail of the output file —
    // it positions the cursor and overwrites in place, so truncate(false)
    // is the correct semantic (not truncate(true)).
    if let Some(f) = ops.output_file.as_ref()
        && let Ok(mut fh) = OpenOptions::new()
            .write(true).create(true).truncate(false)  // <-- too late
            .open(f)
        && fh.seek(SeekFrom::Start(seek_bytes as u64)).is_ok()
    { writer = Box::new(fh); }
}
```

Re-opening a file with `truncate(false)` does not restore what the first open
destroyed. Truncation is decided at open time, which is why GNU decides it
there: its `open` passes `O_TRUNC` only when `seek_records == 0` and
`conv=notrunc` is absent. The fix is one condition — `.truncate(ops.seek == 0)`
— in the single place the file is opened.

**Five defects, all of the same kind: a data loss reported as success.**

| # | What it did | What a user saw |
|---|---|---|
| 1 | `of=` always truncated, even with `seek=` | `dd if=part of=disk.img seek=2048` left `disk.img` containing only the new part. Everything else gone. |
| 2 | An unparseable size became `0` | `bs=1MB` (a real GNU suffix, meaning 1000000) hit a parser that looked only at the last byte, saw `B`, matched nothing, and `"1MB".parse()` failed to `0`. `bs=0` makes every read return `Ok(0)`, which the copy loop cannot tell from end-of-input — so dd truncated the destination, copied nothing, printed `0+0 records in`, and exited **0**. |
| 3 | A failed `seek=` was ignored | The payload was written at offset 0 instead — over the exact bytes `seek=` existed to protect. |
| 4 | A failed `skip=` was ignored | The copy silently started at offset 0, producing a file with the wrong contents rather than an error. Skipping on a pipe also counted *read calls* rather than bytes, so a short read skipped less than asked. |
| 5 | `writer.flush()` result discarded (`let _ =`) | A write failure at the very end lost the tail of the copy and still exited 0. |

Defect 2 is the one most likely to be met by accident, because `1MB` is a
*correct* GNU spelling; defect 1 is the one that destroys the most.

**What it does now.**

- Truncation is decided once, at open: `.truncate(ops.seek == 0)`.
- `parse_size` returns `Result`. Unknown suffixes, empty strings, negative
  numbers and overflow are errors. It understands the full GNU suffix set
  (`c`=1, `w`=2, `b`=512, `k`/`K`=1024, `kB`=1000, `M`/`MB`, `G`/`GB`, `T`/`TB`)
  and products written `2x512`, with the two-letter forms as powers of 1000 and
  the one-letter forms as powers of 1024.
- `bs=0` is rejected outright.
- Every seek, skip, write and flush failure is fatal.
- Skipping on a non-seekable input counts bytes, not read calls.
- `conv=`, `status=`, `ibs=`/`obs=`, `iflag=`/`oflag=` remain **unimplemented and
  rejected**. That is deliberate and is now pinned by a test: a `dd` that accepts
  `conv=notrunc` and truncates anyway is worse than one that refuses, because
  the caller has no way to find out.

**How the tests kept it.** `parse_size_garbage_zero` asserted
`parse_size("notanumber") == 0`, and `parse_size_empty_zero` the same for `""`.
As with `chmod -r`, the tests were right about what the code did and wrong about
what it should do, so the bug was load-bearing: fixing it turned an existing
test red. Both are replaced by tests asserting an error, and the suite grew from
19 to 30 cases, most of them about refusing bad input rather than accepting good
input.

**Reproduction, before the fix:**

```
$ python -c "open('disk.img','wb').write(b'A'*4096)"
$ python -c "open('payload.bin','wb').write(b'B'*512)"
$ dd if=payload.bin of=disk.img seek=1 bs=512
$ wc -c disk.img
1024 disk.img          # was 4096; the last 3 KiB are gone
```

After the fix, `disk.img` is 4096 bytes, block 1 is `B`s, and everything else is
untouched — verified 2026-08-22.

**How this was found.** Not by a harness — there is no `dd-diff.sh`, and a
differential harness for `dd` needs a filesystem rather than a stdout `diff`.
It came out of `scripts/dup-bins-survey.py`, which ranks the 41 duplicated
binary names (see `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`) and
flagged `dd` as 365 lines in `coreutils` against 1166 in `userspace/dd`. The
size gap is not itself a defect — it is a prompt to read the smaller one, which
is what turned this up, exactly as it turned up `chmod -r`. **Both of the two
tools read this way so far had a silent-wrong-behaviour bug**, which is the
argument for reading the rest rather than treating the survey as a ranking to
act on directly.

**Still open, and deliberately deferred:** `conv=`, `status=progress`, separate
`ibs=`/`obs=`. Implementing them belongs with `open-questions.md` → **B-Q7**,
which decides whether this file or `userspace/dd` survives; writing them twice
is the trap that decision exists to avoid.

### Addendum, 2026-08-27 — three statements above are now wrong

`coreutils/dd` has since been rewritten as a transcription of
`coreutils-9.4/src/dd.c` (see `design-decisions.md` §626). Three things this
entry says no longer hold, and are corrected here rather than edited above,
because what the entry got wrong is itself the point.

**1. The stated fix was wrong — in the same way the original bug was.** The
text above says "The fix is one condition — `.truncate(ops.seek == 0)` — in the
single place the file is opened", and that is what shipped. It is not what GNU
does. GNU truncates **at the seek offset**, not at zero and not never: the
`O_TRUNC` is indeed suppressed when `seek != 0`, but `dd` then calls
`ftruncate(fd, seek_records * obs + seek_bytes)` itself. Measured:

```
$ perl -e 'print "A" x 100' > big
$ printf 'xx' | dd of=big bs=1 seek=10
$ stat -c %s big
12                      # not 100, and not 2
```

So the shipped fix turned "destroys the whole file" into "never shortens the
file", which is wrong in the opposite direction and equally silent. Both are
now correct. The lesson is the uncomfortable one: the fix was reasoned from the
*comment* about what `seek=` means, not measured against the program it was
copying — the identical mistake that produced the bug.

**2. "Not by a harness — there is no `dd-diff.sh`."** There is now.
`scripts/dd-diff.sh` runs 339 cases plus 20 deliberate differences, and it
compares four channels per case — stdout, scrubbed stderr, exit status, and a
manifest of the working directory carrying each file's size, block count and
bytes. The manifest is what a `dd` harness needed and what made the entry
conclude one was impractical; it is fifteen lines of `stat` and `od`. Defect 1
above changes **only** the manifest channel — not stdout, not stderr, not the
exit status — so a harness comparing the usual two would have missed it, but
one comparing four catches it on the first case that uses `seek=`. Each of the
four channels was tamper-tested to prove it discriminates (§626).

**3. "Still open, and deliberately deferred: `conv=`, `status=progress`,
separate `ibs=`/`obs=`."** All three are implemented, along with `iflag=`,
`oflag=`, `count_bytes`/`skip_bytes`/`seek_bytes`, the `B` suffix, and the
partial-read accounting. The B-Q7 argument for deferring them — don't write
them twice before deciding which `dd` survives — was overtaken by the argv-utf8
conversion, which had to touch this file anyway; a transcription that stops
short of the operands is not a transcription. B-Q7 still decides which binary
ships, and now decides it between a complete implementation and `userspace/dd`
rather than between a stub and `userspace/dd`.

A fourth statement is *not* wrong but has moved: `skip=` past the end of the
input was fatal here and is a warning in GNU (`cannot skip to specified
offset`, `0+0 records in`, exit **0**). That was not known when this entry was
written and is now fixed.
