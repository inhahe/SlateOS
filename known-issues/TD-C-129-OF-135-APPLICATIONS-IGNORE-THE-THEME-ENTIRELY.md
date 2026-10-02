## TD-C-129-OF-135-APPLICATIONS-IGNORE-THE-THEME-ENTIRELY -- 129 is now 42, and all 42 are games

**Re-measured 2026-09-13: 43 of 137, and 42 of those are games.**

The count in the title was taken on 2026-09-08. Counting the same way today
-- an app crate that draws (`RenderCommand` or `RenderTree` in its source)
and never mentions a `Palette`:

| | then | now |
|---|---|---|
| crates that draw | 135 | 137 |
| of those, themed | 6 | 94 |
| unthemed | 129 | 43 |

The 43 are: asteroids, battleship, breakout, checkers, chess, connect4,
crossword, dots, flood, freecell, game2048, gomoku, hangman, klotski, life,
lightsout, mahjong, match3, maze, memory, minesweeper, nim, nonogram, pacman,
pinball, pipes, pong, reversi, rush, simon, sliding, snake, sokoban,
solitaire, spades, sudoku, terminal, tetris, tictactoe, towers, wordle,
wordsearch, yahtzee.

**Forty-two are games and wait on C-Q16**, which asks whether a game's board
should follow the theme at all. The measured reason there is no chrome-only
slice to take meanwhile is on
`TD-C-SIXTY-EIGHT-APPS-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`: the games do not
separate chrome from board at the constant level, so converting the ramp
themes the board, which is the very question.

**The forty-third was `terminal`, and it is done.** Its `ColorScheme` splits
exactly where the toolkit's text view splits: `foreground`, `background`,
`cursor` and `selection_bg` are the window's own furniture and now come from
the palette, with the selection taking the accent (839). The sixteen ANSI
entries do not move, and that is not an omission -- colour 1 is red because
the escape sequence says so, and a program that prints red expects red on
every terminal ever made. Retinting those would not theme the terminal, it
would corrupt what programs print. Both halves are asserted by
`the_chrome_follows_the_theme_and_the_ansi_table_does_not`.

**Date:** 2026-09-08. **Lane:** C.
**Where:** `apps/**` (135 crates that draw), and the gap that causes it:
`gui/window` (`oswindow`), which does not depend on `appearance` at all.

**In short:** pick the light theme, or an accent colour, or a high-contrast
scheme, and it changes the desktop, the window decorations and the Settings
window. It changes **nothing inside any other application**. 129 of the 135
applications that draw never mention `appearance::Palette`; they each carry
their own hardcoded Catppuccin Mocha constants. So a user who turns on high
contrast for legibility gets it on the window borders and on the desktop
behind, and then opens a text editor that is exactly as it was.

**Why it is not just cosmetic.** High contrast is an accessibility setting,
and `design-decisions.md` §816 made it reach every *surface* — the palette,
the decorations, the shell. What §816 could not reach is application
interiors, because there is no route to them. The setting therefore looks like
it works and does not, which is the failure mode this project keeps finding.

**The measurement.** Counting `Color::from_hex(0x…)` / `Color::rgb(…)` against
mentions of `Palette`, per crate under `apps/`:

| | count |
|---|---|
| applications that draw with colours | 135 |
| of those, never mention `Palette` | **129** |
| the six that do | `settings` (28 — converted 2026-09-08), `colorpicker` (3), `hearts`, `kanban`, `paint`, `stopwatch` (1 each) |

Not every literal is a bug: `paint`'s 80 and `colorpicker`'s are largely
*content* — the colours a user draws with, which must not follow the theme.
The test is whether the **chrome** (background, panel, text, selection)
follows it, and for the 129 it does not, because they have no palette to
follow.

**The cause is a missing route, not 129 independent oversights.** An
application gets its window from `oswindow::app::launch` and implements
`App::render(width, height) -> RenderTree`. Nothing in that trait, and nothing
in `gui/window`, offers a `Palette` — the crate does not even depend on
`appearance`. An application that wanted to follow the theme would have to
load and parse `appearance.yaml` itself, which is why none of them does.

