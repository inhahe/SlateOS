## TD-B-FTP-ECHOES-THE-PASSWORD-IT-ASKS-FOR (lane B, 2026-09-10) -- FIXED 2026-09-11

**FIXED,** and it was three programs rather than one. `userspace/passwd`
and `userspace/su` carried the same defect with their own wording of the
same comment, so `passwd` displayed both the old password and the new.
The shared helper this entry asked for is the crate `readpass`: it clears
`ECHO`, restores it from a `Drop` guard so the error path cannot leave a
terminal silent, uses `TCSETSF` so type-ahead typed while echo was still
on is discarded rather than read, and **refuses to read at all** on a
terminal whose echo it could not clear.

**The one judgement call.** A pipe is not a terminal, has no echo and
shows nothing, so `readpass` reads plainly there rather than refusing --
otherwise every script that pipes a password in would break. Only the
terminal-that-will-not-go-quiet case refuses.


**In short:** `ftp` prompts `Password: ` and the characters appear on screen as
they are typed. Anyone looking at the terminal, and anything capturing it,
sees the password.

**Where.** `userspace/ftp/src/main.rs`, `read_password`, which is three lines
and entirely honest about itself:

    fn read_password(prompt: &str) -> Option<String> {
        // In a real terminal we would disable echo here. For now, just read a line.
        read_line(prompt)
    }

The comment is accurate and the user never sees it. That is the whole of the
issue: the program's behaviour and its self-description disagree only from
outside.

**The proper fix.** Clear `ECHO` in the terminal's `c_lflag` for the duration
of the read and restore it afterwards, including on the error path -- a
password prompt that leaves echo off after a failure is its own bug. `posix`
has `tcgetattr`/`tcsetattr`; `userspace/passwd` and `userspace/su` need the
same thing, so it belongs in a small shared helper rather than three copies.

**Until then** the prompt should say so rather than look like a normal
password prompt, which is a one-line change and is NOT what this entry asks
for -- it asks for echo suppression. Noted because a warning that becomes
permanent is how a workaround outlives the thing it was working around.

**Found while fixing** the discarded-failure defect in the same function's
caller, which is the second time today that reading one line closely turned up
something beside it.
