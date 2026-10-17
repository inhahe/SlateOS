## TD-B-CURSES-MUTANTS-NEITHER-TESTS-NOR-HARNESS-CATCH (lane B, 2026-10-17)

**Status:** OPEN -- every mutant accounted for on lane-b (2026-10-17), awaiting a boot on main

**In short:** mutation testing breaks one line of the curses library at a
time and checks that some test notices. The review of `ca52b061c6..ca9f00e39c`
(record `db50c8d49`) left 203 such breaks no unit test caught; replayed
through `scripts/curses-diff.sh` as well (`scripts/mutants-replay.py`, its
records `replay*.txt` beside the mutants in
`<git-common-dir>/reviews/20261014T221510-ca9f00e39c0f-mutants/curses/`), 27
were caught by neither. Each is now accounted for: 23 cannot change what the
library does, and the other 4 are caught by harness scenarios added for
them -- each put back into the code by hand and seen to make its scenario
differ from Ubuntu's ncurses.

**Where:** `userspace/curses/src/` -- the line numbers are those of
`ca9f00e39c` (`git show ca9f00e39:userspace/curses/src/<file>`).

### Cannot change what the library does (23)

| Mutant | Why |
|---|---|
| `cell.rs:72:48`, `79:5`, `80:5`: `\|` made `^` | the attribute constants OR-ed are distinct single bits: XOR is OR |
| `addch.rs:85:13`: `ch & A_COLOR` made `\|` in `color_mask` | what the mask lets through reaches the cell's colour bits, which `render_char`'s `set_pair` then writes over |
| `addch.rs:409:19`: `x > 0 && y >= 0` made `\|\|` | at `x == 0` the other branch's row is `y - 1`: no line for `y == 0`, and for `y >= 1` the second clause already held |
| `addch.rs:451:37`: `!` deleted before `iscntrl` | a printable byte's `unctrl` is the byte itself, so the fallback arm writes the same cell `waddch_literal` would |
| `mvcur.rs:558:45`: `& A_ALTCHARSET` made `\|` in `charable` | a wide character's second cell holds a code point of 0x1100 or more, whose `wctob` is never itself: the last clause rejects it anyway |
| `mvcur.rs:615:27`: `yold != -1 && xold != -1` made `\|\|` | nothing in the library knows the cursor's row without its column -- `wrap_cursor`, `scrolln`, clearing and `mvcur_resume` forget both at once -- and the crate has no `mvcur` of its own for a program to pass one alone |
| `term.rs:604:17`: `have > alloc` made `>=` in `reserve_pairs` | resizing the table to its own length |
| `term.rs:1096:63`: `n < maxcolors` made `<=` | `start_color` sets the colour count to `max_colors` itself, so `n < count` already implies it |
| `term.rs:770:50`: `xmc > 0` made `== 0` in `vid_puts` | setup turns a terminal's `xmc` into "absent" (-1) once it has taken the attributes to suppress, as `newterm` does without XMC support, so neither test is ever true there |
| `term.rs:892:31`: `previous_attr &= ALL_BUT_COLOR` made `\|=` | after it the pair is 0, so the colour change that follows is made by its first or second test whatever the reverse bit says, and nothing later reads the attributes |
| `hashmap.rs:260:21`: `i < lines` made `== lines`, and `> lines`, in `grow_hunks` | each line of a run is then a run of its own: the first grows back as the whole run did, the middle ones grow nowhere (each one's limits are the line before), and the last grows forward from the run's end, as the whole run did |
| `hashmap.rs:439:21`: `i < lines` made `<=` in `hash_map` | at `i == lines` the line's old number is out of range, which answers "new", and stops the loop where `<` did |
| `screen.rs:1258:17`: `>` made `>=` in `copy_changes` | at `last_src == limit_x` it is set to the value it has |
| `update.rs:77:5`: `ins_char_cost` made -1; `79:48`: `smir && rmir` made `\|\|` | its one caller asks only when `n_last < n_last_nonblank` is false, and the insertion branch is entered only after the lines' last cells matched, which makes it true -- unless the first step of that match breaks to keep a wide character whole, which needs a wide character's second cell equal and its first not, a pair curses never leaves on a line. Upstream's `InsCharCost` is as unreachable there. |
| `update.rs:124:26`: `n > 0` made `>=` in `putc` | `wcrtomb` of a character that is not NUL never answers 0 |
| `update.rs:155:53`: `c < ACS_LEN` made `<=` | at `c == 128` the map's lookup misses, and `c >= 128` holds anyway |
| `update.rs:1154:9`: `&&` made `\|\|` in `scroll_idl` | it runs only under `idlok`, which needs both insert-line and delete-line (`has_il`) or a scrolling region; and with a scrolling region it is reached only when no `ind`, `indn` or `dl` (forward), or no `ri`, `rin` or `il` (back), could scroll in it -- a terminal that `newterm` then counts as unable to scroll at all, so no scroll is tried |
| `update.rs:1228:25`: `curscol = -1` made `1` in `scrolln` | the row is made unknown with it, and every use of the old column needs the old row known |
| `window.rs:504:14`: `n > 0` made `>=` in `scroll_window` | for `n == 0` each line is copied onto itself, which marks nothing |

### Caught now (4)

| Mutant | By |
|---|---|
| `screen.rs:127:26`: `& TABDLY` made `\|` in `def_shell_mode` | `tab-stops`: `tigetstr ht` on a pty that keeps its tabs (25 cases differ) -- the library never moves with tabs, so only the API shows `ht` taken away |
| `update.rs:913:22`, `914:69` in `transform_line` | `insertion-wide`: characters inserted next to a wide character, after a line's start and at its first column (40 cases differ, each) |
| `update.rs:158:37`: `c >= 160` made `>` | `wide-calls-one-byte-locale`: U+00A0 through the wide calls in the C locale, whose legacy coding is 1 from the start -- the byte calls never make that cell |

**What was added for them** (`67cb6c0b6` and after): the crate's
`tigetstr` and `use_legacy_coding`, as both probes' commands; the scenarios
`insertion`, `insertion-wide`, `unknown-column`, `legacy-coding` and
`scrolling-idl`; `tigetstr` lines in both tab-stop scenarios; and two made-up
terminal types, `slate-ich1` and `slate-ilonly`.
