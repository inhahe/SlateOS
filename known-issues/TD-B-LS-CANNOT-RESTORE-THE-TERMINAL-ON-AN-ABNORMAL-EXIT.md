### [B] TD-B-LS-CANNOT-RESTORE-THE-TERMINAL-ON-AN-ABNORMAL-EXIT — 2026-08-22 — OPEN (tech debt)
**Status:** OPEN — nothing to hook while SlateOS has no Unix signals, by design.

**What it is.** GNU `ls --color` installs signal handlers the first time it
writes a colour escape, so that a run killed or suspended part-way through
puts the terminal back before it dies. `put_indicator` does it:

```c
      /* If the standard output is a controlling terminal, watch out
         for signals, so that the colors can be restored to the
         default state if "ls" is suspended or interrupted.  */
      if (0 <= tcgetpgrp (STDOUT_FILENO))
        signal_init ();
```

`Out::put_str` in `userspace/coreutils/src/bin/ls.rs` is that function and does
not, because SlateOS has no Unix signals and `design.txt` forbids adding them
("No Unix signals for process control", "Hardware exceptions → language-level
exceptions"). There is no mechanism to hook.

**How to see it.** Interrupt a coloured listing of a large directory:
`ls --color=always -R / ` then Ctrl-C. GNU's terminal comes back white; ours
stays in whatever colour the escape that was in flight had selected, and every
subsequent prompt is painted until `reset` or `tput sgr0`.

It cannot appear in `scripts/ls-diff.sh`: that harness measures completed runs.

**What the proper fix looks like.** Not signals — a terminal-restore hook that
SlateOS's own process-teardown path runs. The shell wants the same thing for
its own reasons (raw mode, bracketed paste, the alternate screen), so this
belongs wherever that lands rather than in `ls`. When it exists, `Out::put_str`
registers `restore_default_color`'s bytes with it at the moment the `used_color`
latch fires, which is exactly where GNU calls `signal_init`.

**Trigger:** do it when the terminal layer grows an abnormal-exit hook, or when
a second program in the image starts leaving the terminal in a modified state.

Recorded in `design-decisions.md` §368.
