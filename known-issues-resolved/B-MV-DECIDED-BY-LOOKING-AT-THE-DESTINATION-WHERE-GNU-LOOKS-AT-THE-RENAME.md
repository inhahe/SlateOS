## B-MV-DECIDED-BY-LOOKING-AT-THE-DESTINATION-WHERE-GNU-LOOKS-AT-THE-RENAME (lane B, 2026-08-31) — FIXED

**In short.** `mv` was written by reading GNU's `mv` and reproducing what it
appeared to do. A harness that actually *runs* both and compares the output
(`scripts/mv-diff.sh`, 178 cases against a built coreutils 9.4) found **17
places where the two disagreed** — and they were not 17 separate oversights.
They were one wrong idea, repeated: we decided what to do next by **looking at
the destination**, where GNU decides by **looking at what the rename returned**.
Those two questions have the same answer in the ordinary case and different
answers in every unusual one. All 17 are now fixed and the harness reports 0
differences.

**Why the reading-based version passed review.** Every one of the 17 sites was
individually defensible, and several were things the code had a *test* for. A
test written from the same misreading as the code agrees with it. The only
thing that disagreed was GNU itself.

### The structural error

GNU carries `x.rename_errno` as a tri-state: not tried, succeeded, or failed
with this errno (`mv.c:466`, `copy.c:2231`). Every later decision reads that
variable. We had no such variable — we re-derived the situation from a `stat`
of the destination whenever we needed it. The re-derivation is a *different
question*, and it is asked at a *later time*.

Concretely, `copy.c:2757` retries the rename when the recorded errno is
`EEXIST`. We retried when the destination existed. A destination that was
deleted by another process in between makes those differ; so does a rename that
failed with `EEXIST` for a reason the destination no longer shows.

### The one that could not be found by reading at all

`copy.c:2241-2250` decides which file to `stat` for `dest_info` — the record
that powers "will not overwrite just-created `%s`":

```c
char const *name    = rename_errno == 0 ? dst_name  : src_name;
int         dirfd   = rename_errno == 0 ? dst_dirfd : AT_FDCWD;
char const *relname = rename_errno == 0 ? drelname  : src_name;
… follow_fstatat (dirfd, relname, &src_sb, fstatat_flags) …
```

**When the rename succeeded, GNU stats the destination.** The variable is
called `src_sb`, and it is named for the *other* branch. We stat'd the source —
which the rename had just renamed away — so the stat failed, nothing was ever
recorded in `dest_info`, and the just-created check could never fire under any
input. Two of the 17 differences were this one line, and no amount of re-reading
our own code would have shown it; only running GNU did.

### The rest, grouped

- **`>2` operands with a non-directory last operand.** GNU calls
  `target_directory_operand`, and on failure with `2 < n_files` reports
  `mv: target 'c': Not a directory` (`mv.c:495`). We reported a per-source
  error. Note the synthesised error is built from an `ErrorKind`, **not** a
  hard-coded `20`: `errmsg::strerror` picks the POSIX text by kind precisely
  because a raw OS code is a Win32 code on the dev host and an errno on the
  target, and `from_raw_os_error(20)` printed *"The system cannot find the
  device specified."* on Windows.
- **`mv a/.. d`.** We had given `mv` `cp`'s rule. The
  `arg_base += STREQ (arg_base, "..")` bump is `cp.c:678` **only**; `mv.c:544`
  has no equivalent, so GNU targets `d/..` and refuses it as
  `are the same file`. Our diagnostic said the source "names nothing".
- **Diagnostic wording and which operand is named.** `copy.c:2833-2862` splits
  on the errno: `EDQUOT|EEXIST|EISDIR|EMLINK|ENOSPC|ETXTBSY|ENOTEMPTY` give
  `cannot overwrite %s` (destination only — `copy.c:2851` says naming the
  source too "is more likely to confuse the user than be helpful"), everything
  else gives `cannot move %s to %s`.
- **The refusal checks ran against a destination that might not exist.** They
  are now asked only when the `stat` succeeded, which is GNU's guard.

**Where.** `userspace/coreutils/src/bin/mv.rs`; harness
`scripts/mv-diff.sh`. Commit "mv: rebuild the rename path around GNU's control
flow, measured not recalled".

**What this supersedes.** Item 3 of "`mv` converted, and the four further bugs
the rewrite uncovered (2026-08-22)" above says `mv a/.. dst` is "refused with a
diagnostic" — the refusal is now GNU's `are the same file` rather than ours.
The twelve-unimplemented-options list in that section is unchanged and is now
mirrored as 61 explicitly-marked cases in the harness, so each one can be
promoted to a passing case as it lands.
