## TD-B-EVERY-EXIT-CODE-I-MEASURED-THROUGH-WSL-WAS-THE-SAME-ZERO (lane B, 2026-09-14)

**Status:** instrument defect, understood; no code change needed

Every exit status read with `$?` inside a `wsl -d Ubuntu -- bash -c '…'`
payload comes back **0**, whatever actually happened. The control that
settles it is one line:

    $ wsl -d Ubuntu -- bash -c '... false; echo "false_rc=$?" ...'
    false_rc=0          # `false` exits 1, by definition

It is not the heredoc, and not a quoting slip in one probe: a script written
inside WSL with a quoted delimiter and run as `bash /tmp/rc.sh` gives the same
`false_rc=0`. Something in the layering between the Bash tool and WSL resolves
`$?` before the inner shell ever sees it.

### What it cost

One false entry in this file — "GNU `patch -Q` exits 0" — now withdrawn above.
It was recorded *with* a caveat that a one-sample exit code wants
re-measuring, which is the only reason it did no damage. A GNU-abbreviation
probe from the previous tick (`patch --dry-run --inp=…`, "exit 0") rests on
the same broken reading and should not be relied on either.

### The workaround, which is also the better habit

Ask the shell the question directly instead of reading a variable:

    if patch -Q </dev/null >/dev/null 2>&1; then echo zero; else echo NONZERO; fi

`if` consumes the status where it is produced, so nothing can rewrite it in
between. Run `false` and `true` alongside as controls in the same script —
that is what caught this, and it costs two lines.

### What was NOT affected, and why it is worth saying

* **`scripts/patch-diff.sh` is sound.** It runs as a real script from Git Bash
  and prints genuine non-zero codes (`ours (rc=2)`, `gnu (rc=2)`), so all 68
  of its verdicts stand. The bug is specific to my ad-hoc one-liners.
* **Every conclusion I drew from stdout stands**, because none of the ones
  that mattered rested on the exit code: GNU applying the Latin-1 patch was
  read off `patching file orig.txt` plus the resulting bytes, and GNU seeing
  the `diff` difference was read off `2c2`.

This is the same shape as `TD-B-MY-AD-HOC-SEARCHES-OVER-REPORT-BY-AN-ORDER-OF-MAGNITUDE`,
one layer down: there the ad-hoc instrument over-reported, here it under-read.
Both say the purpose-built harness is the thing to trust, and both were caught
by running a control rather than by reasoning about the tool.
