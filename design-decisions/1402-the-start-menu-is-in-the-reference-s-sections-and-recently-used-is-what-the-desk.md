## 1402. The start menu is in the reference's sections, and "Recently used" is what the desktop started

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The start menu's list was the pinned programs and then every
installed program in folders, with a line between. It is now in the Aero
reference's three sections, each under a heading: "Pinned", "Recently used"
-- the last eight programs started, newest first -- and "All apps", the
folders. A program counts as used whichever part of the desktop started it:
its pin, a desktop shortcut, a jump list, the Run box or the menu.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What counts as used | every launch the session carries out (`queue_launches` calls `note_started`) | only starts from the start menu | a program the user opens from its pin every day is the one they use; the menu is not the only door |
| A program started in a terminal | credited to itself (`launcher::program_started`) | to the terminal it runs in | otherwise "Terminal" would be the only program ever used |
| What is listed | installed programs only, eight, newest first, each once | a Run-box command too; more rows | the section is a list of programs to start again; the reference lists eight |
| A pinned program that was used | listed in both sections | left out of "Recently used" | the reference lists Thunderbird in both; the section says what was used, not what is missing from the pins |
| Headings | only above a section with something in it; "All apps" only below another | always all three | an empty heading is a promise with nothing under it; "All apps" alone heads nothing |
| Kept | `startmenu.yaml`, `recent:`, beside `pinned:` | in memory for the session | the reference's menu remembers across logins, as every desktop's does |

### What is not done here

- **Sticky headings** -- the reference's stay at the top of the list while
  their section scrolls under them. The list scrolls by rows, and a heading
  drawn over the first row would hide it; it is worth doing with the scroll
  made smooth.
- **The pins as a grid of tiles** -- the reference's stylesheet has one
  (`aero-sm-pinned`), though its markup lists them as rows.
