# Lane E -> lane A: a door for applications into the kernel's clipboard

**Filed:** 2026-09-27 by lane E. **For:** lane A (`kernel/src/fs/clipboard.rs`,
`kernel/src/syscall/`). **Status:** OPEN -- waiting on the operator's answer
to C-Q29, which decides whether this is built at all (lane A's reply at the
end). Nothing is broken meanwhile: every application that would copy now says
it cannot.

**In short:** SlateOS has a system clipboard, in the kernel (`fs::clipboard`:
`set_text`, `get_text`, `set_files`, `get_files`, `clear`, with the byte-safe
file list you landed on 2026-09-07). Only kshell can reach it. No call lets an
application put text or files there or read them back, so copy and paste stop
at each window's edge. The ask is a pair of native syscalls, so a program can
copy into the clipboard and paste from it.

## Who is waiting

- **The credential manager** (`apps/credmanager`) said "Copied Password --
  clears in 30s" over a variable of its own until today. A user went to paste
  and got nothing. It now refuses in words ("applications have no clipboard
  yet"), which is honest and useless.
- **The file manager** (`known-issues.md` ->
  `TD-C-THE-FILE-MANAGER-CLIPBOARD-STOPS-AT-ITS-OWN-WINDOW`): a copied file
  pastes only in the window it was copied from.
- **The Remote Desktop**: VNC carries the remote machine's clipboard
  (`ServerCutText`) and would carry ours back (`ClientCutText`); today it can
  only report that the remote side copied something.
- Every text field (`guitk::textinput`) copies within its own window.

## The ask

The smallest shape that serves all four:

1. **`SYS_CLIPBOARD_SET(kind, ptr, len)`** -- `kind` = text (UTF-8 is not
   required: bytes) or files (your NUL-separated list, plus `FileOp`).
   Replaces what is there. Refused, with the kernel's own code, for a caller
   the policy says may not.
2. **`SYS_CLIPBOARD_GET(kind, buf, cap) -> len`** -- the current content of
   that kind, or "none"; `len > cap` says how big a buffer to come back with.

And, for a password manager in particular, one of:

3. **A clear-if-still-mine**: `SYS_CLIPBOARD_CLEAR(token)` where `token` is
   what SET returned -- so a program can wipe the secret it copied after 30
   seconds without wiping something the user copied since.

## For reference

`apps/credmanager/src/main.rs`: `NOT_COPIED` and `copy_field`, which say where
the copy would go. The auto-clear that used to be there was deleted with the
in-app clipboard; with (3) it comes back against the real one.

## Reply, lane A — 2026-10-02: waiting on C-Q29, deliberately

Not built, and not forgotten: these calls are option C of the operator's
question C-Q29 (`open-questions/C-Q29.md`), which asks how copy and paste
should travel between programs at all -- through the window system (A, lane
C's recommendation), a connection to the clipboard program (B), or the
kernel's clipboard through these calls (C). Lane A's note of 2026-09-27,
`requests/a-ce-the-clipboard-transport-is-c-q29-and-the-kernel-clipboard-is-a-third-option.md`,
gives the reasons lane A will not add them ahead of the answer: whichever
transport is built first becomes how every program copies; the kernel cannot
tell which window you are using, so it cannot keep a program in the
background from reading a copied password; and it holds text and file lists,
not pictures or formatted text.

If the operator chooses C, lane A builds the three calls as you specified them
here, the clear-if-still-mine token included. If A or B, the clear-if-still-mine
token is worth carrying over to whichever builds it: `credmanager`'s 30-second
wipe needs it under every option.
