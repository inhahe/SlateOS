## TD-ONLY-ONE-KEYBOARD-LAYOUT (lane C, 2026-08-17)

**Status:** FIXED 2026-08-24 (`95439d0fc`, `keylayout`: one table the compositor types with and the shell draws); the layout is the user's `input.yaml` choice, and `compositor`'s tests type through Dvorak and German QWERTZ. On `main` since. Stamped 2026-09-28 by lane F, the compositor's owner since the six-lane split.

**What.** `gui/compositor/src/keymap.rs` holds one hard-coded US-QWERTY
scan-code-set-1 table, and there is no way to select another. Anyone using a
non-US keyboard gets the wrong letters — a French AZERTY user pressing the key
labelled `A` produces `Q`.

**How it arose.** `design-decisions.md` §456 put scancode→key translation in the
compositor (rather than in each of 138 client crates) so that one system keymap
governs everything and a layout change takes effect everywhere at once. That is
the right *structure* for layouts; this entry is the observation that the
structure now exists and is occupied by exactly one table.

**What is missing**, roughly in order of how much each is felt:

1. **Layout selection** — a setting, a way to read it, and the table swap. The
   table is already consulted in exactly one place (`key_for_scancode`), so this
   is a lookup change rather than a redesign.
2. **More tables.** Each is mechanical: the same 88 rows with different letters.
3. **Dead keys.** On many European layouts `´` then `e` produces `é`. That needs
   per-layout state between two key events, which nothing here has — the
   translation is currently a pure function of one scancode.
4. **Compose sequences** (`Compose`, `o`, `c` → `©`), the same shape of problem
   as dead keys but with longer sequences.
5. **AltGr as a level shift.** `Modifiers` has no AltGr; right Alt currently maps
   to `Key::RightAlt` and sets the `alt` flag, so a layout where AltGr+`2`
   produces `@` cannot be expressed.

**Severity.** Low today, because nothing else about the input path is connected
yet — see `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` — so no user is currently
getting the wrong letter. It becomes a *correctness* issue the moment a real
client exists and someone outside the US uses it, and (3)–(5) are the kind of
thing that is much cheaper to design in than to retrofit, because they change
`key_for_scancode` from a pure function into something that holds state.

**Where.** `gui/compositor/src/keymap.rs` — the `key_for_scancode` table, and
`Modifiers` in `gui/toolkit/src/event.rs:185` for the AltGr gap.

**Update 2026-08-24 — (1), (2) and (5) are done; (3) and (4) remain.** The
layouts now live in `gui/keylayout`, a dependency-free crate both the
compositor and the desktop shell read; see `design-decisions.md` §549.

- **(1) Layout selection** — `inputsettings`' `keyboard.layout` names a layout
  by id, `Compositor::set_input_settings` resolves it through
  `keylayout::by_id` on every reload, and `keymap::key_for_layout` translates
  with it. An id this build does not know leaves the layout already in force
  alone rather than leaving the user with no keyboard, and the name is
  preserved verbatim in the file so a settings file written by a later build
  survives a round trip through this one.
- **(2) More tables** — eight: `us-qwerty`, `uk-qwerty`, `dvorak`, `colemak`,
  `workman`, `de-qwertz`, `fr-azerty`, `es-qwerty`. Each is eight row strings,
  checkable against a photograph of the board. The three alternate English
  layouts are pinned by a test proving them exact rearrangements of US QWERTY's
  own character multiset, so a mistyped row cannot pass as a layout.
- **(5) AltGr as a level shift** — done, and without changing `Modifiers`,
  which is constructed as a literal 99 times across 33 files including one
  lane-A file. A keystroke that resolves through the third level *clears*
  `modifiers.alt` instead of setting a new flag: the case that matters is a
  German user typing `@` (AltGr+Q), which previously reached every application
  as Alt+Q with the menu bar answering first.

**(3) Dead keys and (4) compose sequences are still open, and are still the
part that is cheaper to design in than to retrofit.** Both need state carried
between two key events, and `Layout::character` is still a pure function of one
scancode and one level. Nothing offered today needs them — the eight layouts
above were chosen partly for that — but German's `´`, French's `^` and
Spanish's `´` are all *present as ordinary characters* on their boards, which
is a real difference from how those keyboards behave. That is the shape of the
remaining debt: not a missing feature so much as three layouts that type a
standalone accent where a real one would compose.

**Update 2026-08-24, later — (3) is under way: the event can now describe a
dead key, though nothing produces one yet.** This is step 1 of four, and it is
deliberately only the shape:

