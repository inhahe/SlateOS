## TD-B-FDISK-HAS-NO-READLINE (lane B, 2026-10-17)

**Status:** OPEN

**In short:** on a terminal, Ubuntu's `fdisk` and `sfdisk` read each
answer with GNU readline: the arrow keys move along the line and through
earlier answers, and Tab completes file names. SlateOS's `fdisk` and
`sfdisk` read an answer as they read one from a pipe, so on a terminal only
the terminal's own line editing works (Backspace, ^U, ^W). `fdisk` also
shows the list of GPT partition types all at once, where Ubuntu's shows it
a page at a time through `less`. Scripted use -- input from a pipe or a
file, which is how both are tested -- is unaffected.

**Where:** `userspace/fdisk/src/main.rs`, `get_user_reply`: upstream's
`#ifdef HAVE_LIBREADLINE` branch (`rl_callback_handler_install`,
`rl_callback_read_char`, used when `is_interactive`) is not ported; the
`fgets` branch is taken always. `list_partition_types` does not call
`pager_open` / `pager_close` (`lib/pager.c`). `userspace/sfdisk/src/main.rs`,
`get_user_reply` and the script loop of `sfdisk_create_disk`: upstream's
`readline (prompt)` on a terminal (with `add_history`) is not ported; the
prompt is written and the line read with `fgets` always.

**The proper fix:** a readline-compatible line editor for userspace -- none
exists yet; the shells would want the same one (osh's `--noediting` says it
has none) -- driven through the callback interface upstream uses, so the
prompt, the `^C` handling and the empty-line convention (`\n` for an empty
answer) stay upstream's; and a port of `lib/pager.c` (fork `$PAGER`, or
`less`, with standard output piped to it) for the type list, which sfdisk
and others would share.