**Correction, same day: less of the mechanism exists than I first wrote here.**
The first version of this entry said the change-notification already reaches
applications. It does not, and the direction is the opposite of what I
assumed. `oswindow::app::Reloads` and `EventLoop::appearance_changed` are
**outbound**: they are how the Settings application tells the *compositor*
that it has rewritten `appearance.yaml`. `App::take_reloads`'s own doc says so
— *"it exists for the handful — Settings, today exactly one — that edit files
another process reads."*

Inbound there is nothing. `guitk::event::Event` carries `Mouse`, `Key`,
`Resize`, `Moved`, `FocusIn`, `FocusOut` and `CloseRequested`, and no variant
about appearance at all. So an application is never told the theme changed,
and there is no palette for it to be told about. **Both halves are missing**,
which makes step 1 below larger than one dependency line.

**Proper fix, in order.**

1. **`gui/window` depends on `appearance`, resolves one `Palette`, and gains
   a way to hand it to the application** — most likely a new `Event` variant
   so a theme change arrives the same way a resize does, plus an accessor for
   the current palette so an application can paint its first frame correctly
   before any change has happened. Applications read it rather than each
   loading a config file — one parse per process, not 135 implementations of
   the same parse, and one place for the refresh edge to be right.
2. **Convert applications to it**, deleting their private constants. The
   Settings app is the worked example (2026-09-08): every role in those
   constant blocks maps one-to-one onto `Palette`'s fields under the same
   name, so the edit is mechanical once the route exists.
3. **A test per converted app** on the pixels it emits, not on the palette
   object — the Settings app's
   `the_settings_pages_follow_the_theme_they_are_used_to_choose` asserts that
   the same page draws different colours under two themes, which is the claim
   that matters.

**Progress, 2026-09-08.**

- **Step 1 is done.** `gui/window` depends on `appearance`, resolves one
  `Palette` per process from `appearance.yaml`, and hands it over through a
  new defaulted `App::theme_changed` — before the first frame, and again on
  every change. `design-decisions.md` §822 records why it is a trait method
  and not an `Event`.
- **Step 2: 23 done.** `calculator`, `diskcleanup`, `charmap`, `clipmanager`,
  `fileassoc`, `startupmanager`, `magnifier`, `systemrestore`, `videoplayer`,
  `qrcode`, `notes`, `rssreader`, `spreadsheet`, `paint`, `defrag`, `netscan`,
  `photomanager`, `renamer`, `mediaconvert`, `radio`, `flashcards`, `calendar`
  `finance`, `slides`, `worldclock`, `camera`, `compass`, `diskanalyzer`,
  `jsonviewer`, `logviewer`, `passwordgen`, `podcast`, `regextester`,
  `stickynotes`, `taskscheduler`, `contacts`, `habits`, `fontmanager` and
  `automator`, `sysmonitor`, `undelete`, `ircclient`, `dictionary`,
  `reminders`, `weather`, `alarmclock`, `diagram`, `partmanager` and
  `credmanager`, `vpnmanager`, `dbviewer`, `whiteboard`, `mindmap`, `tmux`,
  `remotedesktop`, `markdowneditor`, `netmanager` and `snippets`.
  Each has a test on the rectangles it emits, and each was mutation-checked by
  making `theme_changed` ignore its argument.

**"Done: every non-game application" was claimed on 2026-09-08 and was wrong
by sixteen applications.** Corrected the same day. The claim rested on the
survey, and the survey rested on a regex anchored `^const` -- which sees a
colour declared at file scope and does *not* see one declared `pub const`
inside a `mod mocha { … }` block, referred to as `mocha::BASE`. Sixteen
applications declare theirs that way: `emojipicker`, `unitconverter`,
`torrent`, `systray`, `filediff`, `diskimager`, `kanban`, `screenrecorder`,
`filesearch`, `email`, `launcher`, `hexeditor`, `soundrecorder`,
`colorpicker`, `archivemanager` and `lockscreen`.

**The failure mode is the one that matters here: it did not report them as
outstanding, it reported them as *finished*.** An application with no
file-scope constants looks identical to a converted one — both have zero — so
all sixteen landed in the "names no palette roles at all" bucket and the
totals still summed to 140. A survey that under-reports work looks exactly
like a survey that has found none, which is why the arithmetic adding up was
no evidence at all.

The converter now recovers the enclosing module for each constant and
substitutes the *qualified* name, since a bare `BASE` matches nothing at the
use site.