- `guitk::event::KeyEvent::text` is a `String` rather than an `Option<char>`.
  That is what makes the two cases dead keys need *expressible*: a keystroke
  that types **nothing yet** (distinct from `None`, which already meant "this
  key produces no text at all" — F5, an arrow) and one that types **two
  characters**, which is what a failed composition must emit. See
  `design-decisions.md` §550, which also records the failed-composition policy:
  `´` then `x` types `´x`, following Windows and macOS rather than X11, because
  silently discarding a keystroke is the one failure a text field must not have.
- `guiremote::PROTOCOL_VERSION` is 3. The old encoding (a present-flag plus one
  codepoint) could not carry either case, and a version-2 decoder reading a
  version-3 stream desynchronises *silently* rather than erroring.
- Forty-odd call sites were converted. Reading the field raw is now wrong almost
  everywhere: `KeyEvent::typed()` yields the characters minus control
  characters, and `KeyEvent::types_text()` answers "did this keystroke belong to
  the text field at all?". Seven sites had been missing the control filter and
  would put `\x1b` in a search box when the user pressed Escape; those are fixed
  as a side effect of the rule now having one home.

**Still to do for (3):** layouts must be able to declare a key's face *dead*
(step 2, in `gui/keylayout`, composing through the exact NFC tables already in
`gui/font/src/norm.rs` rather than a second hand-written accent table), and the
compositor must hold the pending accent between two key events (step 3, which is
the part that turns `Layout::character` from a pure function into a machine with
a memory). **(4) compose sequences remain untouched** and are the same shape of
problem over longer sequences.

**Update 2026-08-24, later still — step 2 is done, in both halves. Only the
compositor's memory (step 3) is left.**

*Step 2a — which faces are dead* (`38348bf9b`, `gui/keylayout`). `KeyDef` gained
`dead: DeadFaces`, four flags, one per level. Four and not one because **deadness
belongs to a key's face, not to a character**: French AZERTY carries `^` twice —
plain on the key right of `P`, where it is dead and makes `ê`, and on AltGr+9,
where it is the ordinary ASCII circumflex a programmer types into a shell. Any
per-*character* "is this an accent" table is necessarily wrong about one of them.
The three national layouts declare seven dead faces between them; the five
English ones declare none, which a test asserts rather than assumes.

The load-bearing part is not the flags but a refactor next to them.
`KeyDef::face(level) -> Option<Face>` now decides *once* which of the four levels
the modifiers select, and both `character()` and `is_dead()` read its answer, so
they are structurally unable to disagree. The case that forces this: AltGr+Shift
on a key with no fourth-level character falls back to the AltGr *character*, so
its *deadness* has to fall back too — otherwise the key types a live `~` while
the compositor sits waiting for a vowel, and the user's next letter vanishes.

*Step 2b — what an accent and the next key make* (`30b0864a7`,
`gui/font/src/deadkey.rs`). No accent table: composing `e` and an acute into `é`
is exactly the question `norm.rs` already answers on every string it shapes, from
generated UAX #15 tables covering ~1050 pairs. A hand-written
`match ('e', '´') => 'é'` would be a second answer to that question — shorter,
and therefore the one that eventually disagrees, most likely by omitting the
letter nobody thought to list. So the module adds one thing: the map from the
*spacing* accent a key cap carries (`´` U+00B4) to the *combining* mark the
tables are indexed by (U+0301). Thirteen accents, through the eighteen
characters keyboards print them as.

U+00B0 DEGREE SIGN is deliberately **not** in that map, and a test pins its
absence. It is indistinguishable from U+02DA RING ABOVE on screen, and German
puts it on the shifted face of a key whose plain face is a dead circumflex —
precisely the arrangement in which a mis-declared `°` would quietly compose `å`.

**Still to do for (3):** the compositor must hold the pending accent between two
key events, implement §550's failed-composition policy (type both characters),
and decide the two conventions that are facts about keyboards rather than about
Unicode — what a dead key pressed twice does, and what a dead key followed by a
space does. It is also the only crate that links both `keylayout` and `osfont`,
so it is where a test belongs asserting that every dead face in every builtin
layout maps to a real `deadkey::combining` entry. Until that lands, the three
national layouts still type a standalone accent where a real board would compose.

**Update 2026-08-24, later still — step (3) is done; dead keys work end to
end.** `gui/compositor/src/deadkey.rs` holds the pending accent: an
`Option<char>` on the compositor beside `ModifierState`, consulted from
`handle_key` on the path where the compositor did the translation, disarmed by
`release_all_modifiers` and by a focus change. Typing `´` then `e` on
`de-qwertz` now produces one key event carrying no text and a second carrying
`é`.

The two conventions the note above asked for — plus a third it did not
anticipate — are decided and recorded as `design-decisions.md` §551. **Space**
types the bare accent alone: the only way to type a `´` at all on a board where
that key is dead, and the escape hatch that lets the third rule discard safely.
**A second dead key** flushes the first and re-arms, checked *before* the
composition table is consulted, because `¨` really does compose with an acute
into U+0385 GREEK DIALYTIKA TONOS — an ordering that is invisible except in
that one case. "Pressed twice types one accent and leaves one waiting" then
falls out of that with no rule of its own, which is why it needed no separate
decision. **A key that types no text** discards the accent, Backspace being the
case that decides it, with modifier keys excluded so that `É` stays typeable.

The cross-crate test the note asked for exists as
`every_dead_face_in_every_builtin_layout_composes_with_something`, and it
counts rather than merely sweeping: thirteen dead faces reached across the
three national layouts, so a layout that lost its `dead` block fails here
instead of passing by checking nothing.

**Found and fixed on the way:** `key_for_layout` answered `None` for the space
bar, because a `Layout` is the alphanumeric block and space is not in it. On
the evdev path — every real machine — the space bar therefore typed no text at
all and no text field could hold a space. The host backend hid it completely,
because it supplies its own character. Fixed by
`keymap::text_outside_the_block`; the numeric keypad is the same shape of gap
and is now tracked as
`TD-C-THE-NUMERIC-KEYPAD-TYPES-NOTHING-BECAUSE-NOTHING-TRACKS-NUM-LOCK`.

**What remains of this entry is step (4) only:** compose sequences (`Compose`,
`o`, `c` → `©`). Unlike a dead key, which remembers exactly one character, a
compose sequence is a prefix tree of arbitrary depth and needs a table of its
own rather than the pair-at-a-time composition dead keys reuse. Nothing else in
(1)-(3) is outstanding, and the three national layouts now compose where a real
board would.
