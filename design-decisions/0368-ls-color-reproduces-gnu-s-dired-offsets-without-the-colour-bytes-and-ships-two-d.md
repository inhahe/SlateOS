## §368 — `ls --color` reproduces GNU's `--dired` offsets *without* the colour bytes, and ships two deliberate holes: `ca` and `put_indicator`'s signal handling

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)

**In short:** `ls --color=always` now paints its listing, and it matches GNU
coreutils 9.5 byte for byte across 36 cases in `scripts/ls-diff.sh`. Three
choices inside it are worth writing down, because each is a place where the
obviously-correct code would have produced *different* bytes from GNU. First,
GNU's `--dired` byte offsets do not count the colour escapes, and ours do not
either — GNU's are, strictly, wrong, and we copy them. Second, the `ca` colour
(for a file carrying a Linux "capability", a permission bit stored in an
extended attribute) is parsed but never chosen, because deciding it needs a
system call SlateOS does not have. Third, GNU installs signal handlers while a
colour escape is half-written so that a killed `ls` puts the terminal back; the
design forbids Unix signals, so we do not.

### The `--dired` offsets

`--dired` prints a trailing `//DIRED// 58 59 107 109 …` line: the byte offset,
within `ls`'s own output, of the start and end of every file name, so that Emacs
can find them without re-parsing the listing. Upstream tracks the position in a
global `dired_pos` that it increments **by hand** at every write, and gets away
with it because every write goes through one of four `dired_*` helpers.

Every write except one. `put_indicator` — the function that emits a colour
escape — calls `fwrite` directly:

```c
static void
put_indicator (const struct bin_str *ind)
{
  ...
  fwrite (ind->string, ind->len, 1, stdout);   /* no dired_pos += */
}
```

So with `--color` on, the offsets are short by exactly the number of escape
bytes emitted so far, and point into the middle of an escape rather than at the
name. Measured, GNU ls 9.5, the same eleven-entry directory with and without
colour:

```text
$ ls --color=always -l --dired c | tail -1
//DIRED// 58 59 107 109 157 163 222 225 …
$ ls -l --dired c | tail -1
//DIRED// 58 59 107 109 157 163 222 225 …      # identical
```

The two are the same number, and only one of them is right.

Our `Out` cannot make this mistake by accident: it accumulates the output in a
`Vec` and derives the position from `flushed + buf.len()`, which is always the
true offset. Reproducing GNU therefore took *added* code — an `Out::uncounted`
counter that `put_str` advances and `Out::pos` subtracts.

**Why do it.** The acceptance criterion for this port is "byte-identical to
coreutils 9.5" (§367 states the rule and §366 records the cost of departing from
it). An offset is not a rendering detail an editor can shrug off: Emacs' dired
mode uses it to place point. A listing whose offsets are *more correct* than
GNU's is a listing Emacs will mis-navigate in a different way — not a fix, a
second incompatibility. And the combination is already documented as unsupported
upstream; nobody is relying on the right answer, because there isn't one.

**Why one might not.** It is a bug, reproduced knowingly, in code that had to be
made worse to do it. The guard is the same as §367's: the field is named
`uncounted`, its doc comment says outright that it exists to copy a bug, and
`the_dired_offsets_do_not_count_the_colour_escapes` pins both halves — that the
offsets do not move when colour is switched on, and that the *line* grew by
exactly the escape bytes they are missing. A future reader who "fixes" it fails
the test and lands here.

### `ca`, and why it is parsed but never chosen

`LS_COLORS`' `ca` slot colours a file carrying a POSIX capability — a bit of
elevated privilege stored in the `security.capability` extended attribute, which
is how a Linux binary gets to bind port 80 without being setuid root. Deciding
whether a file has one requires `getxattr(2)`. SlateOS has no `getxattr`, and
the capability model it does have is the microkernel's unforgeable handles,
which are not a file attribute at all and have no on-disk representation to
read.

`Ind::Cap` is therefore parsed, defaulted and stored, and `get_color_indicator`
has no arm that can return it. That is not a divergence: GNU's own
`has_capability` is a stub returning false when coreutils is built without
libcap, which is the common distribution configuration, and `ca` is unset in the
default table for exactly that reason. Ours behaves as a GNU built without
libcap — a supported configuration of the reference, not a departure from it.

The same reasoning covers `Ind::Door`: a Solaris door is `S_ISDOOR`, which is a
constant `0` on every target we have.

### `put_indicator`'s signal handling, and why it is absent

Upstream's `put_indicator` does more than write:

```c
put_indicator (const struct bin_str *ind)
{
  if (! used_color)
    {
      used_color = true;

      /* If the standard output is a controlling terminal, watch out
         for signals, so that the colors can be restored to the
         default state if "ls" is suspended or interrupted.  */

      if (0 <= tcgetpgrp (STDOUT_FILENO))
        signal_init ();

      prep_non_filename_text ();
    }

  fwrite (ind->string, ind->len, 1, stdout);
}
```

The purpose is real: a `ls --color` killed between `\033[` and `m` leaves the
terminal painted, and every subsequent prompt is bright blue. GNU catches the
signal, writes the reset, and re-raises.

We do not implement it, because `design.txt` forbids Unix signals for process
control outright — the mechanism does not exist to hook. The observable
consequence is confined to an interrupted run, so it never appears in the diff
harness, which measures completed runs only.

What we *did* keep is the latch that sits in the same `if`: `used_color` is set
before the bytes are looked at, so the first indicator — even an empty one —
is preceded by a `prep_non_filename_text`. That is why a coloured listing opens
with a stray `\033[0m`, and why `LS_COLORS='ec='` (an `ec` that exists and is
empty) still changes the output. The latch and the signal setup are the same
`if` upstream; separating them was the actual work.

**Recorded as tech debt** in `known-issues.md` as
`TD-B-LS-CANNOT-RESTORE-THE-TERMINAL-ON-AN-ABNORMAL-EXIT`: if SlateOS ever grows
a terminal-restore hook for abnormal exit — which the shell will want for its
own reasons — `ls` should use it here.

### The rule this is an instance of

**A hole in a port should be a hole the reference itself has, or it should be
written down.** `ca` and `do` are the first kind: a GNU built without libcap, on
a system without doors, behaves exactly as we do, so there is nothing to record
beyond a comment at the unreachable arm. The signal handling is the second kind:
no configuration of GNU behaves this way, so it goes in a document. Confusing
the two — treating a genuine omission as "just another build configuration" —
is how a port acquires undocumented divergence.
