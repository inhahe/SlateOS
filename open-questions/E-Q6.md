## E-Q6 — [E] Your WinDirStat fork on SlateOS: port its code, or bring its features to SlateOS's own disk analyzer? — Status: OPEN (raised 2026-10-10)

**In short:** you asked for your fork of WinDirStat on SlateOS (design-decisions
§1050). SlateOS already has a disk analyzer of its own, written here, but it
does a small part of what your fork does. There are two ways to get the rest.
One is to port your fork's code: it is C++ written against MFC (Microsoft's
Windows-only window library), so its windows and its Windows-specific scanning
would be rewritten and its logic kept. The other is to rebuild your fork's
features in SlateOS's analyzer. Which do you want?

**What each has today.**

| | Your fork (`D:/visual studio projects/dirsize/windirstat`) | `apps/diskanalyzer` |
|---|---|---|
| Written in | C++ on MFC and Win32, about 39,000 lines | Rust on SlateOS's toolkit, about 6,500 lines with its tests |
| Views | all files, largest files, duplicate files, search results, file watcher, extensions, treemap | treemap, list, extensions, largest files |
| Treemap | zoom, parent/child selection, extension labels, logical or physical size, two drawing styles | squarified layout, drill-down |
| Columns | logical and physical size, percentage, item counts, attributes, owner, modified time, hard links, hash | name, size, percentage, type |
| Duplicates | hashing, hard-link aware, a hash cache kept between scans and corrected from the file system's change journal | none |
| Search and filters | regular expressions, size, path and name filters, exclusions | none |
| Saving | scans saved and reloaded, CSV export and import | none |
| Actions | open, copy path, show in the file manager, delete, move, empty folders, cleanup shortcuts | none |
| Your fork's own changes | ribbon, exact-byte sizes, sortable timestamps, values shown while a folder is still being counted, a progress bar that learns the machine's speed | progress while scanning; a scan can be stopped |

| Option | *What changes:* |
|---|---|
| **A.** Port the fork | Your fork's own logic runs on SlateOS. Its windows are redrawn with SlateOS's toolkit, and its scanning is rewritten for SlateOS, since the fast scan reads NTFS directly. Two programs stay: your port, and diskanalyzer. |
| **B.** Bring the fork's features to diskanalyzer | SlateOS keeps one disk analyzer, and it gains your fork's views, columns, duplicate finder, search, saving, actions and interface changes, rebuilt in Rust. It follows the system's theme, keyboard rules and permissions as the other programs do. |

- **A** keeps your code. When you change your fork, the same change can come
  across, as far as it touches the shared logic and not the windows. The cost:
  most of the fork's 39,000 lines are its MFC windows and its Windows scanning,
  and those parts would be written again anyway. A second analyzer would also
  sit beside the first.
- **B** gives one analyzer that fits the system. The cost: a feature you add to
  your fork later has to be rebuilt here as well. The two programs would drift
  apart unless both are kept up.

**Claude's recommendation:** **B**. Your fork's value on SlateOS is in its
features: the duplicate finder with its lasting hash cache, search, saved scans,
the extra columns, and your interface changes. Porting would rewrite most of its
code anyway, since MFC and the Windows scanning are most of it. Diskanalyzer
already has the system's tests, theming and permissions, and SlateOS's
change-notification and recycle bin can stand in for the Windows features your
fork uses.

**If never answered:** safe. Nothing is blocked except this one item, and
diskanalyzer keeps working as it does. Neither option has been started.

**Where it bites:** `apps/diskanalyzer`; `roadmap.md`, lane E's backlog, "The
operator's WinDirStat fork on SlateOS"; `roadmap-detailed.md` §6.13.