**And then a *fourth* shape turned up, by not trusting the survey a third
time.** With all sixteen done, the survey again said zero left. Rather than
report that, the tree was searched for the twenty Mocha hex values *in any
form* — and five more applications appeared: `editor` (42 sites),
`typingtutor` (18), `metronome` (13), `ebook` (13) and `settings` (2). They
declare no colour constants at all; they write `Color::from_hex(0x1E1E2E)`
**inline at the use site**. A survey that looks for `const NAME: Color = …` is
blind to those in both directions — it cannot report them as outstanding *or*
as done, so they simply never appeared in any count.

**The lesson, stated plainly because it has now cost three corrections:** the
survey counts *declarations*, and the thing that actually matters is *uses*.
Every time the declaration shape has varied — file scope, module scope,
underscore-prefixed, and now no declaration at all — the count has been wrong
in the direction that makes the work look finished. The check that has never
been fooled is grepping for the twenty hex values themselves and subtracting
the sites deliberately kept fixed. That is the check to run before saying
"done", and the survey is only a work queue.

**And a sixth: submodules.** Every audit and every converter run named
`apps/{app}/src/main.rs`. An application with more than one source file could
therefore hold a second palette in a file nobody ever looked at, and
`apps/settings` held three -- 38 constants across `snapshots.rs`, `remote.rs`
and `associations.rs`, each headed with the comment *"Theme colors (same
Catppuccin Mocha palette as main settings)"*, saying plainly what it was. The
declaration shape there is the **original** file-scope one, so nothing about
it was hard to find. It was simply never looked at.

**The audit that finally holds** is over every `.rs` under `apps/*/src`, not
`main.rs`, counting the twenty hex values and subtracting the sites
deliberately kept fixed:

```
for d in apps/*/src; do
  a=$(basename $(dirname $d))
  n=$(cat $d/*.rs 2>/dev/null | grep -ciE "0x(1E1E2E|181825|…)")
  [ "$n" -gt 0 ] && printf "%-16s %3d" "$a" "$n"
done
```

Final counts: **79 applications converted**, 42 games, and the only
non-game hex values left in `apps/` are ones deliberately kept:
`tmux` 52 (ANSI cells), `whiteboard` 22 (ink), `mindmap` 12, `stickynotes` 9,
`kanban` 7 (labels), `hexeditor` 7 (bookmarks), `snippets` 5 (folders),
`soundrecorder` 4 (markers), `screenrecorder` 3 (annotations), `settings` 2
(the theme-preview mockup), `editor` 2 (merge-conflict tints).

The earlier fix to `survey_all.py` still stands: it used to overlap its buckets
and report "0 already converted", because a converted application has no
constants left and so fell into the "nothing to convert" bucket, which was also
how its hardcoded games list silently hid nineteen games among the work still
to do. Three bugs in one survey, each of which made the remaining work look
smaller than it was.

**The sharpest form of the content-vs-chrome rule, learned from `snippets`.**
The earlier tell -- "the constant is read where no window is in scope" -- does
not fire here: `snippets` sets a folder's colour inside `&mut self` methods.
The reliable question is *when* the colour is resolved:

  * resolved **at draw time**, every frame -> chrome, follows the theme;
  * **written into a stored field** -> content, stays fixed.

A stored colour cannot follow the theme even in principle, because nothing
rewrites it when the theme changes -- the same reason `tmux`'s parsed ANSI
cells stay fixed. `snippets` has both kinds in one file: `Folder.color` is
stored (five sites, left fixed), while `Language::color` and `TokenKind::color`
are syntax highlighting computed per frame (twenty-two sites, themed).

**Check the package name before believing a build.** `apps/tmux`'s crate is
`tmux-app`, so `cargo build -p tmux` silently built something else and reported
success four times running while the file did not compile at all. A green
build of the wrong package looks exactly like a green build.

**Three bugs in one regex, all the same root cause: it pretends to lex Rust.**
The substitution has to avoid string literals, and getting that right took
three attempts, each of which produced a *silent* wrong answer rather than a
failure:

1. No protection — `dbviewer`'s SQL type name `"TEXT"` was rewritten.
2. Protection that could not span a backslash-newline string continuation. One
   unmatched string shifted every later match by a quote, so `tmux`'s
   attribute strings were rewritten *instead of* protected. Fixed with `re.S`.
