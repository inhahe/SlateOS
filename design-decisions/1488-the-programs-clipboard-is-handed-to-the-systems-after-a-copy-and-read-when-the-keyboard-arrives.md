## 1488. The program's clipboard is handed to the system's after a copy, and read when the keyboard arrives

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** text copied in one program could never be pasted in another.
Lane F has built a clipboard into the window system (lane C's request): a
program with the keyboard can put text on it and read it back. The toolkit's
own clipboard, which every field in a program already shares, is now joined
to it in two moves: a copy made in the program is handed over right after it
is made, and the window system's text is read each time one of the program's
windows gets the keyboard -- which is the only moment another program's copy
could have arrived, since only the window with the keyboard may copy. The
desktop does both for its own fields today; ordinary programs get it when
lane F's event loop makes the same two calls (asked in the request). How
programs should reach the system's clipboard is still the operator's open
question C-Q29: this joins the toolkit to the answer lane F built, and the
two calls would join it the same way to any other.

**Where:** `gui/toolkit/src/clipboard.rs` (`take_outgoing`,
`adopt_incoming`), `gui/desktop/src/session.rs` (`give_system_clipboard`,
`take_system_clipboard`); `requests/c-f-carry-the-clipboard-over-the-compositor-connection.md`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Handed over after each copy, read when the keyboard arrives** | asking the window system at every paste | No round trip at a paste, and no change to any field: they all copy and paste through the toolkit's clipboard already. Reading at the keyboard's arrival is complete, because only the window with the keyboard may copy. | A copy another program makes while ours still has the keyboard cannot happen -- the window system refuses it -- so there is no case to miss; if that rule were ever lifted, this would need a change notice. |
| **The program's own copy, read back, changes nothing** | taking whatever is read as a new copy | A rich field's copy keeps its formatting and pictures beside the text it handed over; read back as plain text it would lose them every time the window regains the keyboard. | Another program copying exactly the same text keeps our formatted version -- which matches it word for word. |
| **A picture alone hands over empty text** | handing over nothing, leaving the older text | Another program's paste finds nothing rather than something copied before the picture, which the user did not just copy. Pictures cross when the system's clipboard carries formats. | Pasting elsewhere after copying a picture pastes nothing. |
| **A refusal leaves the copy the program's own** | an error on screen | The window system refuses a client without the keyboard, and a copy over 4 MiB; neither is a fault of the desktop, and the copy still pastes within it. | A very long copy silently does not cross. |
