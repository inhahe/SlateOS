### [E] The credential manager said "Copied Password" over a clipboard no other program could read -- 2026-09-27
**Status:** OPEN, waiting on the operator -- the false claim is FIXED (Copy refuses
in words); copying itself needs `requests/e-a-a-clipboard-door-for-applications.md`,
which lane A holds (2026-09-27) until the operator answers `open-questions.md`
C-Q29 -- which way copy and paste travels between programs -- where lane C is
adding the kernel's own clipboard as option C. Whichever is chosen, the
password manager's "clear it if it is still mine" (a token checked at clear
time) carries over: `requests/a-ce-the-clipboard-transport-is-c-q29-and-the-kernel-clipboard-is-a-third-option.md`.

**In short:** pressing Copy on a password showed "Copied Password -- clears in
30s". The password went into a variable inside the credential manager, and
nothing else on the machine could paste it: SlateOS's system clipboard is in
the kernel (`fs::clipboard`) and only kshell can reach it. A user went to
paste the password somewhere, got nothing, and had no way to tell why. An
earlier fix had wired Copy to that variable "so the one operation a
credential manager exists for" could be done -- which made the missing
capability look present.

**Now:** Copy copies nothing and says so where "Copied" was: "Password not
copied: no other program could paste it -- applications have no clipboard
yet; reveal it to read it". The in-app clipboard and its auto-clear are gone
(a secret held in memory for nothing to use), and the Settings row for the
auto-clear says it does not apply. The same gap stops every program's copy
at its own window -- the file manager's is
`TD-C-THE-FILE-MANAGER-CLIPBOARD-STOPS-AT-ITS-OWN-WINDOW`.

**When the door lands:** copy through it, with the auto-clear back as a
clear-if-still-mine (the request asks for one), so a wipe after thirty
seconds cannot erase something the user copied since.