3. Protection that spans continuations, but a `'"'` **char literal** is an odd
   quote: everything after `'-' | '"' => …` was treated as inside a string and
   silently left unconverted. Fixed by matching char literals in the same
   alternation, so their quote is consumed rather than treated as a delimiter.

**`tmux`'s ANSI palette stays fixed, and the reason is not the same as the
other content cases.** A terminal cell stores a resolved `Color`, written when
the escape sequence is parsed. Threading the desktop palette into the parser
would recolour only text printed *after* a theme change — a terminal half in
one theme and half in the other, which is worse than one consistently in its
own scheme. Doing it properly needs the cell buffer to store a colour *index*
and resolve at draw time, which is a terminal refactor rather than a theme
conversion. The window chrome — tabs, status bar, borders, panes — follows the
theme.

Its theme test also drops the equal-command-count assertion the others make: a
terminal skips drawing a cell whose background already matches the surface
behind it, so a different background legitimately changes how many rectangles
are emitted.

**`whiteboard` and `mindmap` are the clearest content cases yet**, and both
were deferred earlier for exactly the right reason. `whiteboard`'s constant is
the default *ink*; `mindmap`'s `NODE_COLORS` is the eight colours a node cycles
through. Both are saved with the document, so following the theme would mean a
saved drawing or map changing colour when the user changed theme. Both keep
fixed values, with the reasoning written where they are defined — and the rest
of each application (56 and 40-odd chrome uses) converted normally.

**The worst bug this conversion has produced: substitution inside string
literals.** `dbviewer` names a colour constant `TEXT`. The word-boundary
substitution therefore rewrote the *SQL type name* `"TEXT"` to
`"self.palette.text"` in seven places, including inside longer statements like
`"CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"`. The parser
stopped recognising `TEXT` columns. It compiles, clippy is clean, and only one
test — `test_parse_create_table`, which happens to cover `CREATE TABLE` —
failed.

Two things follow, both done:

- **The converter now skips string literals** when substituting.
- **The whole tree was audited** for a palette path inside any string literal,
  not just the exact literal the first grep matched: `"…name TEXT NOT NULL)"`
  contains the substring but is not equal to it, and a naive search missed two
  of the seven. `dbviewer` was the only affected application, and it is clean.

This is the argument for converting one application at a time and running its
own suite: a batch pass would have buried a logic change in a diff of colour
substitutions.

**A `const` table of colours cannot hold a palette, and the fix is a role
selector.** `vpnmanager`'s `TOOLBAR_BUTTONS` was
`&[(&str, Color, Target)]` — a top-level constant, so no runtime palette can
reach it. Changing the middle field to `fn(&Palette) -> Color` keeps the table
declarative and picks the role out at draw time; function pointers are
const-constructible and `|p| p.green` is one. Clippy then wants a `type` alias
for the tuple. This is the shape behind every "`<top level>` use" the survey
reports.

**The shape-2 fixer must handle both spellings of the palette.** The constant
substitution produces `self.palette.<role>`; the `.color()` call-site rewrites
produce `&self.palette` passed on to another function. A fixer that rewrote
only the first left 22 errors in `credmanager` that read as a new problem and
were the same one. It now rewrites both.

**Run the shape-2 fixer on every application, not just the ones the survey
flags.** The survey lists functions needing a *parameter*; it does not list
the ones that already take the app struct, because those need no signature
change. Skipping that pass on `partmanager` produced 113 errors that looked
like a catastrophe and were one missing step.

**Two attributes on one item after a deletion.** `alarmclock` had
`#[allow(dead_code)]` on `SKY`, a comment about `SKY`, then
`#[allow(dead_code)]` on `MAROON`. Converting `SKY` left its attribute
stranded *above a comment*, which the sweep deliberately skips — and the
result is two attributes on `MAROON`, which is
`clippy::duplicated_attributes`, an error. The general rule stands (an
attribute followed by a comment is usually fine); this is the exception, and
it is caught by clippy rather than by the sweep.

**Adding a reference can make an elided lifetime ambiguous.**
`reminders::detail_prose(text: &str, …) -> text::Paragraph<'_>` compiled while
`&str` was its only reference; adding `pal: &Palette` made `'_` ambiguous and
the signature needed a named lifetime. Rare, but it is a *signature* change
rather than a call-site one, so it does not look like the others.

