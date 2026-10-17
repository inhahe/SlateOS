## TD-B-CURSES-MUTANTS-NEITHER-TESTS-NOR-HARNESS-CATCH (lane B, 2026-10-17)

**Status:** OPEN

**In short:** mutation testing breaks one line of the curses library at a
time and checks that some test notices. The review of `ca52b061c6..ca9f00e39c`
(record `db50c8d49`) left 203 such breaks no unit test caught; replayed
through `scripts/curses-diff.sh` as well (`scripts/mutants-replay.py`, records
in `target/drafts/replay-*.txt` on lane B's machine), 27 are caught by
neither. Each is either a change that cannot alter what the library does --
written down below, with why -- or a path no harness scenario reaches, which
needs one.

**Where:** `userspace/curses/src/` -- the line numbers are those of
`ca9f00e39c` (`git show ca9f00e39:userspace/curses/src/<file>`).

### Cannot change what the library does (13)

| Mutant | Why |
|---|---|
| `cell.rs:72:48`, `79:5`, `80:5`: `\|` made `^` | the attribute constants OR-ed are distinct single bits: XOR is OR |
| `addch.rs:85:13`: `ch & A_COLOR` made `\|` in `color_mask` | what the mask lets through reaches the cell's colour bits, which `render_char`'s `set_pair` then writes over |
| `addch.rs:409:19`: `x > 0 && y >= 0` made `\|\|` | at `x == 0` the other branch's row is `y - 1`: no line for `y == 0`, and for `y >= 1` the second clause already held |
| `addch.rs:451:37`: `!` deleted before `iscntrl` | a printable byte's `unctrl` is the byte itself, so the fallback arm writes the same cell `waddch_literal` would |
| `mvcur.rs:558:45`: `& A_ALTCHARSET` made `\|` in `charable` | a wide character's second cell holds a code point of 0x1100 or more, whose `wctob` is never itself: the last clause rejects it anyway |
| `term.rs:604:17`: `have > alloc` made `>=` in `reserve_pairs` | resizing the table to its own length |
| `term.rs:1096:63`: `n < maxcolors` made `<=` | `start_color` sets the colour count to `max_colors` itself, so `n < count` already implies it |
| `update.rs:124:26`: `n > 0` made `>=` in `putc` | `wcrtomb` of a character that is not NUL never answers 0 |
| `update.rs:155:53`: `c < ACS_LEN` made `<=` | at `c == 128` the map's lookup misses, and `c >= 128` holds anyway |
| `update.rs:1228:25`: `curscol = -1` made `1` in `scrolln` | the row is made unknown with it, and every use of the old column needs the old row known |
| `window.rs:504:14`: `n > 0` made `>=` in `scroll_window` | for `n == 0` each line is copied onto itself, which marks nothing |

### No scenario reaches them yet (14)

| Mutant | What it would take |
|---|---|
| `hashmap.rs:260:21` (`<` made `==`, and `>`), `439:21` (`<` made `<=`) | scrolls whose runs of moved lines grow into lines that changed a little, so `grow_hunks` and `hash_map`'s forward limits decide |
| `mvcur.rs:615:27`: `&&` made `\|\|` | a terminal without `cup`, its cursor's row known and column not: upstream returns to the margin first |
| `screen.rs:1258:17`: `>` made `>=` in `copy_changes` | a window refreshed whose changes end exactly at its last column |
| `screen.rs:127:26`: `& TABDLY` made `\|` in `def_shell_mode` | a terminal whose tabs are not expanded, and a move tabs would make cheapest |
| `term.rs:770:50`, `892:31` in `vid_puts` | attribute changes on the magic-cookie terminal the harness builds (`xmc`) |
| `update.rs:77:5`, `79:48` in `ins_char_cost`; `913:22`, `914:69` in `transform_line` | a line into which characters are inserted -- next to a wide character for the last two -- under terminals with `ich`, with `smir`/`rmir`, and with `ich1` |
| `update.rs:158:37`: `c >= 160` made `>` | byte 160 under `use_legacy_coding (1)` in the C locale (a probe command the harness lacks) |
| `update.rs:1154:9` in `scroll_idl`: `&&` made `\|\|` | a scroll, under `idlok`, on a terminal that can delete lines (`dl`/`dl1`) but not insert them, or the reverse, and has no scrolling region to use instead |

**The proper fix:** add those scenarios to `scripts/curses-diff.sh` (and the
probe commands they need to both probes), then replay these 14 through it
with `scripts/mutants-replay.py`, and close this when each is caught.
