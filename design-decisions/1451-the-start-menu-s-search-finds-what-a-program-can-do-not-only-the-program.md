## 1451. The start menu's search finds what a program can do, not only the program

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** Typing "display" in the start menu now lists "Display
settings" first, and pressing Enter or clicking it opens Settings on its
Display page -- not on its front page. Any program's extra commands -- the
ones a right-click on it offers, such as a browser's "New private window" --
are found the same way, by name, among the programs. The row says which
program it belongs to, after its own name, in dimmer text.

**What was there.** The start menu's "Display Settings", "Network Settings"
and "Sound Settings" rows were removed on 2026-09-25 because they started a
program that could not exist (known-issues
`TD-C-THREE-LAUNCHER-ENTRIES-NAME-A-PROGRAM-THAT-CANNOT-EXIST`). On 2026-09-27
they came back as Settings' own actions -- its jump list, reached by a
right-click on Settings (§1425). What was still missing is the search: a
search for "display" found Settings by a keyword and opened its front page.

**How it behaves** (`DesktopShell::start_search_rows`):

| | |
|---|---|
| What is found | a program by name, description or keyword, as before; an action by its name only |
| Which actions | the ones its jump list offers -- those with a command line (`AppEntry::jump_list`, one rule for both) |
| Ranking | together: an action's name counts as a program's name does, so "display" ranks the action (by name) above Settings (by keyword); equal scores keep the menu's order, a program before its actions |
| Each once | a pinned program's actions are found once, as the program is |
| A click, Enter | starts the action as its jump list does, and closes the menu |
| A drag | carries nothing -- an action is not a thing to pin -- and no label follows the pointer |
| A right-click | nothing: the pin menu is a program's |
| The row | the action's picture (its program's when it names none), its name, then its program's name, dimmer |

| Alternative | For | Against |
|---|---|---|
| **An action is a search result of its own** (chosen) | the search lands where it asks; any program's actions, one rule | the list can hold several rows named alike, e.g. two programs' "New window" -- hence the program's name on the row |
| Separate start menu entries for Settings' pages | a row for each page even without a search | four entries sharing one program: pins, recents, drags and the de-duplication all key on the program, about twenty sites to rework |
| Match an action by its program's keywords too | "wifi" would find the network page | it would find *every* action of the program for a word that fits one; an action has no keywords of its own to say which |
| Leave the pages on the jump list only | nothing to build | a search for a page opens the wrong page |

**Revisit if** the desktop entry specification gives actions keywords (so
"wifi" could find the network page), or if a program with many actions
crowds the results -- a cap per program would then be the lever.