**Several types per application share one colour method.** `undelete` has
three (`FileSignatureKind`, `FileCategory`, `RecoveryConfidence`),
`sysmonitor` two, `ircclient` two. The threader now handles every occurrence
rather than the first. Its first version had a subtle bug worth avoiding: it
located each rewritten signature with `s.index(new)`, and since the types
share an identical signature string, `index` returned the *first* every time,
so the body rewrite landed on the same method repeatedly while the others were
left half-converted. Use the match position.

**Wrapper methods propagate the requirement.** `sysmonitor`'s `cpu_color`,
`mem_color` and `disk_color` do nothing but call `color_for_value`, so giving
that one a palette gives all three one. Expect a small cascade whenever the
colour method has callers of its own inside the same type.

**A fourth content case, and the tell held again.** `contacts` sets a new
group's colour in `Group::new` — the swatch that group is shown with, chosen
by the user. Fixed colour, not the theme. As with `paint`, `whiteboard` and
`stickynotes`, the constant was read where no window is in scope.

**Where the remaining time actually goes.** Not the substitution, which is
reliable, but four kinds of call site the scripts cannot rewrite safely:
generic signatures (`fn f<T: PartialEq + Copy>(…)`, which the parameter regex
does not match), calls whose first argument is an expression rather than an
identifier, calls spanning several lines, and *argument order* — a parameter
inserted second must be passed second, and the compiler reports that as a type
error rather than an arity one. All are compiler-visible; none is automatable
without the risk that produced 91 errors in one application earlier.

**Content that only *looks* like chrome, a third time.** `stickynotes`'
`note_palette` reads two constants — but only as the fallback for an
out-of-range note colour, and a note's colour is the swatch the user picked,
not chrome. It keeps fixed colours, like `paint`'s swatches and
`whiteboard`'s default ink. The tell each time was the same: the constant is
read somewhere that has no window in scope (a `Default` impl, a lookup table,
a fallback arm), which is a hint that it is describing *content* rather than
the surface the content sits on.

**Closures rebind the window.** A call-site rewrite to `&self.palette` is
wrong inside `|u, f, r|`, where the window is `u` — three sites in
`taskscheduler`'s test helpers. The compiler names them, but it is worth
knowing that "the window is always `self`" fails in closures as well as in
other types' methods.

**The one conversion bug that a test would not have caught.** A script that
gives `#[test]` functions a local palette matched a *production* function too
and inserted `let pal = Palette::from_settings(&default())` inside
`jsonviewer::highlight_json_text` — **shadowing the parameter**. The function
then ignored the palette it was handed and always drew in the defaults: it
compiles, every test passes, and the only symptom is JSON syntax highlighting
that does not follow the theme while everything around it does.

It surfaced as `unused variable: pal` from clippy, not from any test. Two
things follow. Any script that inserts a binding must check that the enclosing
function does not already have one of that name. And the whole set is worth
auditing for it: `grep` every converted application for a `let pal =` inside a
function whose signature already says `pal: &Palette`. That audit found this
one and no others.

**One application refused conversion, correctly: `whiteboard`.** Its
`MOCHA_TEXT` is read in a `Default::default()` — the default *pen* colour — and
a `Default` impl cannot take a palette. The colour is also content rather than
chrome: it is the ink the user draws with, like `paint`'s swatches. Reverted
rather than forced. The general rule this makes concrete: a constant used in a
`Default` impl is usually content, and the hex-value match cannot tell the two
apart when the chrome and the ink happen to be the same colour.

**Two more shapes met here.** A *recursive* free helper
(`diskanalyzer::squarify_layout`) needs the palette threaded through its own
recursive call as well as its callers, and the public entry point above it
(`compute_treemap`) too. And a helper reached only from a constructor
(`slides::SlideTheme::mocha`, called by `SlidesApp::new`) cannot take the live
palette at all, because none exists yet — it takes the defaults, and
`theme_changed` replaces the result before the first frame.

