## `C-SETTINGS-ACCOUNT-PICTURE-GRID-WAS-DRAWN-BUT-NOT-SELECTABLE` (lane C, 2026-08-20) — FIXED

**In short:** Settings → Accounts → Login Options offers six little pictures
under "Choose a picture for your account". It looked like a choice and was not
one: the first picture was always outlined as the chosen one, clicking any of
them did nothing, and nothing anywhere stored which one you had picked. Now
each picture is clickable, the outline follows the one you chose, and your
choice shows up as the avatar beside your name in the account list.

**Where:** `apps/settings/src/main.rs`, `build_login_options_page`.

**What was wrong.** All six tiles were painted inside a single `s.draw(…)`
closure that computed its own `icon_size`, `icon_spacing` and per-tile `x`
inline, with `let is_selected = idx == 0;` hardcoded. A closure is opaque to
the hit sink — the page walk sees one drawing call, not six tiles — so there
was nowhere to register six click bands even if somebody had wanted to. This
is the §474/§475 shape once more: geometry written inside the paint, where no
other consumer can reach it.

Separately, `UserAccount` had no field for a picture, so there was nothing for
a click to write; and the account list drew a hardcoded `"\u{1F464}"` avatar
for every row regardless of who the row was.

### Fixed 2026-08-20

**The tile geometry is named once**, beside the theme cards which needed the
same treatment: `ACCOUNT_PICTURES` (the six icons, in draw order),
`PICTURE_TILE_SIZE`, `PICTURE_TILE_SPACING`, `picture_tile_dx(index)` and
`render_account_picture`. The grid loop now runs one iteration per tile,
calling `s.hit_rect(x + picture_tile_dx(idx), …)` and then `s.draw(…)` at the
same offset, so the square the user sees and the square a click resolves to
come from one expression.

**`UserAccount::picture` is an index into `ACCOUNT_PICTURES`,** not an icon
string, so the grid the user picks from and the avatar drawn in the account
list cannot come to offer different sets of pictures. The three default
accounts were given distinct pictures (Alice 👩, Bob 👨, Charlie 👶) — partly
so the list looks like a list of people, and partly because three identical
avatars would let a hardcoded-avatar regression pass unnoticed.

**The choice is written to the account marked `is_current`, not to
`user_accounts[selected_account]`.** These coincide on a fresh state, which is
exactly why it is worth stating: the Login Options page says "your account", so
which row the Accounts page happens to have *highlighted* must not decide whose
picture changes. `set_current_account_picture` also refuses an index past the
end of `ACCOUNT_PICTURES`, so the stored field can never name a picture that
does not exist.

**Nothing is ringed when nobody is signed in.** `current_account_picture`
returns `Option<usize>`, and `None` draws as "no tile outlined" rather than as
tile 0 — a machine with no signed-in account should not claim a choice was
made.

### Testing

Seven tests (175 green, clippy clean, rustfmt clean). The tiles are recovered
from the **render tree** and driven with real `handle_click` calls, so where a
tile is drawn and where a click lands have to agree without either being asked
to describe the other. The outline is looked up across the whole tree rather
than assumed to follow its own tile's fill, so a ring painted over the wrong
tile is a mismatch instead of being silently credited to the right one.

Twelve mutations, all twelve caught. Three are worth recording:

- **The bands were probed at the corners, not just the centre.** Shifting every
  band 12 px off its tile was *not* caught at first: the bands are as wide as
  the tiles, so a small offset still leaves the middle covered. Only a click
  near an edge tells whether the clickable square is the square on screen.
  General form: **a control's hit band is a rectangle, and a probe at its
  centre tests a point, not a rectangle.**
- **A geometry mutation that moves the paint and the band together is
  invisible to any test that checks them against each other** — which is the
  price of collapsing them into one expression, and the right price. What
  catches it is a check against something with no shared origin:
  `the_picture_tiles_stand_apart_inside_the_content_column` measures the
  painted tiles against the window, so overlapping tiles (the leftmost of a
  stack would be the only reachable one) and a strip running past the content
  column both fail.
- **The expected avatars are spelled out, not derived from the accounts.** A
  list that drew one hardcoded avatar three times — which is what it did — would
  otherwise have passed by agreeing with itself.

See `design-decisions.md` §479.
