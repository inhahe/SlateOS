## 1040. sharutils' option library is ported as a library, its oddities with it

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `uuencode` and `uudecode` do not read their own command lines.
GNU AutoGen writes a table for each, and a library bundled with them
(libopts 41.1) does everything else: the flags, a settings file in the home
directory (`~/.sharrc`), `--help` through a pager, three kinds of `--version`,
and options to save the current settings to a file and load them back. That
library is ported as a crate of its own, `autoopts`, rather than each program
getting a small hand-written parser -- and it keeps the library's mistakes
where they change what a user sees, because a user of the real programs sees
them too.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Port libopts as a crate, quirks kept (chosen)** | every flag, message, exit status and settings-file effect is upstream's; 433 cases compared | a user moving a script or a `~/.sharrc` from Linux gets the same behaviour; the next sharutils program (`shar`, `unshar`) gets the machinery for free | about 2 500 lines for two small programs, and several behaviours kept that are plainly bugs upstream |
| Hand-write each program's options | `-m`, `-e`, `-o`, `-c` and `--help` work; the rest either missing or approximate | small | `~/.sharrc` silently ignored -- a user whose file says `base64` gets the other encoding with no message; every error message's wording and exit status different; nothing to check it against but prose |
| Port libopts, fixing its bugs | as the chosen one, except where upstream is wrong | nicer | "wrong" is then this tree's judgement, invisible to a user who only knows that the same command did something different on the two systems -- and the harness can no longer tell a fix from a regression |

**The oddities kept**, each measured against the real program and listed in
`autoopts`' crate docs: a `~/.sharrc` line needs its newline and a trailing
`\` does not continue it; `<name>value</name>` loses the value's first byte;
only the first `<?program>` directive is ever compared; a `load-opts` line in
`~/.sharrc` counts the options it loads as typed, so `-m` on the command line
becomes a second `base64`; an optional argument takes the next word, so
`uuencode -v file` is a bad version mode. In uudecode: a short line decodes
what a longer earlier line left in the buffer; a blank base64 line is a write
error; `~user` with no slash scans the whole line buffer for one.

**Where upstream is undefined, the port chose, and says so:** `--save-opts`'
warnings pass one argument to a two-`%s` format (upstream prints a register's
leftovers; this prints nothing, and the harness normalises exactly those three
messages), and uudecode reads bytes no line wrote as zero where upstream reads
its stack. Neither can be matched, and neither is worth a crash to imitate.

**Two consolidations came with it.** gnulib's base64 is bundled by coreutils and
by sharutils, eight years apart but the same decoder; it is one crate,
`gnubase64`, not the second transcription this port first wrote. And libopts'
pager runs through `shellcmd`, which is what `coreutils::shell` was, moved out
so the two do not each decide how a command reaches `sh -c`.

**Where:** `userspace/autoopts`, `userspace/uuencode`, `userspace/uudecode`,
`userspace/gnubase64`, `userspace/shellcmd`; `scripts/uu-diff.sh` and
`scripts/sharutils-ref.sh`. Closes
`known-issues.md` -> `TD-B-BASE64-IS-STILL-THE-OLD-CRATE-UNTIL-UUENCODE-MOVES`.

**Revisit** if a sharutils release fixes any of the kept bugs: the port follows
the version the harness compares against, so the fix comes with moving the
reference.