**A nested case the blanket call-site rewrite gets wrong.** `.color()` becomes
`.color(&self.palette)` everywhere, which is right in the window's methods and
wrong *inside another palette-taking method*: `CalendarEvent::effective_color`
calls `self.category.color(…)`, where `self` is the event, so the argument has
to be its own `pal`. The compiler catches it (E0609 on the app's own type), but
expect one per application that has a colour method calling another.

**The single-helper case is now scripted too.** Every application that needs
one function threaded needs the *same* one: a `color()` method on a domain
enum (`BlockState`, `PortState`, `JobStatus`, `RenameOp`, `ColorLabel`). One
script handles signature, body and call sites; only the calls inside `#[test]`
functions need a hand, and the compiler names them.

**The "129" in this entry's own table was wrong, and the real number is
smaller.** That figure counted crates containing a hardcoded colour, which is
not the same as crates that ignore the theme. Surveying all 140 applications
that draw:

| | count |
|---|---|
| applications with a `main.rs` | 140 |
| **name no palette role at all** — nothing to convert | **48** |
| games (deprioritised by the operator's standing instruction) | 23 |
| converted | 14 |
| **genuinely left** | **~55** |

The 48 matter to the estimate and to correctness both. A crate full of
`Color::rgb(…)` calls is usually naming *content*, not chrome — a paint
program's swatches, a syntax highlighter's token colours, a disk map's
category fills — and converting those would be a bug, not a fix. Matching on
the *hex value* against the known Catppuccin roles is what separates the two
automatically: `paint` had 16 chrome constants (converted) and 64
`Color::rgb` swatches (untouched, and they must be).

**Of the ~55 remaining, 11 need no hand-threading at all** and the rest average
one to three helper functions each; the survey names them per application.

**Survey before converting.** A script that groups every constant use by its
enclosing `impl` block answers, in one pass and before any edit, the only
question that decides the cost. The difference is large enough to choose work
by: `magnifier` was 26 uses with nothing to hand-thread, `systemrestore` 152
uses with one method, while `netmanager` (13 methods), `vpnmanager` (9) and
`credmanager` (7) are several times the work for the same number of
constants. Survey first, then take the cheap ones in batches.

**Known remaining costs, from that survey:** `netmanager` 13 hand-threaded
functions, `vpnmanager` 9, `credmanager` 7, `contacts` 3. Every one of them is
a `color()`-style method on a *domain* enum — `ConnectionState`,
`SecurityLevel`, `VpnProtocol`, `LogLevel`, `PasswordStrength` — which is the
shape worth expecting: applications give their own types a colour method, and
those types are never the window.

**Batch conversion was tried and abandoned; do not retry it as written.** The
substitution half automates well — mapping by *hex value* rather than by
constant name is the trick, since the names vary
(`COLOR_TEXT`/`COL_TEXT`/`MOCHA_TEXT`) while the values are the one palette.
What does not automate is the part after it. Four applications were converted
in one pass and three had to be reverted:

| app | errors after the automated pass |
|---|---|
| `charmap` | 1 (a missing field initialiser) — kept |
| `fileassoc` | 18 — reverted |
| `startupmanager` | 28 — reverted |
| `clipmanager` | 91 — reverted |

The difference is not size, it is **where the drawing lives**. `charmap` and
`calculator` draw from `&self` methods, so the substitution
`CONST` → `self.palette.<role>` is the whole job. `clipmanager` draws from
free functions that take `state: &AppState`, where the same substitution has
to produce `state.palette.<role>` instead — and a script that rewrote call
sites with a regex to thread a new parameter made 91 errors out of 13.

**So: convert one application at a time, and look first at how it draws.**
Three shapes, in increasing cost:

1. **Colours used only in `&self` methods** — pure substitution, done in one
   pass. `charmap`, `calculator` (nearly).
2. **Free functions that already take the application struct** — substitute to
   `<param>.palette` instead. No signature changes, still mechanical, but the
   script has to know the parameter's name.
3. **Free or associated helpers taking neither** — these need a
   `pal: &Palette` parameter threaded through their call sites, and that is
   the part to do by hand. The compiler names every one of them (E0424,
   *"expected value, found module `self`"*), so the work is bounded and
   visible; it is the *automatic rewriting of call sites* that is not safe,
   particularly where a call spans several lines.

**Shape 2 does automate, and now does.** `clipmanager` was the worked example:
52 sites, of which **48** were shape 2 and were rewritten in one pass, leaving
4 to thread by hand. The script that failed the first time was scanning for
each function's body by counting braces — which is unreliable in Rust source,
because `{}` inside a format string unbalances the count and the walk then
skips the rest of the file in silence, reporting zero work to do. Scanning
*backwards* from each line that mentions the palette to its enclosing `fn` has
no such failure mode and is what works.

**The shape test must be "which `impl` block is this in", not "does it take
`self`".** "Has a `self` receiver, so `self.palette` is correct" is false
whenever `self` is not the window, and every application converted so far has
had at least one such method: `ClipType::badge_color`,
`FileCategory::color`, `StartupImpact::color`, `StartupEntry::status_color`,
`StartupStats::impact_color`. The compiler catches each (E0609, "no field
`palette` on type …"), but the useful fix is to group palette uses by their
enclosing `impl` before touching anything — a five-line scan that names the
non-window impls up front instead of discovering them one build at a time.

**A name collision to expect: `palette` may already mean something.**
`paint` has a `palette: Vec<Color>` field — its forty-eight drawing swatches —
so the theme went in as `theme` instead. Check the struct for an existing
`palette` before adding one, and be careful that a blanket
`self.palette.` → `self.theme.` rename does not catch the application's own
uses: it caught `self.palette.iter()`, which iterates the swatches.

**Three smaller things that recur:**

- **Stranded attributes, and the sweep for them is the dangerous part.**
  Deleting `const COLOR_X: Color = …;` lines leaves behind any attribute that
  annotated them, which is `clippy::empty_line_after_outer_attr` — an *error*
  in this workspace, and one `cargo test` does not catch, so a crate can test
  green with a broken build. But a general "attribute followed by nothing"
  regex is worse than the problem: mine matched an attribute followed by a
  *comment* and deleted `#[cfg(test)]` from `magnifier`'s test module, which
  compiled the whole module into the binary and produced 36 warnings. Two
  rules that hold: an attribute followed by a comment is **not** stranded, and
  after any such sweep check `git diff | grep '^-#\['` and read every
  attribute it claims to have removed.
- **Method references stop composing.** `map_or(default, ClipType::badge_color)`
  cannot survive `badge_color` gaining a parameter; it has to become a
  closure.
- **Tests call these helpers too.** A blanket call-site rewrite to
  `&self.palette` lands inside `#[test]` functions that have no `self`; those
  want a locally built default palette instead.

**The pattern, so the rest are mechanical.** Per application: add
`appearance` to `Cargo.toml`; add a `palette: Palette` field seeded from
`AppearanceSettings::default()` so it is never absent; implement
`theme_changed` to store it; replace each `COLOR_*`/`MOCHA_*` constant with
the `Palette` field of the same role; delete the constant block. Then a test
that renders under two themes and asserts the same number of commands with
different colours.

**Two things that recur and are worth knowing in advance:**

- **Associated functions with no `self`.** Most colour uses are in `&self`
  methods and become `self.palette.<role>`, but every application has a few
  free or associated helpers (`key_colors`, `render_key`,
  `render_status_button` in `calculator`) that need a `pal: &Palette`
  parameter threading through their call sites. The compiler finds them all;
  they are the only part that is not a substitution.
- **`clippy::field_reassign_with_default`.** The obvious way to write the test
  — `let mut s = AppearanceSettings::default(); s.theme_mode = …;` — is a
  clippy error in this workspace. Build the settings in one struct-update
  expression instead.

**Roles that are not in the shorter constant blocks.** `Palette` carries
`blue`, `yellow`, `mauve`, `teal`, `peach`, `lavender`, `green` and `red` as
well as the neutrals, so an application naming a hue by name converts
one-to-one. Nothing so far has needed a colour the palette does not have.

**Do not start at step 2 for an application before step 1 existed** — that is
now moot, but the reason stands for any similar sweep: converting before the
route exists means making each application load `appearance.yaml` on its own,
which is 129 copies of a parse and 129 places for the reload edge to be got
wrong.

**Related and already done:** `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-
COPY-OF-THE-PALETTE` did exactly this for the shell's 49 modules, and
`design-decisions.md` §810 removed the toolkit's copy. `guitk::theme` even
carries a test (`this_module_names_no_colours_of_its_own`) that fails if a
colour literal returns to it, and its own comment names the remaining copy as
being "in `apps/`". This entry is that copy.
