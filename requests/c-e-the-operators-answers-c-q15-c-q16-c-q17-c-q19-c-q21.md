# C -> E -- the operator's answers to C-Q15, C-Q16, C-Q17, C-Q19 and C-Q21: the parts in your programs

**From:** Lane C. **To:** Lane E (`apps/**`).
**Filed:** 2026-09-27. **Status:** OPEN -- decided by the operator (relayed from
lane F's session the same day); lane C does the shared parts it names.

**In short:** the operator answered five of lane C's questions whose work is
mostly in your programs. Each is written up in `design-decisions.md`
(§1421-§1426). Below is what each asks of you, and, for the games, the per-game
calls the operator left to lane C.

## 1. The games: each board by whether its colours mean something (C-Q16, §1422)

The operator: "C, and I'll let Claude decide which games get which treament. One
other thing: try to make the games look as polished as possible."

**Everywhere:** every game's menus, score panels, dialogs and window background
follow the desktop theme (the palette), as the applications' do.

**The playing surface, by one rule:** a colour keeps its own value when a player
*reads* it -- tells two things apart by it, or knows a convention by it; it
follows the theme when it only fills space.

| Game | Keeps its own colours | Follows the theme |
|---|---|---|
| tetris | the seven pieces (players identify them by colour) | the well, the grid |
| minesweeper | the numbers 1-8 (read at a glance) | the tiles, keeping covered and revealed clearly apart |
| solitaire, freecell, hearts | card faces: white, red and black suits | the felt; the card backs (the accent suits them) |
| mahjong | the tile faces (they are artwork with conventions) | the table |
| memory | the faces that make pairs | the card backs, the table |
| match3 | the gem colours (they are the gem's kind) | the board |
| simon | its four colours (the game *is* remembering them) | the surround |
| game2048 | the tile colours by value (players read size by colour) | the board |
| wordle | green, yellow and grey (the game's language) | the page |
| connect4 | the two players' disc colours | the frame |
| checkers | the two sides' piece colours | the squares, in two palette shades that stay clearly apart |
| chess | light and dark pieces (which side is which) | the squares, as for checkers |
| reversi, gomoku | black and white (which player) | the board |
| battleship | hit and miss markers | the water, the ships |
| towers | the disc colours by size | the pegs, the base |
| breakout | the brick rows (points by colour) | the field, the paddle |
| pacman | the four ghosts (each is a different ghost) | the maze walls, the dots |
| yahtzee | the dice (white, black pips) | the score sheet |
| sudoku, crossword, nonogram, wordsearch | -- (their meanings -- a given digit, a blocked square, a found word -- are drawn in palette roles: text, accent, a palette red for a conflict) | everything |
| asteroids, pong, snake, maze, nim, hangman, tictactoe, lightsout, pinball, sokoban | -- | everything, with the moving pieces in the accent or a palette hue so they stand out |

**Polish**, the operator's second ask: consistent spacing and alignment with the
toolkit's metrics, the Aero reference's look for the chrome (`guitk::button`,
the reference's glass for panels), no text clipped or cut mid-word, animation
where the game has motion to show (a card moving, a line clearing), and every
game fitted to its window at every size.

## 2. Five features to wire up; where there are two versions, the survivor takes everything (C-Q17, §1423)

The operator: "Option A. I think I saw somewhere in C-Q17 that one of the five
finished features already has a wired up version of itself. If true, make the
one that survives have all the features of both versions."

| Program | Feature to wire in | Note |
|---|---|---|
| `apps/installer` | GRUB bootloader configuration (`pub mod grub`) | the most serious gap: an installer that cannot set up a bootloader |
| `apps/imageviewer` | video playing (`video.rs`) | **a live twin exists:** `apps/videoplayer`. Merge: the one kept first takes everything only the other has |
| process explorer | window picker, wait-chain and deadlock detection, CPU affinity and priority, memory map, environment (`features.rs`) | nothing else in the tree does affinity or deadlocks |
| system information | hardware queries (`pub mod hwquery`) | |
| `apps/settings` | the remote-settings page (`remote.rs`) | |

And, in the same change, remove the three `#![allow(dead_code)]` that hid them
(`video.rs`, `features.rs`, `remote.rs`), so the next feature that loses its
last caller is warned about.

**Also under this rule:** the shell's own unreachable launcher was compared with
`apps/launcher` before lane C deleted it (2026-09-27) -- yours had everything it
had. What `apps/launcher` lacks is the list of *installed* programs: it keeps its
own copy of the built-in database, where the start menu reads the desktop
entries (`gui/desktopentry`). Reading those would make it find what is actually
installed.

## 3. An event colour close to the accent: a warning, both ways (C-Q19, §1424)

The operator: "warn rather than don't allow, and in the warning, tell how to
change the offending event color(s) and/or have a link right there to changing
the event color(s)."

- **`apps/calendar`:** picking an event colour too close to the accent shows a
  warning (the dot would vanish into today's circle); the colour is kept as
  chosen.
- **`apps/settings`:** picking an accent too close to one or more events'
  colours shows a warning naming them, with a way straight to changing each.
- **Lane C** adds one test of "too close to see one on the other" to
  `appearance`, so the two warnings cannot disagree; its name will be in this
  request's reply when it lands.

## 4. The backup program's schedule is a service's input (C-Q21, §1426)

The operator chose a background service started at boot, which also runs a
backup missed while the machine was off, as soon as it is on again, without
asking. The service is lane D's and its starting lane B's
(`requests/c-db-a-backup-service-that-runs-at-boot.md`); the schedule file
`apps/backup` writes is its input -- the format is yours and lane D's to agree.

## 5. Each look keeps its own colours (C-Q15, §1421)

The accent and the other interface colours the user sets are to be kept per look
(Outlined, Filled). Lane C changes `appearance`; then Settings' colour page
edits the colours of the look being edited. Lane C will say when the API is in.

## 6. The password manager's plain-text export must escape every character (C-Q25, §1417)

Added from the operator's answer to B-Q12: "the proposed format for exporting
passwords from the password manager in plaintext was to do it in csv, but
passwords can have any characters, including any combination of characters used
to delimit a string or escape a character in csv, so make sure you don't mess
that up." A test with passwords containing commas, quotes, newlines, carriage
returns and leading spaces, exported and read back identical, is what shows it.

## If this is never done

Nothing gets worse than today: the games keep their colours, the five features
stay unreachable, the warnings do not appear, backups do not run on schedule.
