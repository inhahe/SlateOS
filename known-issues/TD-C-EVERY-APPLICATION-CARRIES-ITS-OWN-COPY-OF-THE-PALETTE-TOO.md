### TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO — 2026-08-22 — OPEN

**In short.** The same defect as
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`, four times
larger, in `apps/` rather than the shell. **2,258** `const NAME: Color`
declarations across **135 files** in **~90 applications**, all of them the dark
theme written out by hand. So a user who picks Light gets a light shell (once
that entry's part 2 lands) containing entirely dark applications — which is a
worse-looking result than today, where at least everything is uniformly wrong.

**Where:** `apps/**/src/*.rs`. 240 distinct names, and the top of the list is
the same ladder the shell had: `BASE` 103, `BLUE` 103, `SURFACE0` 102,
`SUBTEXT0` 102, `GREEN` 102, `PEACH` 101, `OVERLAY0`/`RED`/`YELLOW` 100 each,
`MANTLE` 99, `SURFACE1` 97, `TEAL` 86, `LAVENDER` 82, `MAUVE` 81, `CRUST` 79.
Worst single files: `apps/mahjong/src/main.rs` (28), `apps/checkers` (27),
`apps/reversi` (27), `apps/gomoku` (26), `apps/sysinfo` (26).

**Measured, so it can be re-measured:** match
`^\s*(?:pub )?const ([A-Z0-9_]+)\s*:\s*Color\s*=\s*(.+?);\s*$` over
`apps/**/*.rs`, excluding `target/`.

**The copies mostly agree — but not entirely, and the exception is in the
authority.** Grouping each name by value shows nearly every "disagreement" is
one colour spelled two ways: `BASE` is `Color::from_hex(0x1E1E2E)` in 98 places
and `Color::rgb(30, 30, 46)` in 4 (`apps/benchmark` spells the whole ladder in
decimal). One name is a genuine special case rather than a disagreement —
`apps/launcher/src/main.rs` has `BASE = Color::rgba(30, 30, 46, 240)`, a
translucent panel, the identical case the shell had, resolving the same way to
`Palette::panel_bg()`.

And one is simply **wrong**: `SKY`. Catppuccin Mocha's Sky is `#89DCEB`, which
is what `apps/calendar`, `apps/charmap`, `apps/diagram`, `apps/finance`,
`apps/ircclient`, `apps/logviewer` and `gui/toolkit/src/theme.rs` all say. But
`gui/appearance/src/lib.rs:61` — the crate that *owns* the answer — says
`0x89DCFE`, and that value had already propagated into `apps/alarmclock`,
`apps/emojipicker` and `apps/unitconverter`. It is a one-character
transposition (`EB` → `FE`), and it means `AccentColor::Sky.color()` returns a
colour Catppuccin does not contain, so a user who picks the Sky accent gets a
slightly-off blue everywhere the setting actually reaches. In
`apps/unitconverter` it was not merely declared but *drawn*: the
`Category::DigitalStorage` label renders in it.

**All four fixed 2026-08-22, and the fix was scoped by measurement rather than
by grep-for-`SKY`.** The same independent transcription that found it was run
over the whole lane: every `const NAME: Color = Color::from_hex(0x……)` whose
name is a published Mocha role, compared against a hand-transcribed copy of the
published palette. **1,566 such declarations across `apps/`, 389 across `gui/`;
exactly one name mismatched, in exactly those three files.** So the duplication
problem this entry describes is real, but the *divergence* problem turned out
to be a single transposed byte pair — which is worth knowing before part 2,
because it means the conversion can treat the existing constants as faithful
copies and does not need a per-file value audit on top of the rename.

`gui/appearance` is now protected against the recurrence by
`every_dark_constant_is_the_published_catppuccin_mocha_value`, which pins all
24 declared values against an independent transcription. It is the only test in
that crate whose expected values do not come from the tree, and it is the only
one that could have caught this — every other test compares one part of the
tree against another, which by construction cannot see an error the whole tree
shares. Proved by reintroduction (harness defect T).

That the wrong copy is the *authority's* is this entry's point in miniature:
2,258 duplicates agreeing with each other is not evidence that any of them is
right, and the one place a mistake actually matters is the place that has no
copy to be checked against.

**Two things this changes about the shell entry's part 2, which is why it is
filed now rather than when someone gets to it:**

1. **`Palette` needs `teal` and `sky`.** [DONE 2026-08-22] It carried eight
   named hues; the applications use `TEAL` 86 times and `SKY` 18, and neither
   existed on the struct. Catppuccin defines 14 accents and `AccentColor`
   already names all of them, so the shortfall was in `Palette`'s field list,
   not in the model underneath it. Added immediately rather than when `apps/`
   is converted, because every mode-flip and legibility sweep in that crate
   iterates a hand-written list of the fields — so a hue added later has to be
   threaded back through all of them, and a sweep that silently skips a field
   is the precise failure those sweeps were written to detect. `roles()` is now
   21 wide and `every_named_hue_agrees_with_the_accent_of_the_same_name` covers
   ten hues.
2. **The "text on a coloured button" constants are wrong in light mode and
   will silently stay wrong.** `ALLOW_TEXT`, `DENY_TEXT` and
   `BUTTON_PRIMARY_TEXT` in the shell — and their equivalents across the apps —
   are all `0x1E1E2E`, i.e. the Mocha *base*, chosen because dark text reads on
   a pale green button. Translating them to `p.base` is the obvious move and is
   a bug: Latte's base is `#EFF1F5`, so a light-mode Allow button would be pale
   text on a pale green fill. They must become `readable_on(p.green)` and
   friends. This is the one place in the whole sweep where a mechanical
   name-to-role substitution produces a *new* defect rather than preserving the
   current one.

**Why it shipped:** the applications were written against the shell's
convention, which was to declare the palette locally, because there was nothing
to declare it from — `gui/appearance` could not answer what a window background
is in light mode until 2026-08-22. Each app copying the previous app's header
block is the mechanism; `#[allow(dead_code)]` is not implicated here the way it
was in `gui/desktop` (see `TD-C-DEAD-CODE-IS-ALLOWED-WHOLESALE`), because these
constants are mostly used.

**Proper fix:** the same as the shell's part 2 — take `&Palette` in the render
path and delete the constants — but sequenced *after* it, and after the two
changes above, because an application should be adopting a type the shell has
already proved rather than the two of them converging on it separately. Some
per-app colour is legitimate and must **not** be collapsed: a chessboard's
light and dark squares, a card back, a syntax-highlighting scheme, and a
resource graph's categorical series are the application's own decisions and
merely happen to be drawn from Catppuccin today. The test to apply, per
colour: *would a user who changed their accent expect this to move?* If no, it
stays a constant — but it should then be named for what it is (`BOARD_DARK`,
not `SURFACE1`), because a role name on a value that must not follow the theme
is how the next person collapses it by mistake.

**Trigger:** immediately after
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE` part 2.
Nothing else blocks it.

**If never fixed:** the appearance settings reach the desktop and stop at the
window edge. Every application stays dark on a light desktop, which reads as
each application being broken rather than the theme being unimplemented — the
same misdiagnosis the shell entry describes, multiplied by ninety.
