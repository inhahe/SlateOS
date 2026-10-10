## C-THE-PASSWORD-PROMPT-CAN-BE-IMITATED-BY-ANY-WINDOW (lane C, 2026-10-05)

**Status:** OPEN -- needs lane F (`requests/c-f-let-the-user-tell-the-systems-own-prompts-from-a-programs.md`).

**In short:** the credential service asks you for the password manager's
master password in a window of its own (`gui/credentialsd`, the prompt). Any
program can open a window that looks exactly like it -- the same title
("Password request"), the same words, the same buttons -- and ask for the
master password itself. Nothing on the screen tells the real prompt from an
imitation, so the one secret that opens every other secret can be phished by
any program you run.

**Where:** `gui/credentialsd/src/prompt.rs`: `AskApp` draws its window as an
ordinary application does. Its title and app id are what it says they are, and
the compositor takes a client's word for both (`WindowSpec::app_id` is
documented as advisory: "never gate a capability on this string").

**Why the prompt alone cannot fix it.** Anything the prompt draws inside its
window, an imitation can draw too: a picture you chose, a phrase, a colour.
What an imitation cannot fake is something drawn *outside* its window by a
party that knows which program owns it -- the compositor, which learns each
client's process from the kernel (`SYS_CHANNEL_PEER_CRED`, design-decisions
§1336). Windows' answer is the secure desktop (the screen dims and only the
system's prompt is live); macOS marks system dialogs in ways apps cannot draw.

**What it is not:** a way for the imitation to get anything *but* the master
password. It cannot answer a program's ask -- only the service writes to that
connection -- and the passwords stay in the vault. But the master password
opens the vault file, which any program running as the user can read.

**Proper fix (lane F, then lane C):** the compositor marks a window it can
vouch for -- drawn by the compositor, outside the client's pixels, so no
client can draw it: for instance the title bar naming the program by its
attested executable for a window that asks to be marked, or a whole-screen
dim behind it with the rest of the desktop not taking input, as a secure
desktop does. Lane C's prompt then asks for the mark, and says in its words
what to look for.

**Meanwhile:** the prompt names the program asking and its whole path, which
helps against a different trick (a program pretending to be another in the
prompt's own text) and not against this one.
