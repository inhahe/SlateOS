## TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE — FIXED 2026-08-24 (part 1 2026-08-22, part 2 2026-08-24)

**Status, 2026-08-22.** Part 1 below is **done**: `appearance::Palette` exists,
carries the light ladder as well as the dark one, and `DecorationColors` and
`DesktopTheme` are rewritten to read roles out of it instead of repeating its
values. Part 2 — threading `&Palette` through the 49 modules and deleting the
549 constants — is **still open**, and is now unblocked in the sense the entry
originally meant: the type it needed to thread exists.

Three things worth carrying forward from doing part 1:

- **The "33 distinct values" figure below was wrong**, and low. The survey that
  part 1 started from counted **91 distinct constant names** across the 549
  declarations, collapsing to **18 roles** (10 rungs of the surface/text ladder
  plus 8 named hues) once composites and near-duplicates were resolved. The
  original 33 appears to have counted values rather than names and to have
  missed the modules that spell the same role differently.
- **Two names carried genuinely conflicting values,** so part 2 cannot be a
  blind substitution of role for name. `BASE` is opaque in 26 modules and
  `rgba(30, 30, 46, 240)` in 2 — the translucent ones are panels, and become
  `Palette::panel_bg()`. `SHADOW` is declared at three different alphas
  (100/120/160) plus a separate `LABEL_SHADOW` at 180; the first three are one
  role at 120 (`shadow()`), and `LABEL_SHADOW` stays separate as
  `text_shadow()` because its job is legibility over an arbitrary wallpaper
  rather than elevation over a known surface.
- **Catppuccin's own Latte `subtext0` is not legible enough to ship.** It
  measures 4.37:1 on the Latte base, under the 4.5:1 body-text floor, while
  Mocha's counterpart is 7.37:1. `LIGHT_SUBTEXT0` is therefore `#686B80`, not
  upstream's `#6C6F85` — see design-decisions §525.

Part 1 is proved by `scripts/reintro-palette.py`, which reintroduces 19
defects one at a time and confirms each is caught by the test that names it.
That harness found a real latent defect while doing so (defect Q): the first
version of `DecorationColors::from_settings` built its frame from
`Palette::for_mode` and painted the accent on afterwards, so a frame was
always assembled from a palette carrying the *default* accent. Fixed by
`from_palette(&Palette)` being the single body, with the mode path and the
settings path each passing the palette they mean.

**In short:** The desktop has a settings page where the user picks light or
dark mode, an accent colour, and how transparent panels should be. Almost
nothing obeys it. 49 of the shell's modules — every settings page, every
dialog, the taskbar, the launcher, the login screen, the on-screen displays —
each declare their own private list of colour constants and paint with those.
The user can set the desktop to Light and watch the whole of it stay dark
except the four surfaces `DesktopShell` renders itself. The setting is real,
it is saved to disk, it is read back correctly, and then it is ignored.

**Where:** `gui/desktop/src/*.rs` — 549 `const NAME: Color = …` declarations
spread over 49 modules. They collapse to **33 distinct values**: `TEXT` is
declared 31 times, `BLUE` 29, `BASE` 28, `SUBTEXT0` 28, `SURFACE0` and
`SURFACE1` 27 each. `Color::from_hex(0x89B4FA)` is written out 47 separate
times. The canonical answer lives in `gui/appearance/src/lib.rs`
(`ThemeMode::is_light`, `AccentColor::color`/`color_light`,
`TransparencyLevel::panel_alpha`, `AppearanceSettings::effective_accent`).

**Reproduce:** set `theme.mode: light` in the appearance config and start the
shell. `DesktopShell`'s own four render methods respond. Open any settings
page, the launcher, or a dialog: unchanged, still Catppuccin Mocha. Same for
a non-default accent — `effective_accent()` reaches nothing that draws.

**Why it shipped:** every one of these 49 modules was behind
`#[allow(dead_code)]` for the life of the crate (see
`TD-C-DEAD-CODE-IS-ALLOWED-WHOLESALE`). The single symptom rustc could have
shown — an unused constant — was suppressed 54 times over, so the duplication
grew module by module with nothing to object. Removing those allows is what
surfaced it: 119 warnings appeared at once in a crate that had been reporting
zero.

**This is the sweep's recurring shape, at its largest scale so far:** the tree
held one correct answer that callers could not reach, so it grew wrong copies
of it — 549 of them. The copies are not *wrong* today, because they agree with
what the dark theme happens to be. They are wrong the moment the user changes
anything.

**Proper fix, in two parts — the second is the easy one:**

1. **[DONE 2026-08-22] `gui/appearance` cannot currently answer the question.** It resolves
   *accents* for both schemes (`color` / `color_light`) but the base, surface,
   overlay and text colours exist only as the hardcoded dark values. Nothing in
   the tree knows what a window background is in light mode. So the fix starts
   by adding a resolved palette — a struct carrying the ~33 roles the shell
   actually uses, built from a `ThemeMode` + `AccentColor` +
   `TransparencyLevel` (Catppuccin Latte for light, Mocha for dark, which is
   what the existing constants already are). Until that type exists there is
   nothing to thread.
2. **[OPEN]** Then thread `&Palette` through the render functions of the 49
   modules and delete the 549 constants. Mechanical, large, and safe: a module
   that no longer declares a colour cannot disagree about one. Read the three
   findings at the top of this entry first — `BASE` and `SHADOW` each mean two
   different things depending on the module, so this is not a blind rename.

**Method for part 2, surveyed 2026-08-22.** Two things make it much smaller
than the 549 figure suggests:

- **No module's render function has a caller outside its own file.** Checked
  across `gui/` and `apps/`: nothing references `security_dialog::`,
  `login_screen::`, `print_manager::` and so on — these are state machines
  built ahead of the shell binary that will drive them, and their `render()`s
  are exercised only by their own `#[cfg(test)]` modules. So adding a
  `p: &Palette` parameter is a *within-file* change, not a signature change
  that ripples through the crate. Only `lib.rs` mentions `DesktopTheme` or
  `Palette` at all today.
- **The colour uses cluster in a handful of functions per module.**
  `security_dialog.rs` is the worst file at 29 constants, and its 44 `theme::`
  references sit in five functions — `render`, `render_shield_icon`,
  `render_detail_row`, `render_button`, `render_checkbox` — plus
  `RiskLevel::color`, which becomes `color(self, p: &Palette)`.

**The verification, which is the part worth getting right.** Do not eyeball 549
substitutions. Each converted module gets a sweep that renders its state
*twice*, once per mode, and asserts that every colour the light render emits is
drawn from the light palette: the set of `Palette` fields for that mode, their
alpha variants, black at any alpha (scrims and shadows are an absence of light,
§525 decision 3), and the two `readable_on` endpoints. A constant left behind is
by definition a *Mocha* value, so it fails that membership check in light mode
and names itself. This catches a missed replacement mechanically, which is the
only way a change this size can be trusted.

One wrinkle to encode rather than rediscover: `readable_on` returns `0x11111B`
or `0xEFF1F5`, and `0x11111B` *is* Mocha `crust`. It has to be allowed in a
light render — it is the deliberate dark extreme, not a leftover — which means
the sweep cannot catch a stray literal `CRUST`. Everything else it catches.

Mapping for the alias names, which recur across modules and are the only part
that needs judgement rather than substitution:

| Declared as | Becomes | Note |
|---|---|---|
| `SHIELD_BG`, `BUTTON_BG`, `DETAILS_BORDER` = `0x45475A`/`0x313244` | `p.surface0` / `p.surface1` | plain role aliases |
| `BUTTON_HOVER` = `0x585B70` | `p.surface2` | |
| `DETAILS_BG` = `0x181825` | `p.mantle` | |
| `RISK_LOW`/`MEDIUM`/`HIGH`/`CRITICAL` | `p.green`/`p.yellow`/`p.peach`/`p.red` | categorical, must **not** follow the accent |
| `SHADOW` = `rgba(0,0,0,160)` | `p.shadow()` | one value replaces the shell's three (100/120/160) |
| `DIMMER` = `rgba(0,0,0,120)` | `p.scrim()` | 140; deliberately unified |
| `ALLOW_TEXT`, `DENY_TEXT` = `0x1E1E2E` | `readable_on(p.green)`, `readable_on(p.red)` | **the one trap** — see item 2 below |

Do **not** do part 2 without part 1 — threading a struct whose light variant
does not exist yet just relocates the hardcoding into the struct's
constructor.

**What the sweep does not prove, learned at module 3.** The membership sweep
finds a *leftover constant*. It does not find a *wrong role*, and cannot: a
role is a member of both palettes, so converting a colour to the wrong one
passes in light mode exactly as it passes in dark. Measured rather than
assumed — harness defect EE swapped an icon label from `on_wallpaper` to
`text` and the sweep reported green.

The practical consequence for the remaining modules: **any colour that must
*not* follow the mode needs its own test.** The sweep covers "was this
converted at all"; nothing covers "was it converted to the right thing" except
an assertion someone writes. The pattern is
`an_icon_label_does_not_change_colour_with_the_mode` — render twice, filter the
commands that carry the colour in question, and assert the two renders agree.
Candidates: anything drawn on the wallpaper, on a video surface, on a
thumbnail, or on any other content the palette does not own.

**A procedural note about the workspace gate, learned the hard way on module
36.** It is safe to *edit* source while `cargo test --workspace` is running,
once its build phase has finished — every test binary is already on disk and
nothing will be recompiled. It is **not** safe to *build*. Cargo holds the
target-directory lock only during a build, and releases it while the test
binaries execute, so a `cargo fmt` or `cargo test -p <crate>` fired during that
window acquires the lock and rewrites artefacts the in-flight gate is still
walking through. Nothing failed on the occasion this was discovered — the gate
came back 67,835 passed, 0 failed — but that was luck about ordering, not a
guarantee, and on Windows the failure mode would be a sharing violation in
whichever of the two processes lost the race. Wait for `[run-timeout] child
exited` before running any cargo command against the same target directory.

**Part 2 progress. 49 of 49 modules converted — Part 2 is complete.**

- [x] `security_dialog.rs` — 29 constants, done 2026-08-22. The method above
  survived contact: the sweep lives in `gui/desktop/src/palette_check.rs` as
  `assert_drawn_from(&Palette, &[RenderCommand], derived, what)`, backed by a
  new public `Palette::roles()` so the light-mode membership set is the palette
  itself rather than a second hand-written list. Four self-tests prove the
  sweep rejects what it exists to find, and harness defects V/W/X in
  `scripts/reintro-palette.py` prove it end-to-end on this module.
  - **The state matrix is the load-bearing part, not the assertion.** A
    leftover constant is only caught if the sweep renders the state that draws
    it. This module needed four risk levels × expanded/collapsed ×
    remember on/off × 1-or-2 queued × five hover targets = 160 renders per
    mode. Defect W hides behind the details disclosure and defect X behind a
    hover, so a one-render sweep would have passed both. Budget the same care
    per module: enumerate the branches of `render` that *select* a colour, not
    the ones that move geometry.
  - `derived` was empty here — the dialog computes no colours. Modules using
    `emphasized` or `Color::lerp` will have to declare theirs.
  - Three `CRUST` sites became `readable_on(risk.color(p))`,
    `readable_on(color)` and `p.on_accent()`. These are exactly the sites the
    sweep is blind to (see the `readable_on` wrinkle above), so they were
    decided by reading rather than by test — the standing cost of that hole.
- [x] `run_dialog.rs` — 16 constants, done 2026-08-22. Harness defects Y/Z/AA.
  - **The blue constants are the judgement, and there were four of them.**
    `INPUT_BORDER_FOCUS`, the input's selection fill, the selected suggestion's
    label and `BUTTON_PRIMARY` were all `0x89B4FA`. All four mean "this is
    where you are, this is what will happen if you press Enter", which is the
    accent's job, so all four became `p.accent` — and `BUTTON_PRIMARY_TEXT`
    (Mocha `base`, drawn on that blue) became `p.on_accent()`, because a pale
    Latte accent needs dark text and a deep Mocha one needs light. The error
    message stayed `p.red`: it is categorical, and on a Red desktop an error
    that matched the OK button would be unreadable as an error.
  - **The `readable_on` hole bit here for the first time.** `INPUT_BG` was
    `0x11111B` — Mocha `crust`, which is also what `readable_on` answers for a
    light fill, so the sweep must allow it and cannot tell a converted site
    from an unconverted one. It became `p.crust` by reading the code. Expect
    this in every module with a recessed well or a dark inset; those sites are
    reviewed, not tested, and that is worth knowing before trusting a green
    sweep as complete coverage.
  - The autocomplete dropdown's selected row stayed `p.surface0` rather than
    becoming `p.highlight_fill()`. `highlight_fill` is the right *role* for a
    hovered item, but swapping it in changes what the dialog looks like, and
    this task is a conversion — a redesign hidden inside a 549-substitution
    edit is a redesign nobody reviewed.
- [x] `icons.rs` — 16 constants plus 2 written inline, done 2026-08-22.
  Harness defects BB/CC/DD/EE/FF.
  - **Two of the eighteen were not in the `theme` block at all.** The drag
    ghost's fill and its glyph tint were `Color::rgba(137, 180, 250, 30)` and
    an alpha'd copy of the icon hue, written at the call site. Grep for
    `Color::rgba`/`Color::from_hex` in every module *after* emptying its theme
    block; the 549 count is of named constants and undercounts.
  - **Six mapped straight onto `Palette`'s helpers**, alpha for alpha:
    selection fill/border → `selection_fill`/`selection_border`, rubber-band
    fill/border → `hint_fill`/`hint_border`, drop target → `drop_target`,
    label shadow → `text_shadow`. Those helpers were written *from* these
    values; this is the copy going home. The selection now follows the user's
    accent, which the hardcoded blue never could.
  - **The nine icon-type hues stay categorical** — folder yellow, recycle bin
    red, executable peach. A Red desktop that painted every shortcut and the
    recycle bin the same colour would be worse than one that ignored the
    setting.
  - **New in `appearance`: `Palette::on_wallpaper()` and `on_wallpaper_dim()`,
    pale in both modes.** An icon label lands on an arbitrary photograph under
    a black shadow, so its legibility cannot come from the palette — the same
    argument that already makes `text_shadow` black in both modes (§525
    decision 3). `p.text` would be dark on Latte, and dark text under a black
    shadow is legible against nothing. Also new: `LIGHT_EXTREME` and
    `DARK_EXTREME`, the two answers `readable_on` gives, named so that
    `on_wallpaper` can share the endpoint without borrowing `LIGHT_BASE`'s
    meaning.
  - **The sweep's blind spot showed up here** — see the paragraph above this
    list. `icons.rs` therefore carries a second test,
    `an_icon_label_does_not_change_colour_with_the_mode`.
- [x] `notif_pane.rs` — 15 constants plus 1 written inline, done 2026-08-22.
  Harness defects GG/HH/II/JJ/KK.
  - **The wrong-role lesson from `icons.rs` was applied up front rather than
    discovered again.** This module has a categorical axis of its own — four
    notification priorities — so it was converted *with* the second test
    (`a_notification_priority_does_not_follow_the_accent`) instead of waiting
    for a defect to prove the sweep could not see one. The shape differs from
    `icons.rs`'s: there the invariant was "does not follow the **mode**" and
    the test renders light and dark; here it is "does not follow the
    **accent**", so the test renders the same mode twice with two different
    accents and compares. Defect KK confirms the split — it is caught by the
    accent test and *not* by the sweep, which is the second independent
    measurement of that hole.
    - The test also asserts the *negative*: that something in the same render
      did move with the accent (the quick-settings toggle pill). Without that
      half, a bug which made the whole pane ignore the accent would satisfy
      "the priorities did not move" and the test would pass while measuring
      nothing.
  - **Six sites were `BLUE` meaning "interactive", and all six became
    `p.accent`** — the unread badge, "Clear all", "Back", a toggle that is on,
    a slider's filled portion, and the settings link. The text drawn on the
    first two became `readable_on(...)` of what it sits on rather than the
    fixed near-black `CRUST` it was, because with a user-chosen accent
    underneath there is no longer one legible answer. Same judgement as
    `run_dialog.rs`'s four blues.
  - **A pre-existing inconsistency, recorded rather than silently fixed:** the
    quick-settings toggle paints "on" in blue and the per-app toggle paints
    "on" in green, for the same meaning. Converted faithfully (`p.accent` and
    `p.green`), because unifying them is a design change and this task is a
    conversion. Worth revisiting when the shell gets a real control style.
  - **`PANE_BG` stayed `p.base` rather than becoming `p.panel_bg()`.** The
    helper is arguably the right role for a slide-out panel and would pick up
    the user's transparency setting, but it changes what the pane looks like.
    Same reasoning that kept `p.surface0` in `run_dialog.rs`; noted here as a
    candidate for the pass that comes *after* all 49 are converted.
  - **The inline constant was an animated one**, which is why the theme-block
    grep would have missed it twice over: `Color::rgba(0, 0, 0, (60.0 * vis)
    as u8)`, the scrim behind the pane faded in with the slide. It now derives
    from `p.scrim()` and scales the *role's* alpha by `vis`, so the fade
    survives. Note that the sweep cannot see this one either — it allows black
    at any alpha, by design, because that is how a scrim and a shadow are
    spelled — so no defect is offered for it. A module whose only remaining
    hardcoded colour is black is a module the sweep will call clean.
- [x] `device_settings.rs` — 15 constants plus 1 written inline, done
  2026-08-22. Harness defects LL/MM/NN.
  - **First module where the constants were not in a `mod theme`** but bare
    `const`s at file scope. Nothing about the method changes, but the survey
    step does: grep for `Color::from_hex`/`Color::rgba` across the *whole*
    file rather than looking for a block to empty. This module's sixteenth
    colour — `Color::rgba(243, 139, 168, 30)`, the wash behind the
    driver-problem banner — is exactly what a block-shaped survey misses, and
    defect LL is it.
  - **The sweep needed no `derived` declaration for that wash**, which is
    worth knowing before reaching for one. `palette_check` compares roles on
    **RGB only** — alpha is how a role becomes a panel, a wash or a hover — so
    a translucent `p.red` is still `p.red`, and a translucent *Mocha* red in a
    Latte render is still not a Latte role. `derived` is for colours whose
    *hue* is computed (a `lerp`, an `emphasized`), not for alpha.
  - **`DriverStatus::Updating` is the sharpest instance yet of the
    categorical/accent question**, because it is *blue* — the default accent —
    and so reads as an obvious `p.accent`. It is not: it is one of five fixed
    badge states, and following the accent would move it while `Loaded`,
    `NotFound`, `Error` and `Disabled` stayed put. Defect NN reintroduces
    exactly that mistake and is caught only by the accent test, never by the
    sweep. Expect this trap in every module with a blue *state*: check whether
    the colour has siblings before assuming a blue is the accent.
  - Two things here genuinely are the accent: the active tab's label, and a
    settings toggle in its enabled position. The four overview figures
    (Connected / Total / Problems / Removable) are not — they are one row of
    categorical hues written as a table.
  - The content well behind the tabs was `CRUST`, the `readable_on` hole
    again; `p.crust` by reading. Text on the driver badges and on the eject
    button became `readable_on(...)` of what they sit on.
- [x] `window_rules.rs` — 14 constants, done 2026-08-22. Harness defects
  OO/PP/QQ/RR/SS/TT.
  - **A third naming shape: `const MOCHA_BASE`, `MOCHA_SURFACE0`, … at file
    scope.** After `mod theme { … }` (`notif_pane.rs`) and bare `const NAME`
    (`device_settings.rs`), the survey now has to expect at least three. The
    conclusion is the same one the inline literals already forced: the only
    reliable survey is a whole-file grep for `Color::from_hex`/`Color::rgba`,
    not a search for a block to empty.
  - **The row-of-cells shape makes the categorical/accent question decidable
    rather than a matter of taste.** Each cell of a rule row is coloured by
    what it means — priority red/yellow/`subtext1`, action-count
    green/`overlay0`, status green/red, match-expression blue — and they are
    *siblings in one row*. The blue is the `device_settings.rs` trap again
    (blue is the default accent, so it reads like an accent site), and the
    siblings are what settle it: moving one member of a categorical row while
    the rest stay put is the bug, not the feature. Defect SS reintroduces
    exactly that and is caught only by the accent test.
  - **The module's two genuine accent sites are in different views from its
    green confirm button**, which surfaced a pre-existing inconsistency worth
    recording rather than fixing: the list view's primary action ("+ Add Rule")
    was blue and became `p.accent`; the editor's primary action ("Save") is
    green and stayed `p.green`. Green is the one hue no accent resolves to, so
    reading Save as an accent site would have changed its colour under the
    *default* accent — which no faithful conversion does. The two never appear
    on screen together, which is presumably how the inconsistency survived.
    Same disposition as `notif_pane.rs`'s blue/green toggles: converted
    faithfully, noted for the pass that comes after all 49.
  - **Both "does not follow the accent" tests here carry their negative half,
    and defect TT is the proof it is needed.** TT freezes the selected
    match-type chip on blue — nothing moves with the accent any more, so the
    equality half of the test passes; only the `assert_ne!` on the chip catches
    it. A "does not follow X" test without a negative half certifies a module
    that ignores X entirely.
  - The status badge's wash (`Color::rgba(status_color.r, …, 51)`) needed no
    `derived` declaration, for the reason recorded under `device_settings.rs`:
    the sweep compares roles on RGB alone. Defect RR replaces it with Mocha
    green's channels at the same alpha and the light sweep still names it.
- [x] `user_accounts.rs` — 14 constants, done 2026-08-22. Harness defects
  UU/VV/WW/XX/YY/ZZ.
  - **A fourth shape, and the first that cannot be converted by substitution: a
    `const [Color; N]` table.** `AVATAR_COLORS` was seven Mocha constants in an
    array, indexed by a `u8` read straight off disk. A palette resolved at
    runtime cannot live in a `const`, so the table became
    `fn avatar_colors(p: &Palette) -> [Color; 7]`. Add tables to the survey
    checklist: a whole-file grep for `Color::from_hex`/`Color::rgba` finds the
    *definition* of such a table, but the thing that has to change is every
    signature that reads it — here three public methods
    (`AccountType::badge_color`, `Avatar::palette_color`,
    `Avatar::background_color`) all gained a `&Palette`.
  - **The avatar table settles the categorical question on a new ground:
    distinctness.** The seven colours are identity — how you tell accounts
    apart at a glance — so they are categorical for the same reason a risk
    level is. But they carry an extra constraint no other categorical row has:
    they must stay mutually *distinct*, which an accent-following member could
    not guarantee, since it would collide with whichever of the seven the
    accent happened to equal. `the_avatar_colours_stay_distinct_in_both_modes`
    asserts it directly; defect XX proves the sweep sees the seventh slot at
    all, which it only does because the fixture makes one account per slot.
  - The blue-state trap for the third time: `AccountType::Standard` is blue,
    blue is the default accent, and its two siblings (`Administrator` red,
    `Guest` grey) are what settle it. Defect YY is that substitution and is
    caught only by `a_users_identity_colours_do_not_follow_the_accent`; defect
    ZZ freezes the active tab label on blue and is caught only by that test's
    `assert_ne!` half — the second independent confirmation of the TT lesson.
  - The "on" toggle stayed `p.green` rather than becoming `p.accent`, same
    disposition and same reason as `notif_pane.rs` and `window_rules.rs`: the
    shell disagrees with itself about which an on-switch uses, and a conversion
    is not the place to settle it. Three modules now carry that note; when the
    49 are done it is one small pass, not 49 judgement calls.
  - Two `MOCHA_MANTLE` sites became `readable_on(...)` — the initials on the
    avatar circle and the label on the account-type pill. Both sit *on* a
    coloured fill whose hue is chosen by data, so neither is a palette role.
- [x] `bluetooth.rs` — 13 constants, done 2026-08-22. Harness defects
  AAA/BBB/CCC/DDD/EEE/FFF/GGG.
  - **The distinctness argument from `user_accounts.rs` generalises, and here
    it is the *whole* reason a site refuses the accent.** The scan button has
    two states — blue "Scan for devices", peach "Scanning..." — and peach is
    one of the fourteen accents a user can pick. An accent-following idle state
    would therefore render *identically* to the scanning state on a peach
    desktop, deleting the only signal that a scan is running. That is not a
    taste judgement about what the accent means; it is a collision, and it is
    checkable: `the_scan_button_says_something_different_while_it_is_scanning`
    renders under blue, peach and mauve accents in both modes and asserts the
    two states differ. Defect GGG is exactly the substitution and is caught by
    it. **Generalisation worth carrying to the remaining 41 modules: whenever a
    two-state or n-state control would become "accent vs. fixed hue H", check
    whether H is one of the fourteen accents; if it is, the accent is wrong
    there regardless of how much the site otherwise reads like an accent site.**
  - The two genuine accent sites are the power switch and the filled part of
    each signal meter — "the interactive thing" and "how much of it there is",
    which is `notif_pane.rs`'s stated doctrine. Both of their off/empty halves
    are `p.surface1`, a neutral no accent resolves to, so neither can collide
    the way the scan button would have.
  - The blue-state trap for the fourth time: `ConnectionState::Paired` and
    `PairedNotConnected` are blue, and their siblings (Connected green,
    Connecting yellow, Disconnected `overlay0`) settle it as categorical.
    Defect EEE moves `Connected` onto the accent and is caught only by
    `a_devices_status_colours_do_not_follow_the_accent`; defect FFF freezes the
    filled signal bars on blue and is caught only by that test's `assert_ne!`
    half — the third independent confirmation of the TT lesson.
  - **Defect FFF caught a hole in the negative half itself, and this refines
    the TT rule rather than repeating it. `assert_ne!` on a *combined* vector of
    accent sites is too weak: it proves only that *at least one* site moved, so
    a still-moving site masks a frozen one.** The first draft here collected the
    power switch and the signal bars into one `accent_site_colors` vector; FFF
    froze the bars while the switch kept following the accent, and the harness
    reported `*** NO TEST FAILED ***`. The fix is one `assert_ne!` per site, and
    the rule for the remaining 41 modules is: **a module with n accent sites
    needs n negative assertions, not one over their union.** This is exactly the
    failure the reintroduction discipline exists to find — the test was written,
    reviewed, green, and measuring less than it claimed; nothing but a
    deliberately reintroduced bug would have said so.
  - **The matrix had to cover a branch that returns early.** An unpowered
    adapter draws four commands and `return`s, so every device colour in the
    module is unreachable in that half of the state space; the sweep therefore
    runs powered × discovering × `show_nearby` × four scroll offsets × three
    panel heights, with one device per connection state, batteries straddling
    both ladder thresholds, and signal strengths walking 0–4 bars. The "n more"
    line (defect BBB) is only drawn by the short panels, and the empty signal
    bar only by the weak-signal devices; a fixture with one healthy device
    would have missed both.
  - One `MOCHA_BASE` became `readable_on(p.lavender)` — the device-type glyph
    on the lavender icon circle. Same shape as `user_accounts.rs`'s two: text
    on a coloured fill, not a role.
- [x] `update_settings.rs` — 13 constants, done 2026-08-22. Harness defects
  HHH/III/JJJ/KKK/LLL/MMM/NNN/OOO.
  - **Defect NNN is the FFF lesson paying for itself one module later.** This
    panel has *two* accent sites — the active tab's label and the chosen
    schedule's label — and NNN freezes only the second. The tab label keeps
    following the accent, so an `assert_ne!` over the union of both sites would
    have passed and NNN would have gone unnoticed exactly as FFF did. Written
    per-site from the start, it fails. The rule is now confirmed rather than
    merely inferred: **n accent sites, n negative assertions.**
  - **The widest categorical row so far — five hues in one `match`.**
    `UpdateStatus::color` is green / blue / yellow / peach / red for up-to-date,
    checking-or-downloading, available, pending-restart and error. Five *kinds
    of fact about the machine*, and four of the five hues are among the fourteen
    selectable accents, so the distinctness argument applies to the whole row at
    once rather than to one member: `the_update_statuses_stay_distinct_in_both_modes`
    walks four accents × both modes and asserts no two statuses collide. Defect
    OOO gives "updates available" the same green as "up to date" and is caught
    only by that test — a collision the membership sweep cannot see, because
    green is a perfectly good palette role.
  - **The empty branch of each tab is its own colour and needs its own fixture
    flag.** All three tabs bail early with an `overlay0` "No updates available."
    / "No update history." caption instead of drawing a list, so the sweep runs
    `populated` both ways; a fixture that always had data would never render
    those three captions at all. Same shape as `bluetooth.rs`'s unpowered
    early `return`, and worth expecting in every settings panel: the
    interesting colours are often on the path where there is nothing to show.
  - `UpdateSchedule` is a radio group, not a categorical row: exactly one member
    is chosen, and the chosen one is "you are here", so its label is `p.accent`
    while the rest stay `p.text`. The tab bar is the same shape. That is the
    distinction to carry forward — *which of these am I on* is the accent's,
    *which kind of thing is this* is not, and both can be a blue `if active`
    one-liner in the source.
- [x] `storage_settings.rs` — 13 constants, done 2026-08-22. Harness defects
  PPP/QQQ/RRR/SSS/TTT/UUU/VVV/WWW/XXX, all nine caught by exactly the tests
  named.
  - **A ten-member categorical row, and the first one where the distinctness
    argument is not an analogy but the feature itself.** `StorageCategory::color`
    paints a *stacked bar* and the legend beneath it: System blue, Apps
    lavender, Documents green, Media peach, Downloads yellow, recycle bin red,
    then four greys. Two members sharing a colour do not merely look similar —
    their slices abut in the bar and **merge into one slice**, so the chart
    silently reports a lie. Six of the ten are among the fourteen selectable
    accents, so a member that followed the accent would collide for at least six
    of the fourteen. `the_ten_storage_categories_stay_distinct_in_both_modes`
    walks four accents × both modes; defect XXX (Downloads given Media's peach)
    is caught by it and by nothing else, because peach is a perfectly good
    palette role and the membership sweep cannot see it.
  - **Defect WWW is the FFF/NNN shape a third time, and it is now a pattern
    rather than a coincidence.** The two accent sites here are the active tab's
    label and the six `Change` buttons; WWW freezes only the buttons, and the
    tab label goes on moving. Every module converted so far that has had more
    than one accent site has had a defect that only a per-site assertion can
    see. Treat **n accent sites ⇒ n negative assertions** as mandatory, not as
    a refinement.
  - **A button label is the third thing the accent owns.** The rule so far was
    *which of these am I on* → accent, *which kind of thing is this* →
    categorical. `Change` is neither: it is a press-me affordance, with no
    sibling to be distinguished from and no fact to report. It takes the accent,
    for the same reason a link does — the accent is where the eye is told to go.
    So the rule generalises to: **the accent marks position and invitation; it
    never marks category or measurement.**
  - **`is_low_space` is a measurement and stays red.** The warning banner and
    the over-90% figure are the first *threshold* colour in this conversion, as
    opposed to an enum arm. It is not a selection and not an invitation, so it
    is not the accent's — a nearly-full disk is red the way a stop sign is red,
    under every accent a user can pick. Held by the positive half of
    `the_storage_panels_own_colours_do_not_follow_the_accent`.
  - **Two *absences* needed their own fixture shapes.** The sweep runs three
    fixtures, not one: no drives at all (the overview loop runs zero times and
    the breakdown bails on `drives().get(selected) == None`), one drive carrying
    all ten categories with each slice big enough to clear the `seg_w > 0.5`
    guard, and one drive holding only System and Apps so nothing is reclaimable
    and the green estimate line is skipped. The second and third are mutually
    exclusive — a fixture with a recycle bin in it can never reach the
    zero-reclaimable branch — which is worth knowing in advance: *when a branch
    is an absence, adding data to the fixture cannot reach it, only a second
    fixture can.*
- [x] `power_settings.rs` — 13 constants, done 2026-08-22. Harness defects
  YYY/ZZZ/AAAA/BBBB/CCCC/DDDD/EEEE/FFFF/GGGG, all nine caught by exactly the
  tests named. (The labels ran out of three-letter combinations and widen to
  four; `main()` compares the whole prefix, so nothing collides.)
  - **The first module whose colours are almost entirely *measurements*, and
    the first with a ladder.** Two sites follow the accent — the active tab's
    label and the selected power plan's label, both "which of these am I on".
    Everything else reads the battery: `BatteryHealth::color` is
    green/yellow/peach/red, and the charge bar is a second ladder in a `match`
    on the percentage (`0..=10` red, `11..=20` peach, `21..=50` yellow, else
    green). Neither is the accent's, for a reason stronger than taste: **a
    ladder whose rungs can collide is not a ladder.** Green, yellow, peach and
    red are four of the fourteen selectable accents, so a rung that followed
    the accent would collapse onto a fixed one for four of them, and the panel
    would stop answering the question it was drawn to answer. This is the
    stacked-bar distinctness argument from `storage_settings.rs` applied to an
    *ordered* row rather than an unordered one — and ordering makes it worse,
    not better, because a collapsed rung does not merely look ambiguous, it
    reads as a different measurement.
  - **Defect EEEE is the FFF/NNN/WWW shape a fourth time, and the cleanest
    instance so far.** It freezes only the plan list; the tab label above it
    goes on moving with the accent, so an `assert_ne!` over the union of this
    panel's two accent sites passes with the bug in. Four modules in a row now.
    **n accent sites ⇒ n negative assertions** is settled.
  - **The charge ladder has no function to test, so the test reads it off the
    render.** Unlike `BatteryHealth::color`, it is an inline `match` inside
    `render_battery_summary` with no name of its own, so
    `charge_bar_colors(cmds, track_width)` recovers it by matching the 6pt-tall
    `FillRect` that is *narrower than its track*. That filter is why the ladder
    tests stop at 80%: at 100% the fill is exactly as wide as the track and
    becomes indistinguishable from it. Worth generalising — **a colour chosen
    by a `match` with no named function still needs a per-site extractor**, and
    the extractor's discriminator (here, width) constrains which states the test
    can walk.
  - **`ChargeState::NotPresent` is walked as its own sweep case, not as one
    more charge level.** It is the panel's absence branch twice over: it skips
    the whole battery summary bar, and it replaces the battery tab's entire
    body with a single `overlay0` caption. Defect ZZZ (that caption keeping its
    own grey) is reachable through no other state — the same lesson
    `storage_settings.rs` learned about empty fixtures, arriving here as an
    enum arm rather than as an empty `Vec`.
  - **The conversion turned up a bug that has nothing to do with colour, and
    the fix generalises past this module.** `render_battery_summary` pushed the
    charge fill, then the bar's *opaque, full-width* track over it, then the
    same fill again with the comment `// Redraw fill on top (stacking order)`.
    The track covers the fill's rectangle exactly, so the first of those three
    draws could not be seen by anyone: a third of the charge bar's draw calls
    were pure blend cost. **A hidden draw is invisible in a screenshot too**,
    which is precisely why it survived — there is no way to look at it. It now
    draws the track only when the fill does not already cover it and the fill
    only when it has width, guarded by
    `the_panel_draws_nothing_that_is_immediately_erased`: no `FillRect` may be
    contained outright by a later opaque one no more rounded at the corners,
    and none may be empty. The containment rule is conservative on purpose —
    partial overlap is how every panel on the desktop draws a border, so only
    total coverage is reported. Writing the test found a **second** instance
    the eye had missed: at 100% charge the *track* was the dead command rather
    than the fill. Harness defects HHHH (the original three-push order) and
    IIII (the zero-width fill an empty battery emits, which the coverage half
    of the rule is blind to and the emptiness half catches).
    - Worth lifting into `palette_check.rs` — or a sibling `draw_check.rs` —
      once a second module wants it, since every converted module already
      renders to a `Vec<RenderCommand>` the rule can be run over. Left local
      for now: one instance is not yet a shared helper, and the rule's
      conservatism may need adjusting against a panel that layers differently.
- [x] `network_indicator.rs` — 13 constants, done 2026-08-22. Harness defects
  JJJJ/KKKK/LLLL/MMMM/NNNN/OOOO/PPPP/QQQQ/RRRR/SSSS.
  - **The harness earned its keep here: defect PPPP caught nothing on the
    first run, and the test was wrong, not the defect.** PPPP repaints the
    *airplane-mode-on* peach with the accent. The accent test's fixture came
    from `NetworkState::wifi(…)`, which leaves airplane mode off — so the
    render never emitted the peach at all, and the assertion compared the
    off-state grey with itself and passed. Vacuously green, exactly the failure
    mode this whole harness exists to detect. **Generalised rule: when a colour
    is chosen by a boolean, the test needs the boolean, not just the render.**
    The fix walks airplane × radio (four fixtures) rather than one, and defect
    SSSS was added afterwards to prove the *radio-on* green the same way — each
    on-colour needs its own state or it is silently compared with its own
    opposite.
  - **The blue-state trap, in its purest form yet.** `SignalStrength::color`
    runs red → yellow → green → **blue**, and that blue rung reads exactly like
    an obvious `p.accent`: it is blue, and blue is the default accent, so the
    mistake looks correct on a fresh install. It is not the accent — it is the
    top rung of a ladder, and on a Green desktop `Excellent => p.accent` would
    collapse onto the `Good` green. Defect NNNN.
  - **The strongest side-by-side distinctness case so far.** Unlike the battery
    ladder (one reading on screen) or the storage bar (one chart), the flyout
    lists *every* visible network with its own swatch, so two colliding rungs
    put two differently-strong networks on screen looking identical, in one
    glance, with nothing else to disambiguate them.
  - **Only one site follows the accent** — the SSID of the network you are on,
    which is the same "which of these am I on" role the selected power plan
    takes. The five connection-type hues stay categorical; the note in
    `the_five_kinds_of_link_stay_distinct_in_both_modes` records that this is a
    *weaker* argument than the ladder's, since only one tray icon is on screen
    at a time — it rests on a code the user has learnt rather than on a
    comparison. `Wifi` is excluded from that row because it has no colour of
    its own, delegating to the signal ladder.
- [x] `clipboard_viewer.rs` — 13 constants, done 2026-08-22. Harness defects
  TTTT/UUUU/VVVV/WWWW/XXXX/YYYY/ZZZZ/AAAAA/BBBBB/CCCCC.
  - **The invisible-draw rule was lifted out of `power_settings.rs` into
    `gui/desktop/src/draw_check.rs`**, as the note on that module said it
    should be once a second module wanted it. The public entry point is
    `assert_nothing_is_drawn_and_never_seen(&[RenderCommand], what)`; it keeps
    the two halves of the rule (a fill covered outright by a *later* opaque
    fill that is no more rounded at the corners; a fill of zero width or
    height) and it now carries seven self-tests of its own, which the inline
    copy never had — a translucent cover is not a cover, a cover drawn *first*
    is a background, a rounder cover leaves the corners peeking out, and a
    partial overlap is how every border on this desktop is drawn. `#[cfg(test)]
    pub mod` in `lib.rs`, beside `palette_check`, for the same reason: a
    release build has nothing to check.
  - **A site whose *foreground* is derived from its own background.** The
    active format-filter tab is the one accent site, and its label is
    `p.on_accent()` — `readable_on(accent)` — not a fixed near-black. That
    pairing needs its own test and it cannot be an `assert_ne!` between two
    accents: every accent on offer is pale, so `readable_on` answers the same
    near-black for all fourteen and a correct implementation would fail such an
    assertion. What separates `p.on_accent()` from a hard-coded `p.base` is the
    *mode* — Latte's `base` is near-white, illegible on a pale tab. So
    `the_active_filter_tabs_label_is_legible_on_it` asserts the label *equals*
    `readable_on` of the fill, in both modes, over five accents. Defect YYYY
    (fix the label to `p.base`) is caught by that test and by nothing else.
  - **The frozen half may be one assertion over a union; the negative half may
    not.** `colors_apart_from_the_tabs` takes every colour outside the filter
    strip as one vector and `assert_eq!`s it across two accents — an `assert_eq!`
    over a union fails if *any* member moves, so it loses nothing, and it
    covers sites nobody thought to name. The `assert_ne!` half stays per-site
    (here, per-tab), for the reason FFF/NNN/WWW/EEEE established: a union
    `assert_ne!` passes as soon as one member moves, so a frozen site hides
    behind a moving one. Defects ZZZZ and AAAAA (the sensitive marker and
    "Clear All" repainted with the accent) are both caught by the union
    `assert_eq!`.
  - **Two entry formats have no public constructor**, so the fixture reaches
    into the private `history.entries` to set `RichText` and `Custom`. Without
    them two of the five badge arms are never rendered and the badge ladder is
    only three rungs wide in practice. Absences and unreachable-by-the-API
    states have to be *constructed*, not waited for.
  - **The badge row is the side-by-side distinctness case again**, one step
    below the network scan list: every visible entry draws its badge at the
    same moment, so two formats sharing a colour is a one-glance confusion.
    `PlainText => p.blue` is the blue-state trap for the third time — blue is
    the default accent, so `p.accent` there looks right until the user picks
    Green and Text becomes Image (defect WWWW, caught by both the accent test
    and the distinctness test).
- [x] `notification_settings.rs` — 14 constants, done 2026-08-22. Harness
  defects DDDDD–NNNNN (eleven).
  - **A colour can be right by coincidence, and that is not the same as being
    right.** The ON/OFF badge drew its label in `CRUST`. That happens to work in
    both stock modes — Mocha's `crust` is near-black and Mocha's `green` is
    pale; Latte's `crust` is near-white and Latte's `green` is deep — so the
    two track each other by accident, and a reader of `p.crust` cannot tell
    whether anyone checked. It became `appearance::readable_on(badge_color)`,
    which is the question the badge is actually asking. Defect JJJJJ (put
    `p.crust` back) fails only in the light render, which is exactly the shape
    of a coincidence that holds in one mode and not the other. Expect more of
    these: any near-black or near-white drawn on a role that flips lightness
    between modes is one.
  - **A wasted draw found by conversion, not by the eye.** The volume bar
    pushed an opaque track and then the fill over it. At 100% the fill is the
    same rectangle with the same corner radii, so the track is a rectangle
    nobody can ever see — and the *rendered image is identical either way*, so
    only the command list says so. The track is now pushed only when
    `fill_w < bar_w`. This is the second module to find this class (the first
    was `power_settings`'s charge bar) and the reason `draw_check.rs` exists;
    the erase sweep here walks volume 0/45/100 because only the last one
    coincides. Defect MMMMM (`<` → `<=`) proves the guard is load-bearing.
  - **A measurement is not an invitation, and a scale is not a selection.** The
    four-rung priority stripe (Low/Normal/High/Urgent) and the volume bar's
    fill both stay frozen. An accented Urgent stripe would read as *selected*
    beside its neighbours in the same list, and would collide outright with
    whichever rung already carries that hue — defect KKKKK is caught both by
    the frozen-union equality and by the distinctness ladder, because with a
    Yellow accent Urgent becomes High.
  - **Three tabs, three `assert_ne!`s.** Same shape as the clipboard popup: the
    active tab is the one accent site, its fill is chosen by a boolean, so the
    test walks all three tabs as the active one rather than trusting whichever
    the fixture happened to open on.
- [x] `backup_settings.rs` — 14 constants, done 2026-08-22. Harness defects
  OOOOO–IIIIII (twenty-one), the largest set so far because this module has
  the most accent sites of any converted yet: **nine**.
  - **Nine accent sites, and the per-site rule finally earns its keep on a
    pair that is genuinely indistinguishable.** The schedule tab draws six
    switches — one "enable automatic backups" master and five retention
    switches — and they are *the same 40x20 pill at the same x*, so nothing
    about a rendered command says which of the two source sites emitted it.
    They are two separate `if` expressions, though, and freezing either one
    alone still leaves the six-pill vector different between two accents. The
    first draft of the test asserted over all six and would have passed with
    the retention loop frozen; defects VVVVV and WWWWW are that exact pair,
    each freezing one site while the other keeps moving. The split is by draw
    order — the master is first, its row being the top of the tab — which is
    the only handle available and is worth stating out loud in the test, since
    it is not obvious and not enforced by anything.
  - **An `assert_ne!` on a foreground derived from its own background is a bug
    in the test, not a check.** The three primary buttons ("Backup now",
    "+ Add source", "+ Add rule") label themselves `p.on_accent()`, i.e.
    `readable_on(accent)`. Every accent on offer is pale enough that all
    fourteen resolve to the *same* near-black, so correct code draws the same
    label under any two accents and the assertion fails on a green tree —
    which is what it did on the first run. What separates `p.on_accent()` from
    a frozen `p.crust` is the **mode**, not the accent, so the labels moved to
    a separate test asserting equality with `readable_on` across both modes
    (defects FFFFFF/GGGGGG). This is the same hole recorded under `run_dialog`
    seen from the other side: there it made the sweep blind, here it makes a
    negative assertion unsatisfiable. Rule: **never `assert_ne!` across accents
    on anything that is `readable_on(accent)`.**
  - **The blue-state trap, fourth appearance.** `BackupStatus::InProgress =>
    BLUE` sits in a five-way match beside green/yellow/red/grey. It reads like
    an obvious `p.accent` — a running backup is "active", and blue *is* the
    default accent, so the mistake is invisible on a fresh install. It is a
    category: on a Blue desktop the accent version looks identical, and on a
    Green one a running backup becomes indistinguishable from a succeeded one.
    Defect SSSSS is caught twice over, by the distinctness ladder and by the
    frozen-union equality.
  - **An alpha wash whose RGB is a role.** A disabled exclusion rule's row was
    `Color::rgba(49, 50, 68, 128)` — Mocha `surface0` at half alpha, written
    as a literal because no constant existed for "surface0 but faded". It
    became `Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)`. The
    membership sweep compares on RGB only and so needs no `derived` entry for
    it, but grep for bare `Color::rgba` in every module: a wash is a copy of
    the palette that does not look like one.
  - **An extractor keyed on bare geometry is a guess, and only a defect can
    check the guess.** `radio_dots` matched every 8x8 fill, which is the
    frequency radio's dot — *and* the history tab's status badge. Nothing in
    the rendered command says which site emitted an 8x8 square. The damage was
    not a false positive but a false *negative*: the same pattern is subtracted
    in `colors_apart_from_the_controls`, so the status badges were quietly
    removed from the frozen-union check on every tab, and a run's outcome could
    have started following the accent with no test objecting. Defect SSSSS
    caught the module's own `.color()` table but was recorded as `[MISSING]`
    against the union check — which is the only reason the hole was found. The
    fix qualifies both patterns by x (dot at 40, badge at 32, both tabs inset
    by the same `cx = x + 24`). **This is the second geometric collision in
    this one module** (the six switch pills were the first), so treat it as
    routine, not bad luck: after writing an extractor, grep the module for the
    shape it matches and confirm the count, and write at least one defect that
    only the *other* widget can trigger.
  - **Membership cannot check the two surfaces the panel is made of.**
    `assert_drawn_from` has to allow `0x11111B` and `0xEFF1F5` at any alpha,
    since those are the only two answers `readable_on` gives and any correctly
    converted foreground is one of them. But `0x11111B` is also Mocha's
    `crust` — so reverting the content well to the literal produces a render
    the sweep is *obliged* to accept. Defect PPPPP was the first `*** NO TEST
    FAILED ***` in the whole part-2 conversion, and it is structural rather
    than an oversight. The answer is to stop asking about membership for these
    two: `the_panels_own_surfaces_come_from_the_palette` names the role and
    asserts equality with `p.base`/`p.crust` in both modes, which is strictly
    stronger and also fails in *dark* mode, where a membership check never
    could. Every module with a background and a recessed well needs this test;
    the `run_dialog` note above described the same hole but only worked around
    it by reading the code, which is what let it survive twelve more modules.
  - **`pathlib.write_text` silently converted the file to CRLF**, which broke
    every `\n`-containing harness pattern with `PATTERN NOT FOUND` while the
    Rust still compiled and every test still passed — a failure that looks
    exactly like a stale pattern. Any script that rewrites a source file in
    place must use `write_bytes`, or pass `newline=""`. Both this module and
    `scripts/reintro-palette.py` had to be normalised back to LF.
  - **The active tab's label is the accent itself, not `on_accent`.** Unlike
    the last two modules the tab strip has no fill behind the active tab —
    the label alone carries the state, so it is `p.accent` directly and *does*
    move between accents (defect TTTTT). Two adjacent modules, two different
    correct answers; the shape of the widget decides, not the word "tab".
- [x] `network_settings.rs` — 14 constants, done 2026-08-22. Harness defects
  JJJJJJ–KKKKKKK (twenty-eight), and the first module whose conversion turned
  up **two user-visible bugs that had nothing to do with colour**.
  - **A picker that painted both of its options at the same place.** The DNS
    mode row looped over `[Automatic, Manual]` and never advanced `x`, so
    "Automatic" was drawn and then covered outright by "Manual". The option
    existed in the state, the type system and the click handler, and could not
    be seen or chosen. `draw_check`'s "drawn and never seen" rule is what
    surfaced it — but only by the accident that the overlap was *exact*;
    `draw_check` exempts partial overlap deliberately, so a one-pixel offset
    would have hidden it. That is why the module also has a direct layout test
    (`no_picker_segment_hides_another_or_leaves_the_row`) rather than leaning
    on the erase sweep. **A generic invariant catching a specific bug is luck,
    not coverage; write the specific test anyway.**
  - **A picker that offered four of six variants.** The proxy type row listed
    `None/Http/Socks5/Auto`, omitting `Https` and `Socks4` — both of which
    `ProxyType` defines and `ProxyConfig::validate` accepts. A user already on
    HTTPS saw a picker with *nothing* highlighted, and any touch could only
    move them off a setting they could never get back to. It also sized its
    segments as `(width - 12) / n` and then spaced them `btn_w + 4` apart,
    which is self-consistent at exactly one `n`; at four it already ran ~4px
    past the row's right edge and at six it would have run ~20px past. Both
    pickers now go through one `segment_bounds(x, width, i, n)` helper that
    takes the `n - 1` gaps out of the total *before* dividing. Two open-coded
    copies of one layout had drifted in two different directions, which is the
    same defect class as the 549 palette constants, one layer down.
  - **Eight accent sites, split into eight assertions, and four of them are
    loops.** Active tab label; the status tab's four quick toggles; both
    segmented pickers; the DoH toggle; the proxy auth toggle; the three
    firewall option toggles; "+ Add rule". The per-site rule from
    `backup_settings` holds: n sites ⇒ n negative assertions, because one
    `assert_ne!` over their union passes while any single site still moves.
    Note that three of the eight are `if *enabled { p.accent }` on a `40x20`
    pill and are geometrically identical to each other; they are separated by
    which tab renders them, which is the only handle there is.
  - **Five categorical scales, not four.** Connection state, Wi-Fi security
    strength, signal quality, firewall action — and the firewall
    enabled/disabled dot, which is easy to miss because it is a bare
    green/red rather than a `.color()` method. The blue-state trap did *not*
    appear here: none of the four scales uses blue, which is worth recording
    because four consecutive modules had it and the pattern was starting to
    look universal.
  - **`p.crust` on a categorical badge is right only by coincidence, again —
    and the coincidence is now understood.** The firewall action badge drew
    its label in `CRUST`. Mocha's green/red/yellow are pale (dark text reads)
    and Latte's are deep (light text reads), and `crust` flips lightness with
    the mode alongside them, so a fixed `p.crust` stays legible on all six
    values by accident of which two palettes we ship. It became
    `readable_on(rule.action.color(p))` — not `p.on_accent()`, because the
    fill under it is categorical, not the accent. Defect BBBBBBB puts
    `p.crust` back and fails in light mode only. This is the third module with
    this shape (`notification_settings`, `backup_settings`); the rule is now:
    **any near-black or near-white drawn on a role that flips lightness
    between modes must be `readable_on(that role)`, never a literal.**
  - **The extractor collision arrived on schedule, and was a false negative.**
    `segment_fills` matched every `height: 32.0` fill narrower than the row —
    which is a picker segment *and* the active tab's own pill, since a padded
    text width passes any width bound a segment passes. As in
    `backup_settings`, the damage would not have been a spurious failure but a
    silent one: the same pattern is subtracted in
    `colors_apart_from_the_controls`, so the tab pill's colour would have
    vanished from the frozen-union check. Fixed with a `y > 100.0` bound, the
    tab strip being the only thing drawn above the content well. **Third
    consecutive module with a geometric collision.** Treat the grep-and-count
    step as mandatory, not as diligence.
  - The half-alpha disabled-rule wash (`Color::rgba(49, 50, 68, 128)`) is the
    same "alpha wash whose RGB is a role" as `backup_settings`, and the
    content well is the same structural `readable_on` membership hole; both
    have their own tests (`a_disabled_rule_is_the_enabled_row_made_translucent`
    and `the_panels_own_surfaces_come_from_the_palette`) for the reasons
    recorded above. Defect KKKKKK is the second confirmed case of a defect the
    membership sweep is *obliged* to accept.
  - **Deliberate non-change:** the switch knob is `p.text` on a `p.accent`
    pill (~1.35:1 in Mocha). The correct fix is `readable_on(toggle_bg)`, but
    the pattern is desktop-wide across all 49 modules and fixing it in one
    would make the desktop inconsistent with itself. Tracked separately as
    `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`, to be done as one
    sweep once the threading lands.
- [x] `startup_settings.rs` — 13 constants, done 2026-08-22. Harness defects
  LLLLLLL–OOOOOOOO (thirty).
  - **A hardcoded hex has no role until someone assigns one.** The per-entry
    enable switch and the three boot-tab toggles were a hardcoded `GREEN`,
    which made this the only settings panel in the shell whose switches did
    not follow the accent. Mapping them to `p.green` would have been *just as
    much of a choice* as mapping them to `p.accent`; there is no "leave it
    alone" option, because the literal names a colour and not a role. This is
    the general rule for the remaining 32 modules: a conversion that meets a
    literal must decide what the literal *meant*, and "keep the same pixels"
    is one answer among several rather than the neutral one. They became
    `p.accent`, which is what the other sixteen converted modules already say,
    and which within this module also demotes a real collision: the apps list
    draws the enable switch and the impact badge on the same row and the impact
    scale's lowest rung is green, so a green switch sat beside a green "None"
    badge on every stock install. Following the accent moves that from
    "always" to "only for a user who picks Green".
  - **Five variants over four colours, on purpose.** `StartupImpact::color`
    paints `None` and `Low` the same green: the badge is a three-band traffic
    light (fine / slow / bad, plus grey for a reading that does not exist yet)
    laid over a finer-grained label. Distinctness is therefore a claim about
    the **bands, not the variants** — walking the five pairwise would fail on
    correct code. There is a separate test
    (`the_impact_light_has_fewer_bands_than_the_impact_label`) whose only job
    is to stop a future reader "fixing" the shared arm, and defect KKKKKKKK
    splits it to prove that test is load-bearing.
  - **A scale hidden inside a render call cannot be tested.** The last-boot
    reading's green/yellow/red ladder was three arms of an `if` buried in a
    `RenderCommand::Text`, unreachable from a test without rendering the whole
    tab and hunting for a formatted string. It is now `boot_time_color(ms, p)`
    with a boundary test at 9 999 / 10 000 / 29 999 / 30 000. **Extracting a
    measurement scale to a named function is part of the conversion, not a
    detour**: the conversion's whole claim is that these colours are roles
    chosen by a rule, and a rule nobody can call is a rule nobody can check.
  - **The impact badge's label was genuinely wrong, not merely fragile.** It
    was a hardcoded near-black on the badge's own fill. On the four coloured
    arms that is the usual coincidence — Mocha's green/yellow/red are pale, so
    dark text reads — but `NotMeasured` fills the badge with `overlay0`, a mid
    grey, where near-black is poor contrast in dark mode and no better in
    light. And `NotMeasured` is not an exotic state: `StartupEntry::new` starts
    every entry there, so it is what a freshly-added app shows. Now
    `readable_on(impact_color)`. Fourth module with the `p.crust`-on-a-
    categorical-fill shape, and the first where the coincidence had already
    failed rather than merely being unmaintained.
  - **Three accent *sites*, five accent *controls*.** The boot tab's three
    switches are three rendered pills but one call site (`render_toggle_row`),
    so they are one `assert_ne!` with a length check beside it; the tab pill
    and the per-entry switch are the other two. The per-site rule is about
    **source sites**, not rendered instances — a loop cannot disagree with
    itself, so splitting it would prove nothing that the count does not.
  - **The extractor collision was in the label text, not the geometry.** For
    the first time in four modules the `FillRect` dimensions were all distinct
    (grepped: one `height: 32.0`, one `36.0` wide, one `40.0` wide, and the two
    `74.0`-wide badges separated by height). The trap was elsewhere: the
    module's *title* is the string `"Startup Apps"` and so is one of its *tab
    labels*, so the `button(cmds, label)` helper the last three modules used —
    "the last fill before this text" — would have returned the backdrop and the
    title, silently. The tab strip is keyed on geometry instead (`y: 72.0`
    pills, `y: 80.0` labels). **Generalisation: an extractor keyed on a string
    is exactly as much of a guess as one keyed on geometry, and needs the same
    grep-and-count before it is trusted.**
  - **The frozen union deliberately *keeps* the `on_accent()` tab label**,
    unlike `network_settings`, which excluded it. Both accents the test uses
    are pale, so `readable_on` answers the same near-black for both and a
    correct label is frozen between them — which means a label wrongly painted
    with the accent itself is caught there as well as by the legibility test
    (defect DDDDDDDD proves both). The cost is a dependency on the two accents
    sharing a lightness band, which is written down at the exclusion list.
  - The high-impact banner is the same "alpha wash whose RGB is a role"
    (`Color::rgba(RED.r, RED.g, RED.b, 40)`) as `backup_settings` and
    `network_settings`, with its own test for the same reason: the membership
    sweep compares RGB and ignores alpha by design.
  - **Deliberate non-change:** the switch knobs stay `p.text`, per
    `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`.
- [x] `datetime_settings.rs` — 13 constants, done 2026-08-22. Harness defects
  AAAAAAAAA–YYYYYYYYYY (fifty-one).
  - **Pin an accent site by equality with the accent, never by inequality with
    the literal it used to be.** Every module since `run_dialog` had written
    `assert_ne!(pill, appearance::BLUE, "kept its hardcoded blue")` beside the
    equality check, as a second belt. Here that assertion **failed on correct
    code**, and it was right to: the test loops over a set of accents, the set
    contains blue, and a correctly-converted pill on a blue-accented desktop
    *is* `0x89B4FA`. The inequality was never a real check — it was an
    assertion that the user had not chosen blue. `assert_eq!(site, accent)` run
    over seven accents is strictly stronger (no fixed value satisfies all
    seven) and cannot false-positive, so the `assert_ne!` half was deleted
    outright rather than special-cased. **Generalisation for the remaining 31
    modules: an assertion whose truth depends on which accent the user picked
    is a bug in the test, not a safeguard.**
  - **Geometry can collide *across* tabs rather than within one.** Sixteen
    modules of grep-and-count had trained the check "are any two `FillRect`
    dimensions equal *in this file*". They were, twice, and both pairs were
    invisible to that question because the two members are never drawn
    together: a full-width `height: 36.0` fill is a timezone row on the
    Timezone tab **and** the sync-status card on the Sync tab, and a
    `font_size: 10.0` text is the DST badge on one **and** the "Hidden" mark on
    the other. A shape is only unambiguous once you know which tab drew it, so
    every extractor here takes a render already scoped to a named tab and says
    so in its doc comment. The grep must be per-render-path, not per-file.
  - **A zone row carries two independent kinds of "current", and nothing
    asserted they differed.** The keyboard cursor (`surface1`, a raised
    surface) says *where you are looking*; the machine's configured zone (now
    `p.accent`) says *what is in force*. Painting both the same would lose the
    distinction silently, and no membership sweep can see it because both are
    palette roles. `the_zone_you_are_looking_at_is_not_the_zone_in_force`
    parks the cursor on a row that is not the configured one and asserts the
    two rows disagree; defect `KKKKKKKKK` collapses them to prove it.
  - **The clock-face judgement resolved a disagreement the module already had
    with itself.** The main readout on the DateTime tab was `TEXT`; the
    additional-clock readout on the Clocks tab was `BLUE` — the same kind of
    value, two colours, with nothing written down to justify the split. A
    displayed time is a **measurement**: it is neither a position nor an
    invitation (so not the accent) nor a category (so not a fixed hue). Both
    are now `p.text`, and the emphasis the world clock needs is already carried
    by its weight and size. **Where two sites in one module contradict each
    other, the conversion is the moment to resolve it** — "keep the same
    pixels" cannot be applied to both sites at once, so the neutral option does
    not exist.
  - **`SAFE_ACCENTS` must exclude the hues the module freezes.** This panel has
    two frozen scales (`NtpStatus::color`'s four sync states, and the DST
    badge) using green/yellow/red/overlay0. An accent sweep that included those
    hues would let a wrongly-accented site coincide with its frozen neighbour
    on exactly the accent that matters. The set is the seven that collide with
    nothing here: blue, peach, mauve, teal, pink, sapphire, sky.
  - **Deliberate non-change:** the switch knob stays `p.text`, per
    `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`.
- [x] `touchpad.rs` — 12 constants, done 2026-08-22. Harness defects
  AAAAAAAAAAA–RRRRRRRRRRRR (forty-four).
  - **The slider rule, set here for the remaining thirty modules.** This is the
    first converted module with a slider, so it had to decide what a slider's
    three parts are and write it down rather than leave the next module to
    re-derive it: **the track is a recessed surface (`p.surface1`); the fill and
    the knob are both the accent.** The reasoning is the same one that governs
    every other accent site — the accent marks *position and invitation* — and a
    slider is the one control that carries both at once: between them the fill
    and the knob say where the value currently sits and where you would take
    hold of it. The track is neither; it is the space the value moves through.
  - **The erase check found a real production defect, not a test bug.**
    `render_slider_label` pushed the filled-portion `FillRect` unconditionally,
    so a slider resting on its floor (`pointer_speed = 0.1` against a `0.1..3.0`
    range) emitted a `0 × 4` rectangle — a command the compositor has to carry
    and cannot draw, on every frame, for as long as the value stays there. Now
    guarded by `if fill_w > 0.0`. **This is the second time
    `assert_nothing_is_drawn_and_never_seen` has paid for itself on a module
    that was not suspected of having anything wrong with it**, which is the
    argument for running it on every module rather than only where a zero
    dimension looks plausible.
  - **Geometry collides three ways now, and this module has two of them.**
    Module 18 added *across tabs*; this one adds *within one render path* and
    *across widget kinds sharing a helper*. `12.0 × 12.0` is both the status
    light and a slider knob, and both are drawn on the General section — the
    extractors separate them by x (`x < PX + 100.0` vs `x > PX + 100.0`), not by
    size. Worse, `text: label.to_string(), font_size: 12.0, color: p.text`
    appears identically in `render_toggle`, `render_slider_label` and
    `render_choice`, because all three helpers draw their label the same way;
    each harness defect for those sites needs a distinct *forward* anchor
    (`// Toggle track.`, `// Slider track.`, the choice well's `x: x + 250.0`).
    **A shared helper is a geometry collision generator: n callers, one shape.**
  - **The extractor that keys on x alone is not safe either.** `gesture_columns`
    first keyed the fingers column on `x == x0 && font_size == 12.0` — which
    also matches the pinch *choice control's* label, drawn below the table at
    the same x and size. The colour assertion passed (both are `p.text`); only
    the row-count arithmetic failed. Fixed by deriving the row count from the
    two columns that have no such collision and truncating, which is documented
    in the extractor as relying on the table being drawn before the pinch
    control. **A test that gets the right answer for the wrong reason is a test
    that will get the wrong answer later.**
  - **A reported value is body text.** Extending module 18's clock-face
    judgement to three more sites: the gesture table's finger count (was
    `LAVENDER`), its action column (was `BLUE`), and every choice control's
    current value (was `BLUE`). None is a position, an invitation or a
    category — each is a value being *reported*, and a reported value follows
    neither the accent nor a categorical hue. All are `p.text`; the emphasis the
    finger count needs is already carried by its weight, and the three gesture
    columns are told apart by their headings and their x-positions, **which is
    what a table is.**
  - **The reset button's label was wrong, not merely fragile** — the fifth
    module with the `crust`-on-a-categorical-fill shape and the second where the
    coincidence had already broken. It was Mocha `base`, a near-black chosen to
    read on Mocha's *pale* red. On Latte the fill and that label are both pale
    and the button says nothing. It is `readable_on(p.red)` now, and
    `the_reset_buttons_label_can_be_read_on_the_button` asserts both equality
    with `readable_on(fill)` **and** that the two modes disagree — a label
    identical in both modes is this bug returning.
  - **Deliberate non-change:** the toggle knob stays `p.text` on the accent
    track, per `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`. Changing it
    here would be a second change hiding inside this one.
- [x] `overview.rs` — 12 constants, done 2026-08-22. Nine tests, harness
  defects AAAAAAAAAAAAA–EEEEEEEEEEEEEE (thirty-one).
  - **The wash rule, set here for the remaining modules: a wash is a role seen
    through a veil, so the veil is the alpha and the role is everything else.**
    This module has the first two translucent fills to be converted — the
    overlay's backdrop (`mantle` under an animated alpha) and a card the search
    query has dimmed (`surface0` under a fixed `100`). Each takes the *RGB* of
    its role and keeps its *own* alpha; neither is a palette colour at full
    strength and neither should be.
  - **A wash is exactly the thing the membership sweep cannot check, so every
    wash needs its own test.** `assert_drawn_from` compares RGB only — it has
    to, because a role drawn at alpha 100 is still that role — which means
    deleting the alpha entirely turns a wash into an opaque fill that the sweep
    still passes. `a_wash_keeps_its_own_alpha_and_the_colour_of_its_role`
    asserts all three parts separately: RGB equals the named role's RGB, the
    alpha is the expected veil, and the alpha is below 255. The third assertion
    is the one the sweep structurally cannot make. `widgets.rs` has ten washes
    and inherits this rule.
  - **A card's border carries two independent kinds of "current", and the
    accent must not be allowed to collapse them.** Hover says *where you are
    pointing*; focus says *which window has the keyboard*. They are orthogonal —
    the focused window is usually not the one under the pointer — so painting
    them alike loses a distinction silently, and **no membership sweep could
    ever see it, because both would be palette roles.** The border is now a
    three-rung ladder: `surface2` merely present, `subtext0` focused, `accent`
    under the pointer, and `a_cards_border_says_both_where_you_point_and_what_
    has_focus` asserts all three are mutually distinct across every safe accent.
  - **`lavender` for focus was a category colour marking a state.** Focus is not
    a category — it is a condition a card is in — and on a lavender-accented
    desktop the focused card's border would have been the accent, i.e. the hover
    colour, which is the collapse above arriving by a different road. `subtext0`
    is within a few percent of the same pixel and, being a grey, cannot collide
    with any accent the user can pick. This is the general form of the
    module-18 lesson: **a colour that must stay distinct from the accent must be
    achromatic, not merely a different hue.**
  - **Two badges are frozen, and both their labels were wrong.** The minimised
    marker's yellow and the close button's red report facts about a *window*,
    not choices about the desktop; a close button that means destructive on one
    desktop and matches the wallpaper on another has stopped saying anything, so
    `neither_badge_follows_the_accent` pins them. Their `_` and `x` marks were
    both Mocha `base` — the sixth instance of the near-black-picked-for-Mocha's-
    pale-fill shape, and one that breaks on Latte in *both* directions at once,
    since Latte's yellow and red are deep while its `base` is near-white. Each
    is `readable_on()` of the fill it is actually drawn on.
  - **One defect escaped the first proof run, and the hole it found is the
    most general lesson this module produced: an expectation derived from the
    palette asserts what a thing was *supposed* to be painted, not what it
    *was*.** `each_badges_mark_can_be_read_on_the_badge` compared each mark
    against `readable_on(p.yellow)` / `readable_on(p.red)` — the palette's
    roles — rather than against the fill the render actually emitted. Two
    separate failures followed from that one mistake. Reverting a *badge fill*
    to its Mocha literal left the expectation unchanged, so the test that most
    obviously owns the badges did not notice (it was caught by the sweep
    instead). And the mark could be made to answer for the **other badge's**
    fill with no test failing at all. Every expectation is now computed from
    the emitted fill, which is the only form of the assertion that couples the
    two the way the code does. **Where a test can read the thing it is checking
    against out of the render, it must — a palette lookup is a second opinion
    about the same question, and two opinions cannot disagree in a test.**
  - **Neither shipped palette could tell the two badges apart, so the proof
    needed a palette we do not ship.** `readable_on` returns one of two
    endpoints, and Mocha's yellow and red are *both* pale while Latte's are
    *both* deep — so in every mode that exists, `readable_on(yellow) ==
    readable_on(red)`, and a mark answering for the wrong badge is
    indistinguishable from one answering for its own. The test now also runs
    two contrived palettes whose yellow and red straddle the threshold in
    opposite directions, each with an assertion that the fixture *does*
    straddle, so the fixture cannot silently stop discriminating. **A property
    that happens to hold for both shipped configurations is not proved by
    testing both shipped configurations; it needs a configuration chosen to
    break the coincidence.** This is the same shape as the module-18 rule about
    assertions whose truth depends on which accent the user picked, arrived at
    from the opposite direction.
  - **Two fixture bugs, both worth recording because both made a test pass
    while proving nothing.** (1) `collect_thumbs_for_mode` returns only the
    *current* desktop's lane in `AllWindows` mode, and the fixture had parked
    the minimised window on desktop 1 — so the badge assertions were counting
    zero badges and the card-border assertion two cards instead of three. **A
    badge that is never drawn cannot fail an assertion about its colour**; the
    fixture now puts all three window states on desktop 0 and keeps a fourth
    window on desktop 1 purely so `AllDesktops` still has two lanes. (2) The
    card extractor `w > 40.0 && h > 40.0 && w != 400.0` also matched the
    1920×1080 backdrop, which is a mantle wash — so the "a card is `surface0`"
    assertion was being handed the backdrop's `mantle`. Bounded with `w < SW`.
- [x] `context_ext.rs` — 12 constants, done 2026-08-22. Seven tests, harness
  defects AAAAAAAAAAAAAAA–EEEEEEEEEEEEEEEE (thirty-one).
  - **The first module whose constants were *unprefixed*.** They were `BASE`,
    `TEXT`, `BLUE` and so on rather than `MOCHA_BASE`, which is worth noting for
    the twenty-eight modules still to come: the bulk rewrite has to be anchored
    on `\b` word boundaries, and the post-conversion leftover grep cannot key on
    the string `MOCHA_` the way every previous module's could. Verified instead
    by grepping the twelve bare names, which came back empty.
  - **The shadow joins the shared popup shadow, and that is a small deliberate
    behaviour change rather than a rebinding.** It was `rgba(0, 0, 0, 80)`,
    chosen alone; `Palette::shadow()` is `rgba(0, 0, 0, 120)` and its doc
    comment already says it exists because three modules drawing the same kind
    of popup had each picked a depth without reference to the others. This menu
    is the fourth, and the point of the change is precisely that four popups
    which used to sit at four different depths now sit at one.
  - **A shadow is exactly what the membership sweep waves through.**
    `assert_drawn_from` allows black at any alpha — it must, because a shadow is
    not a role — so the depth could drift back to 80, or to fully opaque, with
    the sweep silent. `the_menu_casts_the_shared_popup_shadow` asserts equality
    with `p.shadow()` *and* that the alpha is below 255. This is the same shape
    as module 20's wash rule arriving from the other side: **the sweep's
    deliberate blind spots are a list of the tests a module still owes.**
  - **The module's one accent site is the icon of a hovered extension item, and
    the reason it is one is worth stating generally: a colour that appears in
    exactly one state marks that state.** The hardcoded blue was drawn only
    while the pointer was over the row, which makes it a hover mark, and hover
    is a position. A built-in item's icon merely brightens to `text` instead —
    a hierarchy rather than an inconsistency, since an extension item invokes
    code the shell did not write. The test asserts both halves: the accent when
    hovered, `subtext0` when not, so the accent is carrying the state rather
    than decorating the row.
  - **Nothing else here takes the accent, and two established rules did the
    deciding without a new judgement being needed.** Pointing at a menu row
    lifts it one surface step (`base` → `surface0`), which is what
    `notif_pane`'s hovered list row already did; a selected settings row does
    the same over `mantle`, which is what `storage_settings` and
    `update_settings` already did. The enabled/disabled dot and the "Slow"
    badge are frozen because they report facts about an extension, which is
    module 19's rule. **Twenty-one modules in, most sites now have a precedent
    rather than a decision**, and the value of having written the earlier ones
    down is that this module needed four judgements instead of twenty-six.
  - **`Color` left the production imports entirely.** The module no longer names
    a colour type outside its tests — it reads roles off the palette it was
    handed and never constructs one. That is a small but real signal that the
    conversion is complete in a way a grep for leftovers cannot show.
  - **…and that signal broke the proof, which is the more useful half of the
    story.** Twenty of the thirty-one defects came back `DID NOT COMPILE` on
    the first proof run, for a reason that had nothing to do with the tests:
    each of them reinstates a hardcoded `Color::from_hex(…)` in production
    code, and after the conversion there is no `Color` in production scope to
    say it with. The harness had been left unable to *state* the very defect it
    exists to state. **A defect has to be expressible in the converted source's
    namespace, not the original's** — a constraint that only appears once a
    conversion is thorough enough to shrink that namespace, and one that will
    recur in every remaining module whose `Color` import likewise becomes
    test-only. The fix is to spell the type by its full path
    (`guitk::color::Color::from_hex(…)`), which needs no import and leaves the
    production code's imports alone; the alternative — keeping a `Color` import
    alive purely so the harness can corrupt the file — would have meant
    carrying a dead import in shipped code to please a test tool, which is
    backwards.
  - **The most important finding of this module, and it is not about this
    module: three defects escaped because the *fixture* never drew them.** A
    built-in item's shortcut hint, a slow extension's "loading..." label and the
    submenu arrow all came back `*** NO TEST FAILED ***`, and not one of them
    was a weak assertion. The fixture menu held a built-in with no shortcut
    (`Open`), an extension with `slow: false`, and an extension with an empty
    submenu — so all three colour sites were drawn by no test at all, and the
    sweep was handed a render in which they simply did not appear. **The sweep
    is only as wide as the render it is given.** Strengthening an assertion can
    never fix this, because there is no assertion: a branch nothing takes is a
    colour nothing checks.
    - Fixed by widening `ext_menu()` to take every branch the renderer has —
      `Open` *and* `Copy` (no shortcut / shortcut), an extension with an icon,
      a shortcut and no submenu, and one that is slow, has no icon of its own
      and does have a submenu — and then by pinning that coverage with
      `the_fixture_menu_takes_every_branch_the_renderer_has`, which asserts
      against the emitted commands rather than the fixture's shape, since a
      branch can stop drawing without the entry that feeds it changing.
    - **This is a standing hazard for the twenty-eight modules still to come,
      and it is invisible without a defect run.** Every previous module's sweep
      was equally at the mercy of its fixture and nothing said so; the only
      reason it surfaced here is that the harness names each colour site
      individually and so notices when one of them is unreachable. Treat a
      `*** NO TEST FAILED ***` on a plain membership defect as a fixture-coverage
      report first and a test-strength report second — in this module it was the
      former three times out of three. The practical rule when writing a
      module's fixtures: **enumerate the renderer's `if`s, not its colours.**
  - **One defect's declaration was wrong rather than its test.** "The menu's
    shadow is drawn in a palette role instead of black" was declared as
    something the membership sweep should catch. It is not: `p.crust` is a
    member of both palettes, so the sweep passing it is the sweep behaving
    exactly as documented. Corrected the declaration to expect only the shadow
    test. Worth stating because it is the failure mode of writing the expected
    catchers from intent rather than from the sweep's stated contract — and
    because a `[MISSING:]` note is ambiguous between "the test has a hole" and
    "the declaration overclaims", so each one has to be read against the
    contract before it is believed.

- [x] `widgets.rs` — 11 constants, done 2026-08-22. Eleven tests, harness
  defects AAAAAAAAAAAAAAAAA–UUUUUUUUUUUUUUUUUU (forty-seven).
  - **Four judgements, and three of them were settled by precedent rather than
    argued.** The selected widget's 2px ring takes the accent, by module 21's
    rule that *a colour appearing in exactly one state marks that state* — and
    with a second reason peculiar to this module: a ring floating on the
    wallpaper cannot say "here" with a surface step the way a hovered list row
    can, so the accent is the only mark available to it. The picker joins
    `Palette::shadow()`, as `context_ext` did. The picker's row icons stay
    `p.blue` because every row is drawn identically, so an accent there says
    nothing about any particular row — and it would cost the accent the one job
    it has here. **Within a single render the accent has to mean one thing.**
  - **The one new rule: a meter is not a slider.** Module 19 gave sliders a
    `surface1` track and an accent fill, and the CPU/Memory/Disk bars look
    exactly like sliders. They are not: a slider is something you drag, and
    these are read-outs nobody can move. They are further a *category* set —
    blue is CPU, green is Memory, peach is Disk — and three bars told apart by
    colour stop being three bars the moment they all follow one accent. The
    tracks keep `surface1`, which is the half of the slider rule that does
    survive: a track is a surface either way. The battery glyph's green is the
    same judgement one widget over — green there is the reading itself, not
    decoration, so a red accent would make the widget say something false.
  - **The per-widget shadow is the first shadow deliberately *not* unified.**
    `rgba(0, 0, 0, bg_opacity / 3)` stays, because its depth is a function of
    the widget's own translucency: a widget you can see through casts a shadow
    you can see through, and pinning it to `Palette::shadow()` would make a
    nearly invisible widget cast a solid one. Both shadows in this module carry
    their own assertions, because the sweep waves black through at any alpha
    and is blind to both by design.
  - **The fixture-coverage lesson from module 21 was applied up front and
    earned its keep immediately.** `full_mgr` was built by enumerating the
    renderer's `if`s rather than its colours — `layer_visible`, `edit_mode`,
    per-widget `visible`, `picker_open`, the selection test, the five
    `WidgetKind` arms and the empty-note branch inside one of them — and four
    of the forty-seven defects do nothing but switch a branch off, purely to
    prove that `the_fixture_takes_every_branch_the_widget_layer_has` notices.
    Zero defects escaped for want of a branch, against three in module 21.
  - **Two did escape, for a different reason, and it is the other half of the
    same rule.** "The Disk meter's label is promoted to body text" and "the
    battery's estimate is promoted to body text" both came back
    `*** NO TEST FAILED ***`. The wash test's table of content colours had one
    entry per *kind* of site rather than one per *source* site: `"CPU"` was
    listed and `"Memory"` and `"Disk"` were not, though all three are separate
    `commands.push` calls, and the battery's estimate was not listed at all.
    **A representative sample is not a per-site check.** So the width rule has
    two halves and they fail independently: the *fixture* decides which
    branches draw at all, and the *assertion table* decides which of the drawn
    sites anyone looks at. Module 21 lost three defects to the first; this
    module lost two to the second, with a fixture that was complete. The table
    now carries a comment saying it must not be shortened, since a shortened
    table looks tidier and reads as an improvement.
  - **Extra catchers are worth chasing down even though the harness tolerates
    them.** Widening the wash table meant four defects were now caught by a
    test their declaration did not name. The harness only reports the reverse
    (`[MISSING:]`), so nothing would have complained — but a declaration that
    understates what a defect proves is a declaration that will not notice when
    that coverage later disappears. All four were updated.

- [x] `sound_settings.rs` — 11 constants, done 2026-08-22. Eight tests, harness
  defects Ax19–Ax21 (fifty-three).
  - **Four judgements, and the interesting one is the mirror of module 22's.**
    Exactly three sites follow the accent — the active tab's label, the
    selected spatial mode's label, and the volume-bar fill — and all three are
    "you are here" or "drag me", which is the only thing the accent is allowed
    to say. Section headings keep `lavender` (the accent never marks
    *category*), and the on/off status pair stays frozen `green`/`overlay0`
    (green *means* enabled rather than decorating it, which is module 19's
    rule).
  - **The one new rule: a meter is not a slider, but a volume bar is.** Module
    22 froze the CPU/Memory/Disk bars because nobody can drag a read-out. A
    volume bar is the same *shape* and the opposite *thing*: it is the control,
    so module 19's slider rule applies unchanged — `surface1` track, accent
    fill. The two rules together are one rule stated twice: **the accent marks
    what you can move, not what you are being told.**
  - **Mute is the one state that overrides the slider rule.** A muted bar draws
    `p.red` and keeps drawing `p.red` on a red desktop, an orange one and a
    green one, because at that moment the bar has stopped being a position and
    become a reading — and a reading that matches the accent is a reading a
    user cannot trust. `a_muted_volume_bar_never_looks_like_an_unmuted_one`
    exists to hold that line; the sweep cannot, since red is a member of both
    palettes.
  - **Both halves of the width rule were applied up front, and for the first
    time nothing escaped.** The fixtures were built by enumerating the
    renderer's `if`s (module 21's half) and the assertion tables were written
    one entry per *source* site rather than per *kind* (module 22's half), with
    the anti-shortening comment copied across. Four of the fifty-three defects
    do nothing but switch a branch off, to prove the coverage test notices.
    Result: **53/53 caught, zero escapes** — against three lost in module 21
    and two in module 22. The two halves are now cheap to apply and expensive
    to skip, which is the whole return on having written them down.
  - **The under-declaration audit paid off again, twice.** Two defects were
    caught by a test their declaration did not name — the frozen-fill defect is
    also caught by the mute test, and the disabled-sound defect by the
    state-doesn't-follow-the-accent test. Neither would ever be reported by the
    harness, which only flags the reverse. Both were widened. This audit is now
    a standing step, not a module-22 one-off.
  - **Two tooling traps worth not rediscovering.** (1) Windows Python resolves
    `/tmp` to `D:\tmp` while MSYS bash resolves it to `C:\…\tmp`; writing the
    defect-name filter list with one and reading it with the other silently
    produced *no* filter arguments, so the harness began a full 454-defect run.
    Never pass a path through both interpreters — generate the list inline into
    a shell variable. (2) Killing the harness mid-run skips its `finally`
    restore, leaving the in-flight defect patched on disk; `run_dialog.rs` was
    left modified by that kill. **After any stop of a harness run, `git status`
    must be checked and the damaged file restored with `git checkout --`.**
- [x] `osd.rs` — 11 constants, done 2026-08-23. Eleven tests, harness defects
  Ax22–Ax25 (ninety-eight). **98/98 caught, zero escapes.**
  - **The width rule has a third gap, and this module is where it showed.**
    Modules 21 and 22 taught that the sweep is only as wide as the render it is
    given, and that a per-*kind* assertion table is not a per-*site* one. Both
    were applied here up front — and both still missed a whole class. A
    per-source-site *text* table and a per-source-site *rectangle* table check
    the colours that reach the renderer; neither can see the **choice** sites,
    the `match`/`if` expressions that decide *which* role gets handed to a
    shared renderer. `render_content` makes 15 such decisions and `icon_info`
    another 10, and not one was reachable: the sweep waves them through
    (every arm names a role, and a role is a member of both palettes) and the
    two role tables render only two fixtures. That is 25 sites checked by
    nothing, which is worse than modules 21 and 22 lost combined. The fix is an
    eleventh test, `every_kind_draws_its_icon_in_the_colour_that_kind_claims`,
    which spells out all 25 mappings. **Generalised: n source sites means n
    assertions — and a site that only *selects* a colour is a source site too,
    even though it draws nothing itself.**
  - **An OSD overlay takes no accent at all — the deliberate mirror of module
    23.** Module 23's rule was "the accent marks what you can move, not what
    you are being told"; a volume *bar* is draggable so it takes the accent.
    The volume *overlay* is the same subject and the opposite answer: it is
    pure feedback that appears, reports and fades, and nothing in it is
    touchable. So zero of the overlay's sites follow the accent, held by
    `no_colour_the_overlay_draws_ever_follows_the_accent`. The settings panel
    below it *is* interactive and has exactly three accent sites, held by a
    test that counts them — a count, not a list, so a fourth site added later
    fails rather than passing unnoticed.
  - **Volume-blue and brightness-yellow are a category pair, so both freeze.**
    Tempting to give volume the accent since it is the most-shown overlay; that
    would make the two indistinguishable on a blue desktop and identical on a
    yellow one. The pair is what carries the meaning, so neither half may move
    independently, and `volume_and_brightness_stay_a_pair_you_can_tell_apart`
    says exactly that.
  - **Proving ink is *derived* needs the accent to vary, and the stock accents
    are all too similar to do it.** `ink_drawn_on_a_coloured_fill_is_readable_
    in_both_modes` originally used the four `SAFE_ACCENTS`, which are all
    pastel — `readable_on` answers the same dark endpoint for every one of
    them, so a hard-coded constant passed. The test only bites once it renders
    a deliberately dark accent (`0x0020_3050`) *and* a deliberately pale one
    (`0x00F5_D0E0`) and demands the ink flip between them. Any future
    `on_accent`/`readable_on` test needs the same treatment; four similar
    fixtures prove nothing about a derivation.
  - **`text_saying` — one source site, many rendered instances.** The helper
    first asserted exactly one match and failed, because the settings panel
    draws four ticks from one `"✓"` expression. The correct question is not
    "which one" but "do they all agree": at least one match, and all matches
    the same colour. If they ever disagree the site has grown a branch that
    needs naming, so the assertion catches that too rather than silently
    reporting whichever came first.
  - **The under-declaration audit found four, and over-declaration one.** Four
    defects were caught by tests their declaration did not name (an album
    branch-killer also trips the text-bounding test; two battery defects also
    trip the text-role table; the Success-icon defect also trips the
    pair-distinctness test). One declared a test that did *not* fire — recolouring
    the selected position dot changes no branch, so
    `the_fixtures_take_every_branch_the_osd_has` was wrong to list. The harness
    reports only that second direction, which is the rarer one; the audit
    script is what catches the first.
  - **clippy trap: `unusual_byte_groupings`.** `Color::from_hex(0x2030_50)` is
    rejected — hex digits must group in equal-size runs. Write the full eight
    (`0x0020_3050`), which is what the rest of the file does anyway.
- [x] `privacy_settings.rs` — 11 constants, done 2026-08-23. Eight tests,
  harness defects Ax26–Zx27 (fifty-two).
  - **The width rule has a fourth gap: an assertion is only as
    discriminating as the palette it renders under.** Modules 21/22/24 taught
    that the sweep is only as wide as its render, that a per-kind table is not
    a per-site one, and that a site which only *selects* a colour is a source
    site too. All three were applied here up front, and the first proof run
    still left 19 defects declared-but-uncaught — because the three role
    tables rendered **Mocha only, with the stock accent**. That is structurally
    blind to exactly the two mistakes this conversion makes: a constant frozen
    back to its Mocha value is *identical* to the role that replaced it when
    viewed in Mocha, and a site naming `p.blue` instead of following the accent
    is identical to the stock accent, which *is* blue. **Testing a conversion
    in the palette it was converted from hides precisely the failures that
    conversion causes.** The fix is `table_palettes()` — both modes, and in
    each an accent (`0x00FF_8C1A`) deliberately outside either palette — which
    every role table now loops over, with `"{mode}"` in all 38 failure
    messages. 15 of the 19 resolved on that change alone.
  - **An expectation written in terms of the code under test cannot fail.**
    The text table asserted the app-state label against
    `PermissionState::Allowed.color(&p)`. Swap the `Allowed => p.green` arm and
    *both sides of the comparison move together*: the row a user reads as
    "allowed" can turn any colour at all and the table stays green. Two arm-swap
    defects walked through it. Assert the **role literal** instead, and list all
    three states — that pins the value, and the three-way spread still proves
    the call site asks the method, because a flat literal there would give all
    three rows one colour. Generalised: **if the expected value is computed by
    the thing being tested, the assertion is checking that a function equals
    itself.**
  - **An exemption defined by the property under test exempts the bug too —
    the accent test had to be written three times.** *Attempt 1* collected
    every colour equal to green, red or overlay0 and compared *that list*
    across accents. The filter runs before the comparison, so a site that stops
    drawing green and starts drawing the accent drops out of the list — for
    every accent equally. The lists still matched; it was checking that the
    sites which stayed put stayed put. *Attempt 2* fixed that by comparing every
    command under two out-of-palette accents A and B, and skipping any site that
    drew A in the first render and B in the second, "because that is a site
    meant to follow the accent". Three defects walked through it — an enabled
    resource, an allowed log entry and a switched-on toggle, all reporting state
    in the accent — because **a site that wrongly follows the accent satisfies
    the exemption exactly.** *Attempt 3* is the one that works: every command
    must match under both accents, any command that moves must move exactly
    A→B, and the number that move must equal a **declared per-state count**
    (one for the tab strip's active label; two on the general tab, which adds
    the selected telemetry level). Generalised: **you cannot recognise the
    legitimate instances of a property by testing for that property — the bug
    has it too. Count them instead**, so a new one fails rather than joining
    the exemption. This is the same shape as module 24's "count, not list" rule
    for accent sites, arrived at from the opposite direction, and it is now the
    third time a *whitelist by description* has silently absorbed a defect.
  - **Four judgements, all recorded in the module's prose header.** (1) Two
    sites follow the accent, both meaning "you are here" — the active tab's
    label and the selected telemetry level's label; held by a test that
    **counts** them, so a third added later fails. (2) Allowed/Denied is a
    *category*, not a decoration, so green and red freeze across every accent —
    a privacy panel whose status a user cannot trust at a glance is worse than
    no status at all. (3) Section headings stay `lavender`; the accent never
    marks category. (4) The tab strip's own fill is position but not accent
    (`p.surface0`/`p.mantle`) — only the label moves.
  - **Traps.** `gen` is a reserved keyword in Rust 2024 and cannot name a
    fixture variable. Harness patterns must contain literal `✓`/`✕`; the Rust
    source stores the characters, not `\u{}` escapes. And `text_containing`
    fired its ambiguity guard on `"Permissions"`, which is a substring of the
    title `"Privacy & Permissions"` — the guard converted a silent
    wrong-site comparison into a loud failure, which is what it is for; tab
    labels now use a `text_exact` helper.
  - **The harness now reports both directions of declaration error.** It only
    ever printed `[MISSING:]` — a test a defect *declared* but that did not
    fire. The reverse, a test that fired but was not declared, was found by an
    audit script run by hand, which meant it was found when someone remembered
    to run it. This module had **sixteen** of them, all created the moment one
    test became far more discriminating than its declarations claimed. That
    direction matters because the declarations are the only record of what the
    suite is known to prove: left stale, the next person to prune a
    "redundant" test cannot see what they would be giving up. Both directions
    are now inline as `[MISSING:]` / `[UNDECLARED:]`, with a closing tally.
- [x] `print_manager.rs` — 11 constants, done 2026-08-23. Eight tests, harness
  defects Ax28–Nx29 (forty).
  - **The first module where the previous module's lesson was applied before
    the proof run rather than after it.** Module 25 lost 19 defects to role
    tables that rendered Mocha with the stock accent; every table here renders
    both modes with an accent outside either palette from the first line of
    the first test. This module needed it more than any so far, because
    `JobState::Queued` is `p.blue` — and the stock accent *is* blue, so under
    it "correctly frozen to blue" and "wrongly following the accent" are the
    same pixels. Two of the forty defects exist purely to hold that line.
  - **Both colour-choice methods are unreachable from `render`.**
    `Printer::status_color` and `JobState::color` are called by nothing else in
    the file — they are pure *selection* sites in module 24's sense. Without an
    explicit table naming all nine arms, every one of them is checked by
    nothing at all: the membership sweep waves them through, because each arm
    names a role and a role belongs to both palettes. A colour method with no
    call site is the easiest kind of site to forget, because grepping the
    render function for colours will never show it.
  - **Four judgements.** (1) `status_color`'s offline/busy/ready red/yellow/
    green is a category — red *means* "this printer will not print" — so it
    freezes. (2) `JobState::color` is the same, six ways, with all fifteen
    pairs asserted distinct under five accents and both modes. (3) The Print
    button is the dialog's default action, which *is* the accent's job, so
    `p.accent` fill with `p.on_accent()` ink; Cancel is not the default action
    and keeps `p.surface1`/`p.text`. (4) The selected printer's name marks the
    choice you have made and takes the accent, while the field it sits in stays
    `p.surface0` — position is marked by the label, not by repainting the
    furniture, the same split as module 25's tab strip.
  - **The accent count has to allow for derived ink.** The counting rule from
    module 25 says every command must match across two accents except a
    declared number. Here three commands move, not two: the printer name and
    the button fill take the accent, and the button's *ink* moves as well
    because `on_accent` is derived from it. The test classifies each moving
    command as accent-valued or derived-ink and fails on anything that is
    neither, so the derived site is accounted for rather than exempted.
  - **Two sites drew the identical string in different roles.** The dialog
    title and the Print button both draw exactly `"Print"`, and the caption
    `"Printer:"` contains it — three sites, two roles, one substring. A
    `text_containing` lookup would have compared the wrong one silently. Added
    `text_exact(cmds, want, size)`, matching text *and* font size. Whenever a
    module draws one word in two roles, the lookup helper needs a second
    discriminator; the ambiguity guard only tells you that you have the
    problem, not which site you meant.
- [x] `power.rs` — 9 constants, done 2026-08-23. Ten tests, harness defects
  Ax30–Bx32 (fifty-four).
  - **The first module whose surface deliberately does not follow the user's
    mode**, and that turns out to matter for the proof, not just the pixels. A
    screen saver blacks out the display in both modes — a light one is a lamp
    pointed at a sleeping user — so Latte's roles, which are picked to sit on
    white, are the wrong palette for that surface: Latte `text` is nearly black
    and would vanish on it. The saver pins itself to the dark palette and, since
    nothing it draws is a position or an invitation, takes no palette argument
    at all. Making the independence *structural* rather than asserted is the
    better half of the fix: a later edit cannot accidentally make the saver
    follow the theme, because there is no theme in scope to follow.
  - **Seventh lesson for the width rule: pinning a surface to one mode disarms
    the membership sweep for that surface.** The sweep works by rendering in
    Latte and finding a value Latte does not contain. A surface pinned to Mocha
    contains every Mocha value *by construction*, in both modes — so a leftover
    `COL_LAVENDER` is bit-for-bit `screen_palette().lavender` and there is no
    observable difference for any test to see. The sweep is still worth running
    (it catches an inline literal that is no role at all, and it pins the star
    field's greys and the rain's greens to the two ramp shapes they are allowed
    to have), but for such a surface the per-site role tables are not a
    supplement to it: they are the entire proof, and anyone who trims them as
    redundant is deleting the only check there is. The harness block says the
    same thing from the other side — the saver's sites carry no freeze-to-Mocha
    defect, because such a defect would be undetectable and declaring it would
    be declaring a test that cannot exist. They reach for the *light* palette
    instead, which is the failure pinning can genuinely suffer.
  - **Lesson 5 applies to which palette, not just which role.** The saver's role
    tables first wrote their expectations as `screen_palette().lavender`. That
    is the module-25 tautology one level up: a `screen_palette` that started
    returning the light palette takes both sides of the comparison with it. The
    expectations now name `Palette::for_mode(false)` directly, and the defect
    that flips the pinning is caught by three tests instead of none.
  - **Four judgements.** (1) The saver follows neither mode nor accent. (2) The
    battery gauge is a *measurement* — red at critical, yellow at low, green
    when nearly full, blue otherwise — so it freezes; note its "otherwise" arm
    is `p.blue` and must stay `p.blue`, which is the module-26 trap again and is
    why every table renders under an off-palette accent. (3) The four power
    profiles are a category, so none of them is the accent, not even Balanced.
    (4) A star's grey is its depth and a glyph's green is its age in the column:
    both are computed ramps, not roles that were missed, and the sweep declares
    them as the two shapes `rgba(v, v, v, 255)` and `rgba(0, v, 0, 255)` rather
    than waving arbitrary colour through.
  - **The logo's ink is derived but cannot be proved so.** `readable_on(sp.blue)`
    sits on a plate that never varies, so — unlike the accent cases, where the
    fill moves and the ink must move with it — a frozen endpoint and a
    derivation are indistinguishable here. The test asserts ink ==
    `readable_on(plate as actually drawn)`, which is a consistency check rather
    than a derivation proof; what it does catch is the failure that matters and
    the one the code actually had, a *named* role (`COL_BASE`, dark) on a plate
    whose lightness a later change could flip.
  - **The membership sweep can never catch a site that reaches for the accent,
    in any module.** `accent` is one of the palette's twenty-one roles, so a
    command drawing it is accounted for by construction — in either mode, under
    any accent value. Thirteen of this module's defects were declared against
    the sweep on the assumption that an off-palette accent would make them
    visible to it; it caught none of the thirteen, and the accent count and the
    role tables caught all thirteen. This is not a gap to fix — the sweep asks
    "is this a colour the palette can account for?", and the accent is one. It
    is a fact about what the sweep is *for*: it finds a **leftover constant**,
    and nothing else. For every site that must not follow the accent, the accent
    count is not a supplement to the sweep; together with the per-site table it
    is the whole check. Read with lesson 7 above, the sweep's reach is now
    precise: it cannot see a wrong role, it cannot see the accent, and on a
    mode-pinned surface it cannot see a leftover constant either.
  - **`Palette::for_mode(true).base` is exactly the `readable_on` light
    endpoint,** which the sweep allows everywhere on purpose, so "the screen
    saver lights the display with Latte's background" is invisible to it. The
    black-out test catches it, which is the test that owns that property.
    Generally: the two endpoint values are sweep-transparent, so a defect that
    happens to land on one is only ever caught by a property test.
- [x] `login_screen.rs` — 11 constants, done 2026-08-23. Sixteen new tests (50
  in the module), harness defects Ax33–Lx35 (sixty-four), all sixty-four
  caught, none escaped.
  - **Eighth lesson for the width rule: the sweep is blind to the two
    `readable_on` endpoints, and this module draws one of them nearly
    everywhere.** Most of what a greeter draws sits on a background the shell
    did not choose — a photograph, a gradient, a colour a user picked — so no
    palette role is legible on all of them. The answer (the `icons.rs`
    precedent) is `on_wallpaper()` plus a hard `text_shadow()` at +1/+1. But
    `on_wallpaper()` is the constant `LIGHT_EXTREME` = `#EFF1F5`, which is
    *also* the light `readable_on` endpoint, which the sweep allows
    unconditionally in **both** modes. So a site that took `on_wallpaper()`
    where it owed `p.text`, or the reverse, is invisible to the sweep — and it
    is a legibility bug that looks perfect in every screenshot taken against
    the default background. Five defects reproduce exactly that confusion
    (Kx33, Ox33, Dx34, Vx34, Xx34) and the sweep caught none of them.
  - **The fix was to make the boundary a shape, not a colour.** Text on the
    background goes through one helper, `push_on_background`, which emits *two*
    commands; text on a panel this module filled is pushed directly and emits
    *one*. The test helpers mirror that exactly — `panel_text` asserts one
    command, `floating_text` asserts two with the shadow one pixel down-right —
    so a site that changes sides fails whichever helper names it, without any
    colour comparison being involved. `exactly_seven_things_in_the_full_render_sit_on_the_background`
    then counts the shadows, so a *new* floating site nobody named is caught
    too. That is lesson 6 (count them, don't recognise them) applied to a
    property the sweep structurally cannot see.
  - **Sharpest measurement of the session: the wallpaper-ink tests cannot
    detect a module that ignores its palette entirely.** Defect Ix35 makes
    `render` shadow its parameter with `Palette::for_mode(false)` — the exact
    failure this whole conversion exists to prevent. Nine tests caught it. The
    two wallpaper-ink tests did not, and could not: `on_wallpaper()` is a
    constant, `on_wallpaper_dim()` is that constant at alpha 200, and
    `text_shadow()` is black, so all three are mode-independent *by
    construction* and a frozen render still draws them correctly. The corollary
    is a rule for every later module: **a site whose colour does not vary with
    the mode contributes nothing to the "did this module read its palette at
    all" question, and needs a separate site that does.** Here that is the role
    tables on the panels.
  - **A near-miss with the same root cause, found by the harness rather than
    reasoned out.** Defect Cx33 resolves the theme background to `p.base`
    instead of `p.crust`, and unexpectedly tripped the floating-text count —
    because Latte `base` *is* `LIGHT_EXTREME`, so the background fill was
    counted as a seventh piece of wallpaper ink. Harmless here, but it means
    the counting test is coupled to which role the background takes; if a later
    edit makes the login background `base`, that test needs re-reading rather
    than re-baselining.
  - **`Default` could not name a colour, so it stopped trying.** The default
    background was `SolidColor(CRUST)` with `CRUST` a Mocha literal in this
    file — a default that had already made the choice the palette exists to
    make, and the direct cause of the greeter staying black for a user who had
    asked for the light theme. `Default::default` takes no arguments and so can
    never be handed a `Palette`; the fix is a payload-free `Theme` variant
    resolved at render time. `the_default_background_defers_its_colour_to_the_palette`
    pins that the default carries *no* payload, which is not a restatement of
    the code: it is what stops a future edit reintroducing a literal by
    changing an argument. The `match` in `render_background` names the three
    colourless variants explicitly rather than using `_`, so adding a variant
    is a compile error instead of a silent dark rectangle.
  - **Three judgements about the accent, consistent with modules 19–27.** A
    *default action* is an invitation, so Sign In takes the accent and its label
    is `on_accent()` — derived, and proved derived by sweeping the accent from
    `0x00` to `0xF0` and requiring both `readable_on` endpoints to be observed.
    A *selection marker* is position, so the chosen avatar carries the accent in
    the row list and keeps it in the password panel. A *refusal* is neither, so
    the error border and message stay `p.red` and the lockout notice stays
    `p.yellow` under every accent. Note that defect Ox34 — freezing the Sign In
    label to `#11111B` — is caught by **one** test only: the sweep allows it (it
    is an endpoint), the deleted-constants test exempts it (it is `CRUST`), and
    the role table passes it (under the off-palette accent, `readable_on`
    genuinely answers `#11111B`). Only the accent-sweep test sees it. A module
    that draws a coloured button and has no such test has no check on that
    label at all.
  - **The sweep found a real rendering bug that nobody put there.** It rejected
    `#CBA5F7` — one step off Mocha `mauve` — in a gradient whose two endpoints
    were *both* `mauve`. The band arithmetic truncated rather than rounded, and
    `a*(1-t) + a*t` lands a hair below `a` in `f32` for most `t`, so a flat
    "gradient" came out as twenty stripes alternating between the colour the
    user chose and one darker, and every real gradient was biased a step dark
    along its whole length — invisible there, because a gradient is expected to
    change. Extracted as `lerp_channel` with `.round().clamp(0.0, 255.0)` and
    pinned by two tests, one for the flat case and one for the midpoint (127.5
    rounds to 128 and truncates to 127, so the fix cannot be "special-case
    equal endpoints"). Worth recording as evidence for the method: a membership
    sweep written to catch a missed *constant* caught an arithmetic error in
    code that had nothing to do with the conversion.
- [x] `taskbar.rs` — 10 constants plus four inline `Color::rgba(…)` triples,
  done 2026-08-23. Nineteen tests in the module (ten new, six reworked),
  harness defects Ax36–Ix38 (sixty-one), all sixty-one caught, none escaped.
  - **Ninth lesson for the width rule: a set-membership table cannot see a
    *permutation* of the set.** The button-background ladder has four rungs —
    `with_alpha(surface0, 128)` running, `surface1` focused, `surface2`
    hovered, `with_alpha(surface1, 180)` for the dragged ghost — and the first
    version of `each_button_state_draws_the_background_its_role_names` asked
    `bgs.contains(&p.surface1)` and `bgs.contains(&p.surface2)` for each. That
    is a test of the *set* of colours drawn, and a render that swapped the
    hovered and focused rungs draws exactly the same set. The whole defect
    class this module is most likely to have — a ladder re-seated one rung
    off — was invisible. The fix is that `fills()` now carries each rect's `x`
    and a `button_x(i)` helper reproduces the layout arithmetic, so every
    assertion names a *site* (`at(button_x(1)) == p.surface1`) rather than
    asking whether a colour is present anywhere. Generally: **whenever the
    per-site table's sites are interchangeable in shape, index it by position,
    or it degrades from a table into a checklist.** `bgs.len() == 4` plus an
    explicit "no background at `button_x(2)`" close the two remaining holes
    (an extra rung, and an idle button that drew one).
  - **Corollary found in the same pass: an assertion made at a stock default
    value cannot fail.** `every_colour_in_the_context_menu_is_in_the_role_it_claims`
    asserts the menu is filled with `p.panel_bg()` rather than `p.base` — a
    real distinction, because a floating menu is a panel and a panel is
    translucent. Except that `Palette::for_mode` sets `panel_alpha: 255` in
    *both* branches, at which `panel_bg() == base` exactly, so the assertion
    was true of a menu filled with either and proved nothing. The probe
    palette now sets `panel_alpha = 200`, and `accented()` carries
    `assert_ne!(p.panel_bg(), p.base, …)` so the day someone changes the
    default back the fixture fails instead of quietly going vacuous. This is
    the same shape as the module-26 accent trap: **a test fixture must move
    every value the assertions discriminate on, and the stock palette is not a
    fixture — it is the one input guaranteed to make defaults indistinguishable.**
  - **The one principled exception to "the accent marks position, never
    category".** Modules 19–28 established that the accent marks position and
    invitation. A taskbar draws two position-ish marks *simultaneously and a
    few pixels apart* during a drag: the focus underline ("you are here", 16×3)
    and the drag insertion caret ("release here", 2×36). Both are small bars;
    if both take the accent they are told apart only by aspect ratio. So the
    caret takes `p.green` — `drop_target()`'s hue, matching the precedent in
    `appearance` that a drop target differs from a selection *by hue and not
    merely by alpha* — but at full opacity, because a 2px bar has no area in
    which to be translucent.
    `the_drop_caret_is_never_the_same_hue_as_the_focus_underline` pins it in
    both modes and under an off-palette accent, so the rule cannot be undone
    by someone "unifying" the two marks.
  - **The window-count badge is a *measurement*, so it keeps its named hue.**
    It reports how many windows an app has; that is a quantity, not a position
    or an invitation, so it stays `p.red` under every accent
    (`the_count_badge_does_not_follow_the_accent`). Its digit is the reverse
    case — ink on a coloured fill, so `readable_on(p.red)`, and *proved*
    derived rather than frozen by the two modes disagreeing about it: Mocha
    `red` `#F38BA8` is light enough to want `#11111B`, Latte `red` `#D20F39`
    is dark enough to want `#EFF1F5`. A single-mode test could not tell that
    from a leftover `MOCHA_MANTLE`.
  - **The ladder was re-seated, not merely renamed.** The old constants had
    hovered = `SURFACE1` and focused = `SURFACE0`, which puts the *transient*
    state a rung above the *persistent* one on a bar where both are visible at
    once. The roles now name the ordering they mean —
    `with_alpha(surface0, 128)` < `surface1` < `surface2` for running <
    focused < hovered — and idle draws no rect at all rather than a
    transparent one. Similarly the section divider moved from `surface2` to
    `overlay0`, the role that actually means "separator"; `surface2` remains
    only where it is an outline (the context menu's stroke). A pure
    find-and-replace conversion would have preserved both mistakes, which is
    the argument for reading every site rather than mapping constant names to
    role names.
  - **The lesson generalises past colour, and the harness proved it.** Defect
    Hx38 clears the fixture's `hover_index` and was caught by *one* test — the
    positional ladder table. `the_fixture_takes_every_branch_this_module_has`,
    whose entire job is to notice a fixture that stopped covering something,
    missed it: it asserted `bgs.len() == 4`, and the hovered app is *also*
    running, so it kept drawing a background — just a different one. A count
    of four cannot tell "hovered, focused, running" from "running, running,
    running". The branch test now checks each of the three rungs is present by
    colour. Worth stating as a rule, because it is the same failure at one
    remove: **a coverage test that counts is as blind to a permutation as a
    membership table is, and a fixture-coverage test is exactly where that
    blindness is most expensive** — everything downstream of it is then
    vacuous rather than merely wrong.
  - **Four of the five under-caught verdicts were mis-declarations worth
    recording, because each names a limit of a *kind* of test.** (1) A
    pre-conversion test named for a site often asserts that site's *geometry*
    and never its colour (`test_render_empty_taskbar` against a frozen bar
    background). (2) **A consistency check cannot see a change that moves both
    sides:** the digit test compares ink against `readable_on(badge as
    actually drawn)`, so re-rolling the badge to another role keeps the pair
    consistent — and Mocha yellow and Mocha red happen to want the same
    near-black ink, so it does not even shift the value. (3) A fixture
    weakened by removing one item is caught only by whichever test reads the
    value that actually changed: dropping the third window still leaves a
    badge (any count above one draws one) and still leaves it `p.red`, so only
    the digit test — which selects the text by its content, `"3"` — notices.
- [x] `language_settings.rs` — 11 constants over three tabs, done 2026-08-23.
  Forty-five tests in the module (twelve new, three reworked), harness defects
  Ax39–Bx41 (fifty-four), all fifty-four caught, none escaped.
  - **Tenth lesson for the width rule, and the most useful one so far:
    *writing the defects is what finds the holes; reviewing the tests is not.***
    This module was committed with 44 passing tests — a two-mode sweep, a
    branch-coverage test, an accent count and six judgement tests — and read as
    thorough. Then the defect list was enumerated one colour site at a time,
    each asking "which test names this back?", and for three whole classes the
    answer was *none*: the current-language card dropping to the list rows'
    rung, the selected and unselected row rungs exchanged, and the fixture
    selecting the row that is already current. All three are
    **role-for-neighbouring-role** substitutions — both values are legal
    members, so the membership sweep accepts either, and no count changes, so
    the coverage test balances. The fix was a per-site table
    (`every_site_draws_the_role_it_claims`) whose row assertions are a
    *positional vector*, not a set: `{surface0, surface0, surface1}` is the
    same multiset whichever row is raised. Three of the fifty-four defects are
    caught by that table and nothing else. **Enumerate the defects before
    declaring a module converted — the declaration list is the coverage
    report, and it is the only one that cannot flatter itself.**
  - **When the module doc enumerates N things, the test must enumerate the
    same N.** The doc named three accent-carrying position marks including the
    default currency's row; the accent count covered the Language and Formats
    tabs and stopped, so the Region tab — and that currency row — was
    unguarded. The doc was ahead of the test, which is what made the gap
    findable at all. The count is now per tab (3 / 1 / 2), and the differing
    counts are what stop three assertions from being one weak assertion
    repeated. The doc was also miscounting in the other direction: there are
    three *axes* but four *sites*, because the current language is marked
    twice — a bar beside its row and the row's own name.
  - **The `.take(12)` trap: a default dataset is not a fixture.** The stock
    language list holds 20 entries, every incomplete one sits at index 12 or
    beyond, and the list renders `.take(12)`. So the default fixture cannot
    reach the "Partial" badge *at all* — the badge fill and its derived ink
    would have been swept without ever being drawn, and every assertion about
    them would have passed vacuously. The fixture builds its own three-language
    list instead. Check that each branch you mean to sweep is reachable **from
    the data you hand the renderer**, not merely present in the code.
  - **A selector must discriminate on something only its target has.** The tab
    strip was first selected by `height == 32.0` — which also matches the
    current-language marker bar, 4×32 — so "three tabs are drawn" counted
    four. It selects on `y == 60.0` now. Same family as the module-26 accent
    trap: a discriminator shared with another site is not a discriminator.
  - **A filtering fixture must match every row it wants rendered.** The search
    query started as `"an"`, which does not match "English (United States)";
    the current language dropped out of the list and took the marker bar and
    the accented row label with it, silently. `"n"` matches all three rows.
    Pinned as defect Wx40, which puts `"an"` back.
  - **The `readable_on` endpoint blindness, twice, and the second is sharper
    than any previous instance.** Defect Gx39 freezes the active tab's label to
    `0x11111B`: the membership sweep allows it (it is an endpoint) and the
    deleted-constants test excludes it (it is also Mocha `crust`), so only the
    accent sweep sees it. Defect Ex40 freezes the badge's ink the same way —
    and near-black is the *right* answer in the dark render, so the
    branch-coverage test, which runs dark only, passes as well. Only the
    two-mode comparison catches it. **A derived-ink site needs a test that
    drives its input across a range or compares the two modes; a single-mode
    assertion on such a site is structurally incapable of failing.**
  - Judgements, for the record: the accent marks position only (selected tab,
    current language, current currency); the "Partial" badge is a property of
    the *data*, so it keeps `p.yellow` and must never follow the accent; ink on
    a fill this module chose is derived (`readable_on(p.accent)`,
    `readable_on(p.yellow)`); headings are a two-rung hierarchy (`p.lavender`
    15pt Bold over `p.subtext1` 13pt Bold, the convention already set by
    `datetime_settings` and `notification_settings`); and an absent value is
    dimmer than a present one (`p.overlay0` placeholder, `p.text` query).
  - The seven lavender headings looked at first like the same category error
    as the taskbar's lavender underline and were investigated as such. They are
    not: `p.lavender` at 15pt Bold is the established section-heading rung
    across three already-converted modules, so the mapping is a straight
    `LAVENDER → p.lavender`. The fourteen verbatim copies of that heading push
    are logged separately as
    `TD-C-EVERY-SECTION-HEADING-IS-WRITTEN-OUT-BY-HAND`; they cannot be
    collapsed inside a per-module conversion without invalidating earlier
    modules' harness patterns.
- [x] `default_apps.rs` — 11 constants over 33 colour sites and three tabs,
  done 2026-08-23. Thirty-five tests in the module (nine new), harness defects
  Ax42–Rx44 (seventy), all seventy caught, none escaped.
  - **The conversion found a real UI defect, and writing the judgement down is
    what found it.** The module doc now says "the accent marks which app is in
    force, and nothing else". Stating that as something refutable made it
    obvious that the app name under each category card was drawn `p.accent`
    *unconditionally* — including on the two cards that read "None set",
    because `builtin_apps()` ships no web browser and no email client. So
    twelve of twelve cards were accented, and a mark carried by everything
    marks nothing; worse, the two cards that actually wanted the user's
    attention looked exactly like the ten that were already settled. Fixed to
    `p.overlay0` in a separate commit from the mechanical conversion, so the
    harness proof of the conversion stays unambiguous. **A judgement you can
    state in one sentence is a judgement you can check; the eleven constants
    had been sitting on top of this bug for as long as they existed.**
  - **The `readable_on`/`CRUST` collision, and the first module where it is
    load-bearing rather than merely noted.** The ink on the current app's chip
    used to be the constant `CRUST` (`0x11111B`). `readable_on` answers exactly
    `0x11111B` for the *stock* accent, whose luma is 175 — so at the shipped
    accent a leftover constant and the correct call produce the same pixel.
    The membership sweep cannot help either, because it allows both
    `readable_on` endpoints outright (`palette_check`'s documented hole). Only
    sweeping the accent across all 21 roles separates them, because the
    expected ink flips endpoints as the accent's luma crosses 140 and a frozen
    value cannot flip. Defect Fx43 — the site naming `p.crust` instead of
    calling `readable_on` — is caught by **that test and nothing else**, out of
    thirty-five. The test closes with `seen.len() == 2` so that a palette which
    happened to be all-dark could not quietly degrade it into a constant
    comparison.
  - **Four fixture traps in one default dataset, the `.take(12)` lesson
    generalised.** `DefaultAppsSettings::default()` cannot exercise this module
    at all: every category has exactly one handling app, so the chip `else`
    arm never renders; nothing is customised, so *both* peach sites are dead;
    and every builtin is a system app, so the third-party count is always 0 and
    the `is_system` badge is never absent. One rival music player plus one
    `.mp3` custom association turns all four branches on. **Check each branch
    is reachable from the data you hand the renderer, not merely present in the
    code** — and check it before writing the assertions, because an assertion
    on a branch that never renders passes vacuously and looks like coverage.
  - **Seven sites had no assertion, and only the defect enumeration found
    them.** The module was committed with 26 tests reading as thorough. Writing
    the defect list one colour site at a time — the module-30 lesson applied
    rather than restated — turned up seven sites nothing named: the category
    icon, both search-box fills, the extension rows, the *ordinary* (non-custom)
    ink on a file-type row, an installed app's own name, and the join line under
    it. Every one would have survived a swap to a neighbouring role. Two are
    worth their own note. The File types search box and its extension rows are
    both `width` × 32, so geometry cannot separate them — but `fills_sized`
    preserves command order and the box is drawn first, which is enough to
    index them. And the ordinary ink is the `else` of the peach branch:
    **both arms of one `if` are two sites, and only one of them had a test.**
  - **Two search boxes sharing one query field are still two sites.** They are
    byte-identical in the source, which is exactly why the harness patterns for
    them have to reach out to the placeholder string to disambiguate — and why
    the test loops over both tabs. Checking only the File types box would have
    left the Installed apps box free to draw anything.
  - Judgements, for the record: the accent marks which one is in force and
    nothing else (three sites, per-tab counts 12 / 1 / 1 — a dozen legitimate
    accented marks are on screen at once, so the taskbar's "exactly one" test
    cannot be borrowed and this needs a per-site table *plus* a count); peach
    marks a departure from the shipped defaults, which is a state and not a
    position, so it must hold still while the accent sweeps all 21 roles; the
    content well is one rung below the panel, asserted as an *ordering* rather
    than a pair of literals so it fails on a palette whose crust was made
    lighter than its base; and ink on a fill this module chose is computed,
    never named.
- [x] `launcher.rs` — 14 constants over 15 colour sites, done 2026-08-23.
  Thirty-seven tests in the module (eight new), harness defects Ax45–Fx47
  (fifty-eight), all fifty-eight caught, none escaped.
  - **The deleted-constants test rendered one of the module's three states, and
    only the harness noticed.** The launcher draws three different trees: an
    empty field (placeholder, no query), a typed one (query, rows), and a typed
    one that matches nothing (query, "No results found", no rows). The test
    swept the first only — so a Mocha constant frozen into the *query* branch
    was a constant it could never reach. Defect Rx45 was caught by three tests
    and missed by that one. This is the module-30 lesson in its sharpest form
    yet: the test read as complete, the module had thirty-seven passing tests,
    and the hole was found by writing a defect rather than by reading anything.
    **A branch the fixture does not render is a branch the test does not
    check** — the same shape as `default_apps`'s four fixture traps, but here it
    was the *test's* choice of state rather than the data's.
  - **The module where the accent and a category hue are the same pixel.** The
    stock accent *is* `blue`, and `Category::Application` is also blue — so
    before the conversion one constant `BLUE` served two unrelated meanings at
    four sites (the caret, the selection bar, every App icon and every App
    badge) and nothing in the module could tell "where you are" from "what this
    is". That is not a hypothetical: an assertion written at the stock accent
    cannot fail, because the two answers agree. Every accent claim here
    therefore runs against an off-palette magenta accent, which is what makes
    the caret follow it while an App badge holds still. Defect Ux45 — the caret
    written as `p.blue` — is the exact edit the pre-conversion code could not
    distinguish from correct, and it is caught only because the fixture moved
    the accent off blue.
  - **A permutation of a set is invisible to a set-membership check, again.**
    The five category hues had a distinctness test and a test that each badge
    matched `Category::color`. Both pass under a *rotation* of all five hues:
    five distinct colours are still five distinct colours, and asking
    `Category::color` what a badge should be is asking the code under test what
    it meant. Only a table naming each category's role — App/blue, Sys/red,
    Set/peach, File/green, Cmd/mauve — can fail. This is the module-29 lesson
    arriving in a module that already had two tests over the same five values.
  - **Two constants compared only against themselves.** `DIALOG_ALPHA` and
    `BADGE_WASH_ALPHA` were each asserted as `drawn.a == THE_CONSTANT`, which is
    a tautology the moment the constant moves. Both now carry a bound as well:
    the wash must be `<= 64` (a tint, not a fill — at 200 the label is read
    against its own hue rather than against the row) and the dialog `>= 224`
    (translucent enough to float, solid enough that the wallpaper does not read
    through the results). Defects Fx45 and DDx46 exist precisely to prove those
    two bounds do something, and before they were added both escaped.
  - **The sweep runs over the shipped app database as well as the fixture.**
    The five-row fixture was built to reach every category, and a fixture built
    to reach everything is exactly the thing that cannot notice a row shape only
    the real data produces. Sweeping `builtin_app_database()` too costs one line
    and is the only check here that sees production data.
  - **One more shadow consolidated.** This was the third of the three popups
    that each picked their own drop-shadow alpha (100 here, against 120 and 160
    elsewhere); it now reads `p.shadow()`, and the test asserts the shadow is
    black in *both* modes rather than equal to any role, because a shadow is an
    absence of light and must not flip with the theme.
  - Judgements, for the record: the accent marks where you are and never what a
    thing is (exactly two accented marks on screen — the caret and the selected
    row's bar); five categories are five distinct hues in either mode; a badge's
    wash is its own hue at a lower alpha, derived rather than named, so adding a
    category cannot leave a stale wash behind; and the dialog floats, which is
    one byte of alpha nothing else in the module would have noticed.
- [x] `resmon.rs` — 11 constants over 16 colour sites, done 2026-08-23.
  Fifty-five tests in the module (nine new), harness defects Ax48–Ix49
  (thirty-five), all thirty-five caught, none escaped, none under-caught.
  - **The first module in the shell that draws no accent at all, and the claim
    is a count of zero.** There is nothing here to select, nothing to invite
    and nothing in force — only quantities that have to be told apart. That is
    worth stating because it is *falsifiable*: the test asserts the accent
    appears in exactly zero commands, in both modes and both display modes, and
    four defects exist only to trip it. It is also worth stating because it is
    worth **nothing at the shipped theme**: the stock accent *is* `blue`, and
    `Cpu` is blue, so under a default install "this module draws no accent" and
    "every CPU line is accented" produce identical pixels and the count reads 40
    rather than 0 for a reason that has nothing to do with position. Without the
    off-palette magenta fixture this test would assert the opposite of what it
    says. Same trap as `launcher.rs`, reached from the other direction: there
    the accent and a category hue collided, here the accent and a *measurement*
    hue do.
  - **A permutation, and the one module where nobody could ever see it.**
    `test_resource_type_colors_distinct` already walked the six hues pairwise,
    and six distinct hues rotated by one are still six distinct hues — so the
    whole widget can be drawn in its neighbours' colours with every pre-existing
    test green. Only a table naming each pair (CPU/blue, RAM/green, Disk/peach,
    Net/mauve, GPU/lavender, Temp/red) can fail, and the proof confirms it:
    defect Cx48 rotates the four graphed hues and is caught **by that table
    alone**, 1 test out of 55. And note what "nobody could see it" means here —
    `resmon` is declared in `lib.rs` but nothing constructs a `ResourceMonitor`
    (`TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`), so there is no
    screen to check against. Four distinct colours in the wrong order render as
    a perfectly plausible graph; "it looked fine" would have certified it.
  - **The pinning table is the only thing that covers what is never drawn.**
    GPU and temperature are collected and given hues but not plotted, so the
    membership sweep and the deleted-constant sweep — both of which read *drawn*
    commands — are structurally blind to them. Defect Fx48 freezes the
    temperature hue to Mocha red and is caught by one test. A sweep over render
    output cannot check a value that never reaches render output, which is an
    obvious sentence that took a defect to notice.
  - **The compact strip's sparkline hues were untested, and writing a defect is
    what found it.** The strip plots the same four metrics from a *different*
    call site with its own hue lookup, and it carries no labels at all — so a
    compact sparkline in the wrong metric's colour was unreachable by every
    test in the module: membership passes (a wrong role is still a role), the
    accent count passes, and the per-site table never looks at compact lines.
    Fixed at the root before the run; defect Lx48 (every compact sparkline drawn
    in the CPU hue) now proves it, caught by that test alone. The module-32
    lesson again — a branch the fixture does not render is a branch the test
    does not check — but here it was a whole *display mode*, not a state.
  - **A pin wearing a relationship's docstring, found by four under-declared
    catches.** `a_metric_is_one_colour_wherever_it_appears` says it asserts only
    that a metric's label and its graph agree, never which colour they agree on;
    it then ended `assert!(bars.all(|c| *c == p.blue))`, naming a role outright,
    so every hue-map defect tripped it. **A test that quietly does two jobs
    cannot be read as a coverage claim about either** — and this is the first
    time the `[UNDECLARED:]` half of the harness output, rather than
    `[MISSING:]`, is what found the problem. Over-catching is not harmless: it
    is how a coverage report flatters itself. The pin belongs to
    `each_measurement_is_pinned_to_the_role_it_names`, which caught all four
    unaided.
  - **`render_bar_graph` is a primitive this module never calls**, so judgement
    4's original "label, sparkline and bars are one colour said three times"
    overstated what the code does — the module draws two of those three. The
    doc now says twice and says why: the primitive's hue is the caller's
    business and not a claim this module gets to make. The bar claim survives
    as its own test, restated honestly as "the primitive substitutes no default
    for the colour it is handed", and is handed `p.accent` precisely because
    nothing here draws the accent.
  - Judgements, for the record: a hue names a *measurement*, never a state or a
    position; six measurements are six distinct colours each pinned by name; the
    grid is furniture, checked against all six metric hues rather than the four
    currently plotted so that graphing GPU later cannot quietly make it
    ambiguous; and a metric's label and sparkline are one colour said twice,
    derived at each site rather than named beside it.
- [x] `mouse_settings.rs` — 10 constants over 14 colour sites, done 2026-08-23.
  Twenty-three tests in the module (nine new), harness defects Ax50–Kx51
  (thirty-seven), all thirty-seven caught, none escaped, none under-caught,
  none under-declared.
  - **The first module in the conversion to come back clean on the first run**,
    and the reason is that the last two modules' lessons were applied *before*
    writing the tests rather than discovered by writing the defects. Module 32
    taught that a branch the fixture does not render is a branch the test does
    not check, so all three state-dependent sites here — the header background,
    the heading ink and the toggle pill — are rendered both ways at every
    section from the start. Module 33 taught that a test doing two jobs cannot
    be read as a coverage claim about either, so each test here makes exactly
    one claim and every *pin* lives in the pinning table. That is worth
    recording because it is the first evidence the method is transferable
    rather than a sequence of one-off saves.
  - **Seventeen of the thirty-seven defects are invisible to both sweeps.**
    Membership caught 20 and the deleted-constant list caught 20 — the same 20,
    every one of them a leftover Mocha literal. Every *permutation* defect (a
    legal role in the wrong place) passes both, and six of them are caught by
    `every_site_draws_the_role_it_claims` **alone**: the panel drawn a rung up
    on `surface0`, the title at a label's dimness, the label and value swapped,
    the switch label at a value's brightness, and the pill with its branches
    collapsed so every switch looks on. n source sites still need n assertions;
    nothing else has ever caught these.
  - **The one defect the shipped theme would hide, stated as its own defect.**
    The open section's heading pinned to `p.blue` rather than the accent is
    legal (blue is a role), survives both sweeps, and is caught by four tests —
    *all four of which would pass under the stock palette*, because the stock
    accent **is** blue. The off-palette magenta fixture is the only reason any
    of them can see it. Third module in a row where the accent/role collision
    is the load-bearing part of the fixture rather than a detail of it.
  - **The thumb is asserted from both sides, and the second side is the
    load-bearing one.** `emphasized(p.accent)` replaces a `LAVENDER` that sat
    beside a `BLUE` fill with nothing connecting them — the same shape as
    `launcher.rs`'s two self-comparing alphas. The test asserts both that the
    handle *is* the derivation and that it equals **no role at all**, across
    three different accents. The first half alone would pass a future edit that
    pinned the thumb to whichever role happened to match under one accent; the
    second half is also what makes the membership sweep's `derived` list honest,
    since a derived colour that turned out to be a role would be waved through
    by `Palette::roles()` without the declaration doing any work. Defect
    "thumb is `emphasized(p.blue)`" is caught by the sweep for exactly that
    reason: (107, 139, 194) is in neither palette and is not what was declared.
  - **A count is what makes "the accent means one thing" falsifiable.** The
    pinning table names fourteen sites and by construction cannot notice a
    fifteenth, and "the accent is used tastefully" is not a property a command
    list carries. So judgement 1 is a table of exact counts per (section,
    switches) — one heading plus one fill per slider on show — written out by
    hand rather than derived from the same `if` the renderer uses, which would
    have made the test agree with any answer. It catches eleven defects,
    including every one that *adds* an accent: the banner, a switched-on pill,
    the track along its whole length, the thumb, and the panel background.
  - **The retheme test is narrow, and that is why it could be declared
    precisely.** `only_what_is_in_force_moves_when_the_accent_moves` renders the
    whole panel under two different accents and asks which commands differ —
    the question a user asks by changing their accent and glancing at the
    screen. It caught seven defects and, correctly, said nothing about the ten
    that swap one fixed role for another: those render identically under both
    accents. A whole-list test that fired on everything would have been an
    under-declared mess; this one fires only on sites that gain or lose their
    dependence on the accent.
  - **The low-contrast knob is left alone on purpose.** `text` on a `green`
    pill is two light values —
    `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`, being fixed across the
    shell in one pass so every switch reaches the same answer. What *is*
    asserted here is the separable half: the knob is the same ink on both
    pills, because it marks a position and a position does not change meaning
    when the state does. The docstring names the debt so the next reader does
    not mistake a deferred fix for an unnoticed one.
  - Judgements, for the record: the accent says what is in force and says
    nothing else; a state is not a position, so the on-pill and the unsaved
    banner keep named hues that survive a retheme; the thumb is derived from
    its fill rather than named beside it; and three sites choose by state, so
    every test renders both branches.
- [x] `hotkeys.rs` — 10 constants over 12 colour sites, done 2026-08-23.
  Sixty-eight tests in the module (seven new), harness defects Ax52–Tx53
  (forty-six): 44 caught on the first run, the other two never actually
  introduced — see the last bullet — and caught once the harness was fixed.
  - **The first module where a constant turned out to be a *setting*.** The
    panel background was `Color::rgba(30, 30, 46, 240)` — Mocha `base` with an
    alpha soldered onto it. That is not a colour someone forgot to convert; it
    is the transparency setting, frozen. It was wrong in both directions at
    once: a user who had turned transparency **off** still saw the wallpaper
    through their hotkey list, and one who had turned it up to Full got a panel
    noticeably more solid than every neighbouring popup on the same screen.
    `panel_bg()` is `base` at the palette's own `panel_alpha`, and the test
    renders at three of them and reads the alpha back.
    - The same test asserts the shadow does **not** move, and that half is the
      load-bearing one. A fix that made *everything* follow `panel_alpha` would
      be as wrong as the constant was — a shadow is an absence of light and does
      not thin out when the thing casting it does. Without that clause, "the
      panel follows the setting" is satisfied by a renderer that dissolves the
      whole panel including its shadow.
    - **Two defects exist only because of this judgement, and each is caught by
      exactly one test.** `panel_bg()` reverting to plain `p.base` is invisible
      to the membership sweep (base *is* a role), invisible to the deleted-
      constant list, and invisible to the pinning table — because at
      `Palette::for_mode`, `panel_alpha` is 255 and `p.base == p.panel_bg()`.
      Only the transparency test, which sets the field to 200 and 160, can see
      it. Likewise the alpha-refreezing defect
      (`Color::rgba(p.base.r, p.base.g, p.base.b, 240)`) reads the *right* role
      and still reintroduces the whole bug. A conversion audited only by "is
      every colour a role?" would have shipped both.
  - **Nothing on this panel is accented, and that had to be asserted as a
    count.** A hotkey card is *read*, not operated: the selected row marks where
    the user is looking, not what is in force. So the selection moves a rung
    (`surface0` over the panel, ink from `subtext1` up to `text`) rather than
    changing hue — which also matches `launcher.rs`, the shell's other
    keyboard-driven list, where the surface fills the row and the accent is
    spent on a separate marker bar. "Nothing is accented" is not something a
    pinning table can check, because a table names *n* sites and by construction
    cannot notice the *n+1*th; only a count over the whole render can. Ten
    defects paint the accent onto a site that should not have it, and the count
    is what sees them — and it only means anything because the fixture's accent
    is off-palette. Under the shipped theme the accent **is** `blue`, so every
    one of those ten would have been indistinguishable from a legal role.
  - **The selection is said twice, and a test that checked one saying would
    certify the other.** A fill appears *and* the label brightens; either alone
    is a one-bit signal that a low-contrast display or poor colour
    discrimination can lose. Six defects collapse, invert or misplace one of the
    two sayings, and the one that moves the highlight a row below its own label
    is caught by **that test alone** — the pinning table reads colours, not
    coordinates, so an off-by-one between the highlight's `y` and the label's is
    exactly the failure a colour-only conversion audit cannot see.
  - **Judgement 4 is stated as relations rather than literals, because the
    literals are what the conversion is deleting.** "The badge is `#181825`" is
    true in Mocha and false in Latte, so it is not the claim worth testing. What
    holds in both modes is that a badge is *visibly a badge*: a different colour
    from the card it sits on, outlined in something different again, and
    lettered more quietly than the heading. "Quieter" had to be defined as
    *distance from the panel* rather than absolute darkness — in Latte the
    dimmer ink is the **lighter** one, so a luma comparison written the obvious
    way passes in Mocha and inverts in Latte. That test fires on fourteen of the
    forty-six defects, including a badge painted the same colour as the card and
    a badge lettered as loudly as the heading.
  - **Every catcher list was predicted exactly** — of the 44 defects the run
    actually introduced, not one was caught by a test that had not declared it
    and not one escaped a test that had, including the badge-contrast
    predictions, which needed the Latte luma of every role worked out by hand
    first. The lessons applied before writing a line of test code: render every
    branch the renderer selects a colour in (four here: selection on/off crossed
    with the empty and default registries); make each test one claim so its
    catcher list is a coverage statement; use an off-palette accent so an accent
    assertion is falsifiable; and index anything positional by index rather than
    counting it.
  - **But two defects escaped, and the fault was the harness's, not the
    suite's** — which is the more useful finding of the two. `reintro-palette.py`
    applies each edit with `str.replace(old, new, 1)`, which takes the *first*
    match. Both escapes were role *swaps* written as two edits, and in both the
    first edit manufactured a fresh copy of the string the second edit was
    looking for, at a lower file offset. The second edit landed on that copy and
    undid the first. The pair was a no-op; the file was byte-identical to the
    original; the suite was run against unmodified source and — correctly —
    passed.
    - The harness reported that as `*** NO TEST FAILED ***`, which is not
      merely unhelpful but the **exact opposite** of the truth. It reads as "the
      suite has a hole here", when what happened is "the suite was never asked
      anything". A tool whose entire purpose is to stop a test being trusted on
      faith must not itself convert an unasked question into an unanswered one,
      because that verdict costs a full re-run of the suite to disbelieve.
    - Fixed in three places rather than by re-anchoring the two defects and
      moving on. `--check` now compares the patched text against the snapshot
      and reports `NO-OP`, so an authoring mistake of this shape surfaces in the
      seconds-long preflight instead of after the twenty-five-minute run it
      invalidates. The run itself refuses to write a file it did not change and
      names the failure `*** PATCH IS A NO-OP ***`. And the summary line counts
      these in their own column — `n never introduced` — because folding them
      into `caught` would inflate the sweep with defects that never existed,
      and folding them into `escaped` blames the tests for the harness's error.
    - The general lesson, which applies to every remaining module: **a
      multi-edit defect can undo itself, and the way to prevent it is to anchor
      each edit on a neighbouring line the other edit cannot forge.** Both were
      re-anchored that way (on `font_size: KEY_FONT_SIZE,` and on
      `corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),`), after which both are
      caught by the tests that declared them.
- [x] `screen_capture.rs` — 10 constants over 22 colour sites in two public
  renderers, done 2026-08-23. Fifty-eight tests in the module (eight new),
  harness defects Ax54–Vx55 (forty-eight). Forty-four caught on the first run
  with **zero under-caught and zero under-declared** — every catcher list
  predicted exactly, for the second module running — and the other four never
  compiled, which was a bug in the defect generator rather than in the suite;
  see lesson 19 below and the 48/48 line at the end of this entry.
  - **The catcher census**, which is the part worth keeping: the ordered pin
    table `every_site_draws_the_role_it_claims` caught 42 of 48 and was the
    sole catcher of 14; `nothing_in_the_recorder_is_accented`,
    `none_of_the_ten_deleted_constants_is_still_drawn` and the two-mode sweep
    `every_colour_both_renderers_draw_comes_from_their_palette` caught 9 each
    and were sole catcher of none; `the_recording_dot_says_the_state_and_only_
    the_state`, `the_indicator_pill_is_as_transparent_as_the_user_asked` and
    `a_transport_button_is_lettered_for_its_own_fill` caught 5 each, with the
    transparency test alone on 2. The pre-existing `test_indicator_idle_empty`
    caught 1, which is the only evidence in this sweep that the module had any
    colour coverage before the conversion.
  - **The only module so far whose conversion fixes a bug a user could already
    see.** The Record, Pause and Resume buttons were lettered `MOCHA_BASE` — a
    near-black — which is legible on Mocha's *pale* red, yellow and green and
    illegible on the light theme's, where all three of those fills are dark.
    The lumas are 78, 104 and 91 against `readable_on`'s threshold of 140, so
    in Latte all three want near-**white** ink and were getting near-black on
    dark. Naming the ink beside the fill is what allowed the two to disagree;
    `readable_on(fill)` cannot.
    - The test pins the endpoints **by hand** — near-black in dark mode,
      near-white in light — rather than calling `readable_on`. A test that
      called the same function the renderer calls would agree with it however
      wrong both were, which is the tautology lesson from module 32 applied
      before writing the test instead of after.
    - It also asserts the two modes' inks *differ*. That is the claim a pinned
      constant provably cannot satisfy, and it is what distinguishes "the ink
      is computed" from "the ink happens to be right in the mode I tested".
  - **The frozen-transparency finding from `hotkeys.rs` recurred verbatim**, in
    a module written by a different hand: the indicator pill was
    `Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 220)`. Two
    occurrences in two consecutive modules is enough to expect it in the
    remaining thirteen, so it is now a thing to look for rather than a thing to
    discover — grep for `rgba(` in a module's constant block before converting
    it.
    - Two of the forty-eight defects are single-catcher because of this, and
      both are invisible to the membership sweep, the deleted-constant list
      **and** the pin table at once: the pill reverting to plain `p.base`, and
      the recording dot fading with `panel_alpha`. `Palette::for_mode` always
      sets `panel_alpha` to 255, so at the default palette
      `p.base == p.panel_bg()` and every table-shaped test agrees with the bug.
      Only a test that *varies* the setting sees it.
    - The third of the three, re-freezing an alpha onto the *right* role, was
      caught by the pin table too — and the distinction is worth keeping,
      because it is the line between what a table can and cannot do. A table
      compares `Color`s including alpha, so it sees a value that is wrong at
      the default setting; what it cannot see is a value that is *right* at
      the default setting and wrong at every other. Dropping the setting is
      exactly that, which is why it needs a test that changes the setting and
      why predicting it as single-catcher was right for one defect out of the
      two I expected.
    - The dot clause is the load-bearing half. A fix that made everything
      follow `panel_alpha` would satisfy "the pill answers the setting" and
      still be wrong — a recording indicator that fades until it cannot be seen
      has failed at the one thing it is for.
  - **The transport colours are a code, not a theme, and the count is what
    proves it.** Red records, yellow pauses, green resumes, grey means neither;
    a user reads those the way they read a traffic light, so they keep named
    roles across a retheme rather than following the accent. Ten defects paint
    the accent onto a site, and the count over the whole render is what sees
    them — a pin table names *n* sites and by construction cannot notice the
    *n+1*th. As always it only means anything because the fixture's accent is
    off-palette; under the shipped theme the accent **is** `blue`.
  - **The pin table compares the ordered vector, not the set.** Eight defects
    are two sites trading roles, which leaves the multiset of colours drawn
    byte-identical — the module-29 lesson, and the reason the expected value is
    a `Vec<Color>` in draw order per state per mode rather than a membership
    table.
  - **Six states, and four of them draw no indicator at all**, while three
    reach only the controls panel's fallback arm. Every colour test therefore
    iterates the states explicitly; a test that rendered "the recorder" would
    have been exercising two states out of six and reporting on all of them.
    That is module 32's unrendered-branch lesson and module 33's whole-display-
    mode lesson, applied before the tests were written rather than after a
    proof run found the hole.
  - **Lesson 19: `--check` proves an anchor is *findable*, not that the patched
    file is Rust.** The preflight compares patterns against the unmodified
    snapshot and reports `PATTERN NOT FOUND` / `AMBIGUOUS` / `NO-OP`. All
    forty-eight passed it — `1171 defects, 0 stale, 0 ambiguous, 0 no-op` — and
    four of them then failed to compile.
    - The cause was in the generator, not the harness. Its substitution helper
      assumed the `color:` line is the *last* line of an anchor, because in
      twenty-three of the twenty-four anchors it is. The controls bar's anchor
      put it first and used `corner_radii: CornerRadii::all(8.0),` as the
      disambiguating tail, so the helper rewrote the `corner_radii` line and
      the struct literal ended up with two `color:` fields —
      `error[E0062]: field 'color' specified more than once`. All four affected
      defects were the four built on that one anchor.
    - Fixed by re-anchoring on the two comment lines *above* the colour instead
      of the `corner_radii` line below it, restoring the colour-last shape the
      generator assumes. That is the general rule now: **an anchor's last line
      is the line being replaced**, and any disambiguation goes above it.
    - The reporting was wrong too, and in the same way the no-op reporting was
      wrong before module 35 fixed it: `DID NOT COMPILE` was tallied under
      `escaped`. A patch that fails to build never reaches a test binary, so no
      test had the opportunity to fail — calling that an escape blames the
      suite for the harness's own authoring error. It now sits with
      `PATCH IS A NO-OP` and `PATTERN NOT FOUND` in a column renamed from
      `never introduced` to `never asked`, which is the property the three
      actually share.
    - The pattern across lessons 18 and 19 is one thing said twice: **the
      preflight can only check the property it models.** It models "does this
      pattern occur exactly once in the original file", which catches a stale
      anchor and, since module 35, a self-cancelling edit pair — but says
      nothing about whether the *result* parses. A defect that does not compile
      is caught within minutes by the run itself, so the preflight is a way to
      avoid wasting an hour, not a correctness gate; treating it as the latter
      is what made four failures a surprise.
    - Re-run after the fix: **4 caught, 0 escaped, 0 never asked, 0
      under-caught, 0 under-declared**, and each of the four was caught by
      exactly the tests it declared — so module 36 closes at **48/48**. The
      catcher census above is unchanged by it: the four add three more to the
      pin table, one each to the two-mode sweep, the deleted-constant list, the
      accent count and the transparency test.
- [x] `snap.rs` — 10 constants over 16 colour sites in three public renderers,
  done 2026-08-23. Thirty-six tests in the module (eight new), harness defects
  Ax56–Yx57 (fifty-one).
  - **This module is where the palette's own numbers came from, which makes it
    the one module that could be converted by accident.** `mod theme` here held
    `ZONE_FILL` at alpha 50, `ZONE_HIGHLIGHT` at 90 and `ZONE_BORDER` at 160,
    and `appearance::wash` ships `FILL = 50`, `HIGHLIGHT = 90`, `EDGE = 150` —
    `Palette::selection_border`'s doc comment names snap zones as its caller.
    So three of the ten constants were *already* the palette rung, written out
    by hand, and one (160 against 150) was the copy having drifted ten units
    from the original. The conversion closes that drift rather than preserving
    it: a copy that agrees is still a copy, and this one had already begun to
    disagree.
  - **Five judgements, recorded in the module's own `# Colour` doc section**,
    because each is a place where "read it from the palette" does not by itself
    say *which* role:
    1. **A snap zone follows the accent.** It is a selection — the thing the
       drop will land in — so it moves with the user's accent, and the three
       rungs become `selection_fill`, `selection_border` and `highlight_fill`.
       This is the **opposite** answer from `screen_capture`'s transport
       buttons one module earlier, and deliberately so: red-records is a code a
       user decodes, a highlighted drop target is a selection a user points at.
       The distinction is "does the colour *mean* something specific" versus
       "does this colour mean *this one is chosen*".
    2. **The zone labels are lettered for the scrim, not for the mode.** They
       sit on `p.scrim()`, which is black in both modes on purpose (§525
       decision 3), so `readable_on(p.scrim())` is the correct ink and `p.text`
       is the plausible wrong one — it would put dark grey on near-black under
       the light theme. The test pins the near-white endpoint by hand and
       asserts `ink != p.text`, which is module 32's tautology lesson and
       module 36's endpoint-pinning practice applied together.
    3. **The picker is a panel and obeys the transparency setting.** Its
       background was frozen at `rgba(30, 30, 46, 230)` and its hover at
       `rgba(69, 71, 90, 200)` — the frozen-transparency finding for the
       **third** consecutive module, after `hotkeys.rs` and `screen_capture.rs`.
       Both become `panel_bg()` / `panel_hover()`.
    4. **The scrim was tinted and should not have been.** `OVERLAY_SCRIM` was
       `rgba(30, 30, 46, 140)` — Mocha `base` at the scrim alpha — so in light
       mode the backdrop behind the zone grid would have *lightened* the
       desktop instead of pushing it back. `p.scrim()` is `rgba(0, 0, 0, 140)`
       and does the job in both modes. This is the same class of bug as module
       36's near-black lettering: a value that is right in the palette it was
       written in and wrong in the other one, invisible until the other one
       exists.
    5. **A hue against a rung, never a hue against a hue.** The picker's active
       preset is `p.accent` and its inactive presets are `p.overlay0` — a grey
       rung, not `p.lavender`. `AccentColor` offers Lavender, so an inactive
       marker in lavender would be *identical* to an active one for any user
       who picked that accent. The test iterates all fourteen named accents in
       both modes and asserts the two are present and distinct; a fixture with
       a single off-palette accent would pass while the shipped Lavender broke.
  - The picker's drop shadow stays `rgba(0, 0, 0, 100)` rather than
    `p.shadow()`'s 120, and that is a deliberate keep: the picker is a small
    popup, not a window, and the lighter shadow is a size judgement rather than
    a copy of the palette. It is declared in the module docs so the next reader
    does not "fix" it.
  - **The transparency test pins what must *not* follow the setting**, not only
    what must. Sweeping `panel_alpha` across 255 / 200 / 160 and asserting the
    background and hover track it is half the claim; the other half is that the
    shadow (`rgba(0,0,0,100)`) and the scrim (`rgba(0,0,0,140)`) do **not**.
    Module 36's dot clause is the same shape — a fix that made everything
    follow the setting would satisfy the first half and be wrong.
  - **Lesson 19 is now closed in the harness rather than in a habit.**
    `scripts/reintro-palette.py` grew a third mode, `--compile [names…]`, which
    applies each selected defect, runs `cargo check -p <pkg> --all-targets`,
    restores, and reports `builds` / `DOES NOT COMPILE` / `NOT APPLIED`. That
    is precisely the question `--check` cannot answer — module 36's four broken
    defects passed `--check` and were found an hour into the run they had
    already invalidated. Minutes instead of an hour, and it is a preflight for
    every future module rather than a rule someone has to remember.
    - `--all-targets` and not a bare `cargo check`: several defects reinstate a
      constant whose only remaining reader is the test module, so checking the
      lib alone would miss exactly the errors these defects cause.
    - Applying a defect is now one function, `apply_to`, shared by all three
      modes — so what a preflight vets is literally what the run runs, rather
      than a second implementation that can drift from it. It returns the
      *reason* a defect could not be introduced, which keeps `PATTERN NOT
      FOUND` and `PATCH IS A NO-OP` reported apart: a missing pattern is source
      that moved under the defect, a no-op is the defect arguing with itself.
    - `--compile` restores after **each** defect rather than at the end, and
      still runs under the same SHA-256-verified whole-snapshot rewrite in a
      `finally`. An interrupted preflight therefore leaves at most one file
      patched, and that one gets put back too.
    - It is a preflight, not a gate. The real run still detects a broken defect
      (`DID NOT COMPILE`); what this buys is learning it before the run whose
      result it would spoil has been started.
    - First use, on this module's fifty-one: **51 build, 0 do not, 0 not
      applied**, tree restored byte-clean, in a fraction of the sweep's time.
  - **The catcher census.** The ordered pin table
    `every_site_draws_the_role_it_claims` caught 48 of 51 and was sole catcher
    of 13; `the_picker_is_as_transparent_as_the_user_asked` 15 and sole on 2;
    the two-mode sweep
    `every_colour_all_three_renderers_draw_comes_from_their_palette` 13;
    `none_of_the_ten_deleted_constants_is_still_drawn` 12;
    `the_zone_under_the_cursor_out_reads_the_zones_at_rest` and
    `a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode` 6 each;
    `the_scrim_is_black_in_both_modes` 5;
    `an_inactive_preset_is_never_the_accent_the_user_chose` 4 — the last five
    sole catcher of nothing. **No pre-existing test in the module caught a
    single defect**, which is the plainest statement available that a module
    with thirty-six tests can have no colour coverage whatever.
  - **Lesson 20: an off-palette accent is the right fixture for "did this
    follow the accent" and the wrong one for anything that is a *function of*
    the accent.** One defect escaped all fifty-one — the hovered zone's label
    lettered `readable_on(p.accent)` instead of `readable_on(p.scrim())` — and
    the reason is arithmetic, not oversight. `readable_on` is a step function
    with its threshold at luma 140; the fixture accent `#FF00FF` has luma 105,
    so it falls on the *same* side as the black scrim and the wrong expression
    returns the right answer. Every colour test in the module used that one
    accent, so all of them agreed with the bug.
    - It is a real bug and not a hypothetical: Yellow, Peach, Rosewater and
      Flamingo are all above the threshold, so a user on any of those four
      would get near-black labels on a black scrim — invisible.
    - The rule that follows is sharper than "vary the fixture". A fixture
      value is a *sample*, and one sample characterises a function only if the
      function is constant. `readable_on` is a step, judgement 5's hue-vs-hue
      collision is an equality — both have exactly one interesting input among
      the fourteen, and a spot check finds it with probability 1/14 and 4/14
      respectively. Where the property under test is a function of the accent,
      **walk the fourteen**; where it is "is this site accented at all", the
      single off-palette magenta remains correct and cheaper.
    - Fixed by walking `OFFERED` in the label test as
      `an_inactive_preset_is_never_the_accent_the_user_chose` already did, and
      by lifting the fourteen-accent table and a `wearing(light, accent)`
      helper to the top of the test module so the next test that needs a
      sweep does not re-derive one.
    - Note which test the fix belongs in. The pin table cannot be made to
      catch this: it compares a vector of colours against expectations built
      from the same palette, so it would have to walk the fourteen accents
      *and* recompute the expected ink per accent — which is the tautology of
      module 32. The claim "the ink is the scrim's, whatever the accent" is a
      different claim from "each site draws the role it claims", and it needs
      its own test.
  - **Five declarations were wrong in two directions**, after two consecutive
    modules of predicting every catcher exactly. Two were under-caught: a
    fully-opaque `MOCHA_BLUE` at a *border* slot does not trip the
    hovered-out-reads-resting test, because that test compares alphas and 255
    beats 150; and two inks trading places is invisible to the membership
    sweep, because both inks are members. Three were under-declared: the
    transparency test pins both rungs the picker's background and hover traded,
    the scrim test pins the scrim's alpha so a setting-driven one trips it, and
    climbing to the active marker's rung is precisely what the inactive-preset
    test forbids. All five are now reconciled in the harness — the declarations
    are the only record of what each test proves, and a wrong one is a
    misleading record rather than a harmless one.
    - Two modules of perfect prediction did not mean the method had stopped
      needing the run. What it meant is that modules 35 and 36 were shaped like
      the ones before them; this one has three renderers, a threshold function
      and a scrim, and the predictions went wrong in both directions the first
      time the shape changed.
  - **Re-run of the six after the fix and the reconciliation: 6 caught, 0
    escaped, 0 under-caught, 0 under-declared**, and the escaped defect is now
    caught by exactly one test — the accent-walking label test, which is the
    only test that can see it. Module 37 closes at **51/51**. The census above
    gains one to that test, taking it to 7 catches and its first sole catch.

- [x] `display_settings.rs` — 9 constants over 23 colour sites, done
  2026-08-23. Sixty-one tests in the module (eight new), harness defects
  Ax58–Zx59 (fifty-two).
  - **This is the first module where "read the colour from the palette" is the
    wrong instruction for part of the file.** Three things here are
    *instruments*, not decoration, and a settings page that themes its own
    measuring equipment is lying to the user about what their display does:
    1. **The five test patterns.** A sixteen-step grey ramp, eight SMPTE bars,
       a twenty-four-step hue sweep, a black-and-white checkerboard and
       `rgb(128, 128, 128)`. Their entire purpose is being *exact* and the same
       on every machine in every theme. `TestPattern::render` therefore takes
       no `&Palette` at all — the protection is in the signature, so the
       mistake cannot be made by a future edit rather than merely not having
       been made by this one.
    2. **The night-light preview swatch**, which shows what the screen will
       physically look like at the chosen colour temperature. It follows
       `ColorTemperature::preview_color`, not the theme.
    3. **The Red, Green and Blue gamma rows.** These are coloured because they
       *are* the red, green and blue channels; the colour is a label, not a
       style. They take `p.red` / `p.green` / `p.blue`, which shift between
       modes for legibility but never follow the accent — the same answer as
       `screen_capture`'s transport buttons two modules earlier, and for the
       same reason: a colour that a user *decodes* is not a colour that says
       "this one is chosen".
  - **The deleted `MOCHA_BLUE` was two different things, and the shipped theme
    is what hid that.** It coloured the active tab's label, the filled part of
    a slider and the selected pattern chip — all of which mean "chosen" and
    become `p.accent` — *and* the Blue Gamma row, which means "the blue
    channel" and becomes `p.blue`. The stock accent **is** blue, so the two
    were the same pixels and the file had no way to say which it meant. This
    is the sharpest instance yet of the defect the whole task is about: a
    single copied constant does not merely duplicate a value, it **merges two
    concepts**, and the merge is invisible for exactly as long as nobody
    changes the accent.
  - **The selected chip's lettering was near-black and had to stop being.**
    `MOCHA_MANTLE` on Mocha's pale blue is legible; on Latte's `#1D62EC`
    (luma 93) it is not. The ink is `p.on_accent()`, computed from the fill
    rather than named beside it — module 36's finding, now the standing
    practice.
  - **Two of the new tests locate their subject structurally rather than by
    index, and the first draft proves why.** The swatch was `cols[4]` and the
    chip fill was `chip[7]`; run against the real command stream, the first
    pointed at the active tab's label and the second at the "Test Patterns"
    heading. An index into a render is a claim about *layout*, and this module
    is not about layout — so the swatch is now found as **the only colour on
    the page that belongs to no palette** (which is also precisely the property
    being asserted), and a chip as **the 32-pixel fill plus the `Text`
    immediately after it**, which additionally checks all five chips instead of
    the one at a fixed offset.
  - **Lesson 21: an off-palette fixture must be off the *instruments* too, not
    only off the palette.** The obvious accent for a module full of colour is
    magenta, and it is the one this module must not use: the SMPTE bars *are*
    magenta, so counting "how many accents does this tab draw" counted a
    calibration target as chrome and reported three where one was correct.
    The fixture is now `#C828A0`, chosen by a property rather than by taste —
    no channel at 0 or 255 and not a grey, which is exactly what puts it
    outside every pattern, since the bars are combinations of 0 and 255, the
    ramp and the board are greys, and the hue sweep is fully saturated at every
    step. `accented()` asserts both non-collisions, so the reasoning is in the
    code and not only here.
    - The general form: **a test fixture must be disjoint from everything the
      module draws that the fixture is not**, and "everything the module draws"
      now includes things the module is forbidden to theme. Twenty modules of
      "pick something not in the palette" was a sufficient rule only because no
      earlier module drew anything outside the palette on purpose.
  - **Preflight then sweep: 52 build / 0 do not / 0 not applied, then 52
    caught, 0 escaped, 0 never asked, 0 under-caught, 0 under-declared.** The
    first perfect prediction since module 36, and worth saying *why* it came
    back after module 37 broke the streak: the one under-declaration in this
    module's set was found by reading the declarations against the assertions
    before the run rather than by the run — defect H (every tab label accented)
    also trips the gamma-row test, because that test pins the Calibration tab
    at exactly one accent and four accented labels is four. Module 37's lesson
    was that predictions go wrong when a module's shape changes; the response
    is not to predict better but to *check the prediction against the test
    source*, which costs minutes against a sweep that costs half an hour.
  - **The catcher census**, from the fifty-two:
    `every_site_draws_the_role_it_claims` 33 and sole on 6;
    `none_of_the_nine_deleted_constants_is_still_drawn` 26, sole on none;
    `every_colour_the_panel_draws_comes_from_its_palette` 25, sole on none;
    `the_gamma_rows_are_the_channels_and_never_the_accent` 12, sole on none;
    `an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent` 9 and sole on 1;
    `a_selected_pattern_chip_is_lettered_for_its_own_fill` 7 and sole on 3;
    `the_test_patterns_are_the_same_in_both_modes` 5 and **sole on 4**;
    `the_night_light_swatch_shows_the_temperature_not_the_theme` 2 and **sole on
    both**. **No pre-existing test caught anything**, for the fourth
    consecutive module.
    - The shape of that census is the module's argument in one line. The two
      broadest tests — the membership sweep and the deleted-constant table —
      caught 51 defects between them and were sole catcher of **none**, while
      the two narrowest caught 7 and were sole catcher of 6. A sweep that only
      had the broad tests would have reported 47 of 52 and looked excellent.
      What it would have missed is every defect that themes an instrument,
      which is the only class of defect this module has that the others do not:
      the instrument colours are *declared to the sweep as derived*, so the
      sweep is structurally blind to them and the pinning tests are the entire
      coverage. A membership test cannot check a value it was told to accept.

- [x] `accessibility_settings.rs` — 9 constants over 14 colour sites, done
  2026-08-23. Thirty-seven tests in the module (nine new), harness defects
  Ax60–Zx61 (fifty-two).
  - **The near-black-on-accent bug, found for the third time, on the
    accessibility page.** The selected tab was lettered `CRUST` (`#11111B`) on
    `BLUE`, which measures 8.91:1 in Mocha and is excellent. The same pair in
    Latte is near-black on `#1D62EC` and measures **3.58:1** — below the 4.5:1
    floor, on the one page in the shell whose subject is people who cannot read
    low-contrast text. The ink is now `p.on_accent()`, computed from the fill.
    That this recurs module after module is the point: a constant named for a
    colour cannot record that it was chosen *for* another colour, so every copy
    of it silently re-asserts a contrast measurement that was only ever taken
    once, in one theme.
  - **The deleted `GREEN` and `BLUE` are the state/selection split again, and
    here getting it wrong would have broken the widget's meaning outright.**
    `GREEN` appeared at two sites — the "*n* features active" line and the
    toggle switch's on-pill — and both encode **state**, a code the user
    decodes, so both stay `p.green` and do not follow the accent. `BLUE` on the
    tab pill encoded **selection**, which is what the accent is for, so it
    becomes `p.accent`. Rewriting both the same way would have given a user
    with a green accent a settings page on which *every switch reads "on"*.
    Two sites, one deleted constant, two different answers.
  - **Lesson 22: a locator derived from the renderer's own list cannot see a
    permutation of that list.** The one wrong declaration in the set was defect
    `Z`, which reorders `A11yTab::ALL`. Four tests read the tab bar; only two
    caught it. The two that missed walk `A11yTab::ALL.iter().enumerate()` and
    index the render by the same `i`, so permuting the list permutes the
    expectation identically and the defect is invisible to them by
    construction.
    - This is a blind spot *inside* the structural-locator discipline, not a
      lapse from it. That rule says an index into a render is a claim about
      layout, so locate by the property being asserted — and these two tests
      obey it, by deriving the index from the renderer's list rather than
      writing `[3]`. The refinement is that a locator taken from the code under
      test moves *with* the code under test. Only an expectation written out
      independently can see an ordering defect: here the ordered-vector pin
      (lesson 9) and the green/accent test, which reads `tabs(&cmds)[0]`
      outright. Coverage was never in question — the defect was caught twice,
      deterministically — but the declaration is the only record of what each
      test proves, and a wrong one is a misleading record.
  - **Preflight then sweep: 52 build / 0 do not / 0 not applied, then 52
    caught, 0 escaped, 0 never asked, 1 under-caught, 0 under-declared**;
    reconciled and re-run, `Z` now caught by exactly the two tests that can see
    it. The pre-sweep declaration review — module 38's institutionalised
    lesson — again earned its keep in the other direction, catching one
    *under*-declaration by reading: the same `Z` also trips the green/accent
    test, because that test reads `tabs(&cmds)[0]` after choosing Visual, and
    with the order permuted index 0 is the unchosen Input tab.
  - **The catcher census**, from the fifty-two:
    `every_site_draws_the_role_it_claims` 46 and **sole on 8**;
    `green_means_on_and_the_accent_means_chosen` 17, sole on none;
    `none_of_the_nine_deleted_constants_is_still_drawn` 16, sole on none;
    `every_site_changes_when_the_mode_does` 15, sole on none;
    `every_colour_the_panel_draws_comes_from_its_palette` 14, sole on none;
    `exactly_one_tab_is_accented_and_it_is_the_chosen_one` 12, sole on none;
    `the_selected_tab_is_lettered_for_its_own_fill` 11, sole on none;
    `the_active_feature_line_is_green_and_only_drawn_when_there_is_one` 7 and
    sole on 1; `the_section_headings_keep_their_hue_in_both_modes` 7 and **sole
    on 4**. **No pre-existing test caught anything**, for the fifth
    consecutive module.
    - The ordered-vector pin caught 46 of 52 and was sole catcher of 8, which
      is the highest share any single test has reached across all thirty-nine
      modules. That is a property of the *module*, not of the test: fourteen
      colour sites in five tab bodies, almost all of them plain role reads with
      no derived value and no instrument, is precisely the shape an ordered pin
      covers completely. The reading to resist is "the ordered pin is the only
      test worth writing" — it was sole on 8 here and on 6 in module 38, but in
      module 38 the four narrowest tests were sole on 8 between them, and here
      the two narrowest are sole on 5. The broad test finds that *something*
      moved; only the narrow one says the switch stopped meaning "on".

- [x] `focus_assist.rs` — 9 constants over 15 colour sites, done 2026-08-23.
  Forty-one tests in the module (nine new), harness defects Ax62–Zx63
  (fifty-two).
  - **The near-black-on-accent bug again — but this time illegible in all
    three states at once.** The tray pill is the module's only always-visible
    surface, and it was lettered with a hard-coded `BASE` (`#1E1E2E`) on
    whichever hue codes the current mode. In Mocha that is fine. In Latte it
    measures **3.02:1 on blue, 3.12:1 on yellow and 3.13:1 on red** — every
    active mode below the 4.5:1 floor simultaneously, which is worse than the
    single failing pair the previous modules had. The ink is now
    `readable_on(fill)`, which lifts all three to ≈4.85:1.
    - This is the fourth module in which the *same* constant-named-for-a-value
      hid the *same* class of bug, and the fourth time the light render is what
      named it. The bug is not that someone chose a bad colour; it is that
      `BASE` is a name for `#1E1E2E` rather than a name for "the ink that can
      be read on this fill", so the site could not express the thing it
      actually needed.
  - **A severity code is not a theme, so the mode hues do not follow the
    accent.** Blue, yellow and red here mean "a little silence", "more" and
    "total" — three rungs of one scale. A user who sets a green accent gets a
    green *picker*, because that marks a choice, but the rungs stay put: a
    wrong colour is hard to read, whereas a uniform one says nothing at all.
    `the_mode_hues_are_a_severity_code_and_never_the_accent` pins both halves —
    the exact ordered scale, and that no two rungs collide.
  - **The `derived` parameter was not needed, but the fixture had to be an
    instrument.** Every colour this module draws is a plain role, so the
    membership sweep accepts everything with `derived: &[]`. That makes the
    sweep *structurally blind* to any defect that swaps one role for another —
    including "every mode hue follows the accent", the single worst defect the
    module can have. What catches it is the fixture: `accented()` asserts that
    the off-palette accent it installs is equal to no role *and* to no mode
    hue, so the assertion fires inside every one of the twelve tests that use
    it. Lesson 21 (an off-palette fixture must be off the *instruments* too)
    turned out to be the whole coverage for that defect.
  - **A site nothing renders is a site nothing checks** — caught before the
    defects were written, not by them. "No automatic rules configured" is drawn
    only by a manager with *no* rules, and the busy fixture has one, so neither
    the membership sweep, nor the deleted-constant table, nor the ordered pin
    ever reached that line. A fixture rich enough to exercise four picker rows
    and a suppressed-count line is exactly the fixture that silences the
    empty state. Fixed by adding a second, deliberately *empty* render to all
    three, which is also the only render in which `Off` is the chosen row —
    and that second pin is what later caught the `FocusMode::ALL` permutation.
  - **Preflight then sweep: 52 build / 0 do not / 0 not applied, then 52
    caught, 0 escaped, 0 never asked, 0 under-caught, 0 under-declared** —
    clean on the first run, the first module to need no reconciliation. That is
    not luck: the pre-sweep declaration review (module 38's institutionalised
    lesson) found the one error by reading, an *over*-declaration on the
    tray-ink defect, roughly twenty-five minutes before the run would have
    found it. Reading the test beats running it for a third consecutive module.
  - **Lesson 22, applied rather than learned.** Module 39 discovered that a
    test which derives its index from the renderer's own list cannot see that
    list permuted. Here the picker test walks `FocusMode::ALL.iter()
    .enumerate()` and asserts the lit row is `i` — permute `ALL` and the
    expectation permutes with it. The defect "the picker offers its modes in a
    different order than `FocusMode::ALL`" was therefore declared to be caught
    by exactly *one* test, the ordered pin, and the sweep confirmed it: caught
    by 1. The pin sees it only because its second fixture writes out
    `[true, false, false, false]` — an expectation composed by hand, not read
    from the list under test. `FocusMode::ALL`'s own doc comment now states
    this limit at the definition, where the next person to write a test against
    it will read it.
  - **The catcher census**, from the fifty-two:
    `every_site_draws_the_role_it_claims` 48 and **sole on 9**;
    `every_colour_the_module_draws_comes_from_its_palette` 20, sole on none;
    `none_of_the_nine_deleted_constants_is_still_drawn` 20, sole on none;
    `every_site_changes_when_the_mode_does` 15, sole on none;
    `the_picker_marks_the_chosen_row_with_the_accent` 15, sole on none;
    `the_mode_hues_are_a_severity_code_and_never_the_accent` 8 and sole on 1;
    `the_tray_icon_is_inked_for_its_own_pill` 7, sole on none;
    `the_current_line_is_quiet_when_off_and_never_the_accent` 5, sole on none;
    `the_suppressed_line_is_overlay_and_only_drawn_when_there_is_one` 3, sole
    on none; then four pre-existing tests: `tray_indicator_hidden_when_off` 2,
    and `settings_render_not_empty`, `settings_render_with_rules` and
    `tray_indicator_shown_when_active` 1 each — all sole on none.
    - **A pre-existing test caught something on its own merit for the first
      time in six modules**, and it is worth being precise about which. Of the
      four, three caught exactly one defect apiece — the same defect, "all
      three mode hues follow the accent" — and they caught it *only* because
      they were re-signed to take `accented(false)` and the fixture's assertion
      fired inside them. Those three assert `!cmds.is_empty()` and
      `cmds.len() > 10`; they check nothing about colour and proved nothing
      about colour. The genuine catch is `tray_indicator_hidden_when_off`,
      whose own `assert!(cmds.is_empty())` is what sees "Off is given a hue, so
      the tray shows an indicator for no focus at all". One real pre-existing
      catch in six modules is the honest number, and the distinction matters:
      counting the other three would credit the tests for the fixture's work.
    - The ordered pin's 48-of-52 and sole-on-9 both beat module 39's record,
      and for the same reason — fifteen sites that are almost all plain role
      reads. The countervailing observation is unchanged, and this module
      supplies the sharpest example of it yet. The pin *does* catch "all three
      mode hues follow the accent" — but only incidentally, because the busy
      fixture happens to sit in Alarms Only and the pin therefore happens to
      assert the tray pill is `p.yellow`. Change that fixture's mode to `Off`,
      which draws no pill at all, and the pin's coverage of the module's worst
      defect vanishes silently, while
      `the_mode_hues_are_a_severity_code_and_never_the_accent` — which asserts
      the scale itself, in both modes, for every mode — keeps it regardless.
      A pin is an assertion about one render, and it is only as good as the
      fixture underneath it; a narrow test is an assertion about the rule, and
      survives the fixture changing. That is why the pin's ever-growing share
      of the census is not an argument for writing only pins.
- [x] `window_peek.rs` — 9 constants over 13 colour sites, done 2026-08-23.
  Sixty-six tests in the module (ten new or strengthened), harness defects
  Ax64–Zx65 (fifty-two).
  - **The first contrast bug in the series that is broken in the theme the
    shell actually ships.** The close button's X was a hard-coded `TEXT`
    (`#CDD6F4`) drawn on a hard-coded hover `RED` (`#F38BA8`). That measures
    **1.60:1 in Mocha** and 1.47:1 in Latte — so unlike the previous forty
    modules, whose near-black-on-accent bugs only surfaced when the light
    palette was rendered, this one was illegible in the dark mode the shell
    starts in, and had been since the module was written. The idle state was
    failing too, less spectacularly: `TEXT` on `SURFACE2` is 4.62:1 in Mocha
    but 3.69:1 in Latte. Both sites now read `readable_on(close_bg)`, which
    gives 8.10:1 / 4.80:1 hovered and 5.90:1 / 8.67:1 idle.
    - Worth being precise about why the *dark* failure survived: nobody was
      looking at the dark render, because the entire method of this task is
      "render in light and see what does not belong". A membership sweep asks
      whether a colour is *in* the palette, and `TEXT` on `RED` is two
      perfectly good palette members — it is their *pairing* that is
      unreadable. Contrast is not a membership property, and no amount of
      sweeping finds it. It took a test that computes the ratio.
  - **A window's own colour is not the shell's to theme**, so
    `WindowSnapshot::dominant_color` became an `Option<Color>` rather than a
    `Color` seeded from the palette — see `design-decisions.md` 526. The
    placeholder for "nobody has sampled this window yet" belongs to the
    renderer, which knows the mode; the snapshot, which is built long before
    any palette is in scope, must be able to say "no colour" without naming
    one.
  - **Three coverage holes, all found by *designing* the defects rather than
    running them** — and the sweep then confirmed each was real by making the
    test written for it the *sole* catcher of the defect that exposed it.
    - **Lesson 23: a step function collapses distinctions, so a test that
      asserts only its output cannot see its input change.**
      `readable_on(bg)` returns one of two values. `readable_on(p.peach)` and
      `readable_on(p.red)` are equal in *both* modes — near-black in Mocha,
      near-white in Latte. So the defect "the hovered close button is repainted
      peach" changes the button and changes nothing the module's headline
      legibility test can observe: the ink it asserts is identical, and the
      ratio it computes stays above the floor. The test was structurally blind
      to the thing it was named for. The fix is not a better assertion inside
      that test but an ordered pin that names the *fill* — so the pin now loops
      `close_hovered` over both states, which is also what made the hovered
      fill and its ink into rendered sites at all.
    - **Lesson 24: a test that calls the inner function cannot see a defect in
      the outer one.** Every colour test in the module called
      `PeekPopup::render`. The desktop calls `PeekManager::render`, a one-line
      delegate. A hard-coded palette constructed *there* — `&Palette::for_mode
      (false)` instead of the caller's — would have passed all sixty-five other
      tests and shipped a peek popup that ignored the theme entirely. A
      delegate is a site. `the_manager_renders_with_the_palette_it_was_given`
      is the sole catcher of that defect, exactly as predicted.
    - **A fixture that hands two states to two different objects never
      exercises their precedence.** `four_states()` hovers slot 0 and focuses
      slot 1, so no render in the module ever had one thumbnail that was both.
      Swapping the focus and hover branches of the border colour — which would
      make pointing at the focused window visibly unfocus it — passed
      everything. `a_focused_thumbnail_stays_accented_while_the_pointer_is_over
      _it` is the sole catcher of that one. The general form is worth stating:
      a fixture built to be *rich* is not the same as a fixture built to be
      *ambiguous*, and precedence is only visible under ambiguity.
  - **Preflight then sweep: 52 build / 0 do not / 0 not applied, then 52
    caught, 0 escaped, 0 never asked, 0 under-caught, 1 under-declared.** The
    single under-declaration is benign and is now recorded in the harness: the
    defect that collapses `content_color`'s whole conditional to `p.surface1`
    takes the *minimized* arm with it, so
    `a_minimized_window_ignores_whatever_was_sampled_from_it` fires too. It was
    declared against the two tests that name the sampled colour, which is the
    defect's headline; the minimized placeholder is collateral. Under-declaring
    costs nothing but the reconciliation — it is over-declaring, which claims a
    test catches something it does not, that the sweep exists to punish.
  - **The pre-sweep declaration review found the error by reading, for the
    fourth consecutive module.** The peach defect above was declared against
    the legibility test; a by-hand check of `readable_on(p.peach)` against
    `readable_on(p.red)` in both modes showed it would have been caught by
    *nothing at all*. That is a hole in the tests, not a mis-declaration, and
    running the sweep would have reported it as `escaped` twenty-five minutes
    later. Reading the test beats running it — but note what actually did the
    work here: not reading the *test*, but evaluating the function the test
    depends on, at the two inputs the defect swaps between.
  - **The catcher census**, from the fifty-two:
    `every_site_draws_the_role_it_claims` 50 and **sole on 10**;
    `every_site_changes_when_the_mode_does` 15, sole on none;
    `only_the_focused_thumbnail_wears_the_accent` 14, sole on none;
    `the_close_button_x_is_legible_on_the_button_it_marks` 14, sole on none;
    `every_colour_the_module_draws_comes_from_its_palette` 13, sole on none;
    `none_of_the_nine_deleted_constants_is_still_drawn` 13, sole on none;
    `the_manager_renders_with_the_palette_it_was_given` 13 and **sole on 1**;
    `a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it` 8 and
    **sole on 1**;
    `an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not` 7, sole
    on none; `a_minimized_window_ignores_whatever_was_sampled_from_it` 4, sole
    on none; then two pre-existing tests,
    `test_popup_render_with_hovered_thumbnail` and
    `test_popup_render_minimized_window`, 1 each and sole on none — both
    catching by command *count* rather than by colour, which is the usual
    accident.
    - The pin's 50-of-52 is the highest of the series, and its sole-on-10 is
      too. That is not a sign the other tests are redundant: two of the three
      defects the pin *cannot* see are the two coverage holes above, and each
      needed a test built specifically for it. A pin is an assertion about one
      render. It sees every site that render contains, and nothing about the
      sites it does not reach — which is why widening the fixture (looping the
      close button) raised its score, and why the manager delegate, which no
      fixture of `PeekPopup` can reach at all, stayed invisible to it.
- [x] `about.rs` — 9 constants over 19 colour sites, done 2026-08-23.
  Fifty-six tests in the module (seven new), harness defects Ax66–Zx67
  (fifty-two).
  - **The wordmark was unreadable in light mode, and the reason is worth
    stating precisely: Latte's blue is *dark*.** The product name was drawn as
    a hard-coded `MANTLE` (`#181825`) on a hard-coded `BLUE` (`#89B4FA`) logo
    tile — 8.71:1 in Mocha, which is why forty-one modules' worth of dark-mode
    review never flagged it. But the light palette's `blue` is `#1E66D5`, a
    *darker* colour than the dark palette's, because a light theme needs its
    accents to hold contrast against white rather than against near-black. Ink
    chosen to be dark enough for a pale-blue tile is therefore nearly the same
    brightness as a mid-blue one: `#181825` on `#1E66D5` measures **3.25:1**,
    below the 4.5:1 body-text floor. The fix is `readable_on(logo)`, which
    answers `#EFF1F5` there for **4.72:1**, and still answers near-black in
    Mocha. This is the same shape as the previous modules' near-black-on-accent
    bugs, but arrived at from the opposite direction — not "the ink stayed dark
    when the fill went pale" but "the fill went *dark* while the ink stayed
    dark with it."
  - **The branding-vs-accent split**, recorded as design-decisions.md §527. Two
    sites drew the same `BLUE` constant: the logo tile and the selected tab's
    label. Pointing both at `p.accent` would have given a user who picks pink a
    pink SlateOS logo, so the logo keeps `p.blue` and only the tab strip follows
    the accent. The testing consequence is the part that belongs here: because
    the two sites used to hold the same value, one `blue` assertion covered
    both, and any test left at that level would keep passing if they were
    re-merged in *either* direction. So they are pinned separately and by
    position — the logo at its own index against `p.blue`, the tab at the active
    tab's index against `p.accent`, with the accent's occurrence count asserted
    to be exactly one. None of that is expressible in a fixture that leaves the
    accent at its stock value, because the stock accent *is* `blue`.
  - **Lesson 22 applied a second time, and it cost two test failures to
    learn.** The ordered pin needs a locator saying which colour appears where
    in the tab strip. The convenient way to write it is to walk `AboutTab::ALL`
    and compare each entry to the active tab — and that is an echo, not an
    expectation: a locator derived from the list under test permutes when the
    list does, so it cannot see the list permuted. The strip is instead written
    out by hand per tab in `chrome(p, tab)`, alongside `accent_index(tab)`. The
    hand-written form is also what exposed my own error: I had assumed the
    active tab's fill is always the strip's first entry. It is not — the fill
    sits at the active tab's *index*, so Hardware renders
    `[quiet, fill, hot, quiet, quiet]`. A derived locator would have quietly
    agreed with the renderer and told me nothing.
  - **Five declaration errors caught by reading, before the sweep — the fifth
    consecutive module in which reading beat running.** One of them is lesson
    23 in a new dress and is the reason this bullet exists. The defect *"the
    logo tile is still Mocha `BLUE`"* was declared to be caught by the
    legibility test. It is not, and cannot be: the wordmark is
    `readable_on` of *that same value*, so reverting the tile to a constant
    reverts the ink with it and the pair stays perfectly legible in both modes.
    A step function's output cannot see its input change. The catcher is the
    accent test, via its light-mode `p.blue` assertion — a *membership* claim,
    not a contrast one. The other four: two accent-derived-ink defects that
    only the mode-change test sees (its skip clause skips the accent itself,
    not ink derived from it), and two "builds a palette of its own" defects
    that collapse every accented site to the stock accent and so are caught by
    the accent count dropping to zero.
  - **Sweep: 52 caught, 0 escaped, 0 never asked, 0 under-caught, 3
    under-declared.** The preflight was `52 build, 0 do not, 0 not applied`, and
    both runs ended `restored: all files match their recorded SHA-256`. The
    three under-declarations were reconciled in `scripts/reintro-palette.py`
    with the reasoning attached; each is a case of a test being *wider* than I
    credited it, which is the harmless direction.
  - Catcher census (52 defects):

    | Test | Caught | Sole catcher |
    |---|---|---|
    | `every_site_draws_the_role_it_claims` | 50 | 18 |
    | `every_site_changes_when_the_mode_does` | 24 | 0 |
    | `every_colour_the_dialog_draws_comes_from_its_palette` | 23 | 0 |
    | `none_of_the_nine_deleted_constants_is_still_drawn` | 23 | 0 |
    | `the_chosen_tab_wears_the_accent_and_the_logo_never_does` | 10 | 0 |
    | `the_wordmark_is_legible_on_the_logo_tile` | 9 | 0 |
    | `the_empty_licence_list_is_drawn_from_the_palette` | 3 | 1 |

    - **The two defects the ordered pin cannot see are both in the empty
      licence list** — a site its fixture, which has two licences, never
      renders. Not a weakness in the pin: a pin is an assertion about one
      render, and it sees every site that render contains and nothing about the
      sites it does not reach. It is the reason the empty fixture exists at all,
      and it is the same finding as module 41's, arrived at from a different
      direction — there the unreachable site was behind a delegate, here it is
      behind a data shape.
    - **The one defect only the empty-list test catches is the interesting
      one:** *"an empty licence list is reported in `RED`, as though having no
      licences were an error."* `red` is a perfectly good palette member, so the
      membership sweep is blind to it by construction; it flips correctly
      between modes, so the mode test is blind to it; and the pin's fixture
      never renders the line. Three general-purpose tests, all structurally
      unable to see a defect that is purely a question of *meaning* — whether
      "you have no licences" is a fact or a fault. That is the boundary of what
      mechanical checking reaches, and the only thing on the far side of it is a
      test written by someone who decided what the site is supposed to say.
    - The legibility test's 9-of-52 with 0 sole catches understates it. Two of
      those nine are defects the membership sweep is *obliged* to accept — the
      `CRUST` wordmark, because `0x11111B` is a `readable_on` endpoint the sweep
      was told to allow, and the accent-coloured logo, whose derived white ink
      lands at 3.14:1 on magenta. It shares those catches with the pin only
      because the pin happens to be watching the same two indices. Change the
      fixture and the pin stops seeing them; the ratio does not care.
- [x] `calendar.rs` — 8 constants over 35 colour sites, done 2026-08-23. 113
  tests in the module (ten new), harness defects Ax68–Xx69 (fifty).
  - **The largest finding is not a colour at all: a field that round-tripped
    perfectly and was never drawn.** `CalendarEvent::color` was parsed by
    `import_text`, stored, and written back by `export_text` faithfully — and
    the month grid's event dot drew a fixed `LAVENDER` that never looked at the
    field. The only site that read it was the detail card, which appears only
    for a day the user has *selected*. So a colour set in the calendar file
    survived every save/load cycle intact while changing nothing the user could
    see until they clicked the day. **A value that round-trips correctly and a
    value that is used are unrelated properties**, and a round-trip test — which
    this module had — asserts only the first. This is the generalisation of
    module 41's delegate finding: there the unexercised thing was a function,
    here it is a *read*.
  - **It was nearly missed by the search that found everything else.** The sweep
    for surviving colour work was `grep '\.color' | grep -v 'color:'` — strip
    the struct-literal noise, keep the reads. But `color: event.dot_color(p)` is
    a read *spelled* as a write, and the filter removed the one production line
    that mattered along with the forty that did not. **A filter that removes the
    noise can remove the signal with it** whenever the signal and the noise
    share a shape. The read was found by reading the renderer instead.
  - **`Option<Color>`, not a sentinel — design-decisions.md §528.** The tempting
    non-decision is `.unwrap_or(p.blue)` inside `import_text`. That makes the
    *parser* consult the palette, so the same file parses to different data
    depending on which theme is active, and `export_text` then writes that
    theme-dependent value back: merely opening the calendar in light mode would
    rewrite every event the user never coloured. A display setting must not be
    able to edit user data. `None` is resolved once, at draw time, by
    `CalendarEvent::dot_color`.
  - **Three contrast failures, all one shape: a fill role read as an ink.** The
    adjacent months' day numbers were `SURFACE2` on `BASE` — 2.46:1 in Mocha and
    **1.91:1** in Latte; the week-number gutter was the same; and the detail
    card was `SURFACE0` with `SUBTEXT` on it, **3.40:1** in Latte. Surfaces sit
    near the background — that is what they are for — so any `surface*` used as
    ink is unreadable by construction, and the dark-mode-only review that
    preceded these conversions could not see it because Mocha's surfaces are
    further from its base than Latte's are from its.
  - **The detail card is the one case where the *fill* had to move rather than
    the ink.** No quiet role clears 4.5:1 against Latte `surface0`: `subtext1`,
    the next rung up from the offending `subtext0`, still only reaches 4.05. So
    the card became `mantle` — one step *away* from `base` in both modes, a
    shallow well rather than a raised panel — which buys two readable ink tiers
    at once (5.14 for the event time, 6.57 for its title in Latte). Worth
    recording because the reflex in the previous forty-two modules was always to
    move the ink; here that reflex has no solution.
  - **Lesson 24 held again: a delegate is a site.** `render_tray_clock` is a
    one-line forward to `ClockDisplay::render` and was called from nowhere in
    the tree — every existing test called the inner function directly. It could
    have dropped the palette, transposed x and y, or handed the clock a palette
    of its own, and nothing would have failed. Two harness defects do exactly
    that, and one test is the sole catcher of both.
  - **The contrast test cannot catch a single defect in this module, and that is
    correct.** `every_pairing_the_calendar_draws_clears_the_contrast_floor`
    reads palette values and a hand-written table of which ink lands on which
    fill; it never calls the renderer. So no defect in `calendar.rs` is declared
    against it — the catchers would all be MISSING. It is not dead weight: it is
    the only thing standing between the module and a repeat of the three bugs
    above, and it would fail loudly if `appearance` moved a role. But it is a
    claim about the *palette*, not about this file, and declaring it as a
    catcher for a rendering defect would have been a declaration error in five
    consecutive modules' worth of tradition. **A test can be correct, valuable,
    and structurally incapable of catching any defect in the file it lives in.**
  - **My hand-computed contrast figure was wrong, and only running it found
    that.** I predicted Latte `accent` (which stock is `blue`, `#1D62EC`) on
    Latte `base` at 4.415:1 and expected the new contrast test to fail on the
    "Today" button. It passed; the measured value is **4.63:1**. The pairing is
    genuinely thin but it clears the floor, and no code change was warranted.
    The lesson is narrow and worth keeping: WCAG luminance is a gamma-corrected
    weighted sum, estimating it by eye is estimating an exponential, and a
    prediction that would have driven a code change has to be *computed*.
  - **Sweep: 50 caught, 0 escaped, 0 never asked, 0 under-caught, 2
    under-declared.** The preflight was `50 build, 0 do not, 0 not applied`, and
    both runs ended `restored: all files match their recorded SHA-256`. No
    declaration errors were found by reading this time — the first module in six
    where the pre-run check against the test source turned up nothing, which is
    what the previous five modules' errors were teaching.
  - Catcher census (50 defects):

    | Test | Caught | Sole catcher |
    |---|---|---|
    | `every_month_view_site_draws_the_role_it_claims` | 33 | 12 |
    | `every_role_the_calendar_draws_moves_with_the_mode` | 13 | 0 |
    | `every_colour_the_calendar_draws_comes_from_its_palette` | 12 | 0 |
    | `every_year_view_site_draws_the_role_it_claims` | 11 | 9 |
    | `the_tray_clock_delegate_draws_the_palette_it_is_handed` | 6 | 2 |
    | `a_coloured_event_shows_the_users_colour_in_the_month_grid` | 6 | 0 |
    | `the_today_button_wears_the_accent` | 3 | 2 |
    | `an_uncoloured_event_round_trips_without_gaining_a_colour` | 2 | 2 |
    | `the_dot_and_the_detail_bar_resolve_a_colour_the_same_way` | 2 | 0 |
    | `the_selection_disc_is_drawn_on_the_cell_that_is_clicked` | 2 | 0 |
    | `every_pairing_the_calendar_draws_clears_the_contrast_floor` | 0 | 0 |

    - **Twenty-seven of the fifty are caught by exactly one test, and the
      fifteen sole catches that lie outside the month-view pin are all sites a
      general-purpose check cannot reach.** Nine are in the year view, which the
      month-view pin never renders; two are behind the tray-clock delegate,
      which nothing else in the tree calls; two are the "Today" button, which by
      construction cannot coexist with a visible today cell and so is invisible
      to the scene the pin uses; two are the file format, which no renderer test
      touches at all. The other twelve sole catches belong to the pin, and are
      the role swaps and fill-role-as-ink defects that are perfectly good
      palette members drawn in the wrong place. Not one of the twenty-seven
      would have been found by widening an existing test — each needed a fixture
      that reaches the site.
    - **The contrast test caught nothing, and could not have.** It is the first
      test in this series with a zero, and the zero is structural rather than a
      gap: it reads palette values and a hand-written table of which ink lands
      on which fill, and never calls the renderer. Its job is to fail if
      `appearance` moves a role, not if `calendar` misuses one. Recorded because
      a zero row in this table has meant "the test is weak" in every previous
      module, and here it means something else entirely.
    - **Both under-declarations were the same accidental locator**, and it is a
      shape worth naming.
      `the_selection_disc_is_drawn_on_the_cell_that_is_clicked` is a *layout*
      test: it finds the selection disc, takes its centre, and asserts the hit
      test agrees. It finds it by matching `dark().surface0` — so its locator is
      a role assertion it never meant to make, and changing the disc's role
      makes `find_map` return nothing and the `.expect` fire before the geometry
      it exists to check is reached. A real catch, but a fragile one: it
      protects the role only for as long as nobody widens the locator, and it
      reports the failure in the vocabulary of layout.
    - **One of the two was also caught by a value collision in the mode test's
      skip clause**, which is worth writing down because the same clause appears
      in every module from 39 on. The clause exempts colours that are
      legitimately identical in both modes by comparing *values*: `SHADOW`, the
      accent, `readable_on(accent)`, and the user's event colour. For this
      fixture's magenta accent, `readable_on` answers `#EFF1F5` — and Mocha
      `surface0`'s `readable_on` is also `#EFF1F5`. So when the two discs are
      swapped, today's ink is misfiled as "legitimately fixed", asserted equal
      across modes, and fails because it is not. The collision only ever makes
      the test *stricter* — a site wrongly classed as fixed is asserted equal,
      never skipped — so it cannot hide a defect; it can only produce a failure
      whose message points at the wrong reason. That is why it is documented
      here and in the harness rather than tightened: keying the clause on sites
      instead of values would mean deriving the site list from the renderer, and
      lesson 22 says an expectation derived from the code under test is an echo.

- [x] `session_mgr.rs` — 7 constants over 13 colour sites, done 2026-08-24. 43
  tests in the module (ten new), harness defects Ax70–Kx71 (thirty-seven).
  - **Three of the thirteen sites were unreadable in *both* modes, not just in
    Latte.** The window count, the shortcut hint and the empty-state line were
    all `OVERLAY0` — an overlay role used as an ink, which measures **3.59:1**
    on Mocha `mantle` and **2.14:1** on Latte. Every previous module's contrast
    findings were Latte-only, because Mocha's roles are further apart than
    Latte's and a dark-mode-only review can miss a light-mode failure but not a
    both-modes one. This module is the exception: the review that preceded
    these conversions looked at Mocha and still passed three sites that fail in
    Mocha. The reason is that `overlay0` *reads* like an ink name — it is the
    only quiet-sounding role that is not in the `subtext*` family — so it was
    picked by name rather than by measurement. All three moved to `subtext1`
    (9.91 / 5.14 on mantle).
  - **The selected row moved its *fill*, and this is now a rule rather than a
    one-off.** The conventional highlight is `surface0`, but the row carries a
    caption: on Latte `surface0` the caption's natural ink `subtext0` reads
    **3.40** and even `subtext1` only reaches **4.05**, so no quiet tier
    survives there. Rather than shout the caption, the fill recedes to `mantle`
    and the picker's own background moves `mantle` → `base` to keep the two
    distinguishable. This is the second module to hit the identical wall — the
    calendar's detail card was the first — so it was written up as a
    cross-module rule in `design-decisions.md` §529 instead of a third
    per-module note: *a selection highlight may be a raised `surface0` only if
    everything drawn on it is full-strength `text`; otherwise it recedes to
    `mantle`.* The remaining five modules and the ~2,258-constant app
    conversion now have the answer without re-deriving it.
  - **§528's parser rule applied to a *constructor*.** `Workspace::new` set
    `color: BLUE`, baking one mode's blue into a saved workspace at the moment
    it was created — a value that would then outlive every theme change, so a
    workspace made in dark mode would keep wearing a pastel blue in light mode
    forever. §528 was written about a *parser* consulting the palette; the same
    hazard is present anywhere user data is *created* without a palette in
    hand, which a constructor is by definition. The field became
    `Option<Color>` and resolves at draw time via `Workspace::tag_color`. The
    default is deliberately `p.blue` and not `p.accent`: a tag whose default
    followed the accent would make every *untagged* workspace look like the
    themed one, which is the opposite of what a tag is for.
  - **One site keeps `subtext0`, and it needed a test to say *why*.** The
    unselected row's icon is the module's only surviving `subtext0`, which is
    legal on `base` (7.37 / 4.64) and illegal on the selected row's `mantle` in
    Latte (4.31). It is safe purely by construction — the ink is chosen by the
    same `if selected` that chooses the fill, so `subtext0` is only ever drawn
    where `base` is. A premise like that is invisible to every test in the
    suite: a membership test accepts the colour, a role pin accepts it, and a
    contrast test that reads a hand-written pairing table would simply have the
    correct pairing written into it. `only_the_selected_row_is_filled` pins the
    premise directly — exactly one `mantle` fill exists in the scene, and its
    `y` steps by a constant as the selection moves — so widening the fill to a
    second row breaks it. **A by-construction safety argument needs a test that
    asserts the construction, not the consequence.**
  - **Lesson 18 was avoided by prediction rather than discovered by failure.**
    The background/border transposition, written as two harness edits, is a
    silent no-op: edit 1 rewrites `color: picker_bg,` to `color:
    picker_border,`, and edit 2's first match is then that same freshly-written
    line, so the patch undoes itself and the "defect" compiles to the original
    program. It was written instead as one anchor spanning both `color:` lines
    with the intervening `// Border.` block. Second module running where the
    pre-run walk of every prediction against the test source found the trap
    before the machine did.
  - **Sweep: 37 caught, 0 escaped, 0 never asked, 0 under-caught, 3
    under-declared.** The preflight was `37 build, 0 do not, 0 not applied`,
    and both runs ended `restored: all files match their recorded SHA-256`. No
    declaration errors were found by reading — the second module in a row.
  - Catcher census (37 defects):

    | Test | Caught | Sole catcher |
    |---|---|---|
    | `every_picker_site_draws_the_role_it_claims` | 33 | 10 |
    | `every_colour_the_picker_draws_comes_from_its_palette` | 10 | 0 |
    | `every_themed_site_the_picker_draws_moves_with_the_mode` | 10 | 0 |
    | `only_the_selected_row_is_filled` | 8 | 0 |
    | `the_title_wears_the_accent_only_while_searching` | 5 | 2 |
    | `the_tag_the_picker_draws_is_the_one_the_resolver_gives` | 4 | 0 |
    | `the_empty_state_draws_the_caption_ink` | 3 | 1 |
    | `a_tagged_workspace_keeps_the_users_colour` | 2 | 1 |
    | `a_new_workspace_is_born_untagged` | 1 | 0 |
    | `every_pairing_the_picker_draws_clears_the_contrast_floor` | 0 | 0 |

    - **The ordered pin catches 33 of 37 and is the sole catcher of 10**, which
      is the highest share any single test has had in this series. That is a
      property of the module rather than of the test: the picker is one
      renderer with one scene, so unlike the calendar — month view, year view,
      tray delegate, file format — there is no second surface for a general
      check to miss. The four defects it does *not* catch are exactly the four
      that are not in the drawn output at all: three in the resolver and the
      constructor, one in the empty-state branch, which the populated scene
      never renders.
    - **The contrast test caught nothing, and again could not have.** Second
      module in a row with a structural zero, for the same reason as the
      calendar: it reads palette values against a hand-written pairing table
      and never calls the renderer. Recorded so the zero is not read as a gap.
      It is worth noting what this *does* mean, though: the three `overlay0`
      failures this module fixed were failures of that table's *contents*, not
      of the test — nobody had written the pairing down. A test that checks a
      hand-written table catches a role moving underneath it; it cannot catch a
      pairing nobody thought to list.
    - **`the_empty_state_draws_the_caption_ink` is the sole catcher of the
      empty-state ink, and this is the lesson-24 shape in a branch rather than
      a function.** The populated scene never reaches the `workspaces.is_empty()`
      arm, so every other test in the module — the ordered pin included — is
      blind to it. **A site nothing renders is a site nothing checks**, and a
      conditional arm is as much a separate site as a separate function is.
    - **Two of the three under-declarations are one accidental locator, and it
      is a new variety: positional, not value-keyed.**
      `the_tag_the_picker_draws_is_the_one_the_resolver_gives` finds the three
      colour tags at hard-coded indices 4, 10 and 14 of the drawn list. Any
      defect that changes the *number* of `FillRect`s shifts every index after
      it, so "every row is filled" and "no row is ever filled" both make the
      test read a neighbouring site and fail — in the vocabulary of tag
      resolution, about a defect in selection painting. The calendar's
      accidental locator keyed on a colour value; this one keys on list
      position. Both are real catches and both are fragile in the same way:
      they protect a property they never meant to assert, and the protection
      evaporates the moment someone adjusts the fixture. Left as-is for the
      same reason as the calendar's — deriving the indices from the renderer
      would make the expectation an echo (lesson 22).
    - **The third under-declaration is positional too, one layer up.**
      `every_themed_site_the_picker_draws_moves_with_the_mode` zips the 19-entry
      label list against the drawn list, so "every row is filled" inserts two
      commands and slides the labels out of register. The skip clause then
      exempts the wrong site, and `USER_PINK` — legitimately identical in both
      modes — is asserted to differ and does not. Same class as the calendar's
      value-collision under-declaration and same direction of error: the
      misalignment can only make the test fail where it should not have been
      asked, never pass where it should have failed, so it cannot hide a
      defect. **Every under-declaration these four modules have produced has
      been an alignment or collision artefact of a test locating its site
      indirectly** — which is the price of not deriving locators from the code
      under test, and a price worth paying.
    - **`B×70` is the one under-declaration I predicted and then failed to
      carry.** The picker's background left as the literal `0x181825` is caught
      by `only_the_selected_row_is_filled`, because in that test's dark fixture
      the literal *is* `p.mantle`, so the background `FillRect` matches the
      mantle locator and the test counts two filled rows. I had predicted
      exactly this for the role-swap variant (`A×70`, background → `p.mantle`)
      and did not carry the reasoning across to the Mocha-literal variant of
      the same site. **A literal and the role it equals in one mode are the
      same defect to any test that runs in that mode**; declarations for the
      two variants of a site should be written as a pair, not independently.
- [x] `file_drop.rs` — 6 constants over 13 colour sites, done 2026-08-24. 41
  tests in the module (eleven new), harness defects Ax72–Hx73 (thirty-four).
  - **The badge ink has to be `base`; `crust`, the reflexive choice, fails all
    four effect colours in Latte and passes all four in Mocha.** The effect
    badge is a filled chip whose fill is one of four saturated hues — red
    forbids, green copies, blue moves, peach links — with a label drawn on top,
    so the ink is being asked to read on four different backgrounds at once.
    `crust` measures 8.10 / 12.61 / 8.91 / 10.59 on those four in Mocha and
    **4.10 / 3.98 / 3.96 / 3.95** in Latte — four failures, none of them
    visible to a dark-mode review. `base` measures 7.08 / 11.03 / 7.79 / 9.27
    and **4.80 / 4.66 / 4.63 / 4.62**, clearing the floor on all eight. The
    margin is thin enough on the Latte side (4.62 against 4.5) that the
    reasoning is now written into the module header rather than left to be
    re-derived: the darkest role is not automatically the most readable ink,
    because Latte's `crust` is a *light* colour and the effect hues are
    mid-tone.
  - **No `surface*` or `overlay0` role can be a badge fill in either mode.**
    The obvious-looking "quiet chip" for the item count is `surface2` with
    `text` on it, which is **4.62 in Mocha and 3.69 in Latte**; `surface1` is
    6.31 / **4.39**; `overlay0` fails with every ink there is. Only four
    pairings in the whole palette work for a chip on the card: `peach`+`base`,
    `subtext1`+`base`, `text`+`base` and `text`+`crust`. The count chip took
    `text`+`base` — an inverted chip — which is the only one of the four that
    is not also an effect colour.
  - **The item-count chip was `peach`, and so is `DropEffect::Link`.** Both
    were the same hardcoded `MOCHA_PEACH`, so a twelve-file *copy* drag drew a
    green badge next to a peach chip that means "link" everywhere else in the
    same overlay. This is a meaning collision rather than a contrast failure,
    and no contrast or membership test can see it: peach is a legal member and
    reads fine on the card. `the_item_count_chip_is_never_an_effect_colour`
    asserts the disjointness directly, against all four effects in both modes.
  - **The contrast test changed shape — it now reads the pairing out of the
    rendered commands — and stopped being a structural zero.** The previous
    four modules each had a contrast test that compared palette values against
    a *hand-written* pairing table and never called the renderer; each caught
    exactly nothing in its own file, because a table of pairings is a claim
    about the palette, and no defect in the module under test can falsify it.
    Here the walk takes `cmds.windows(2)`, pairs every `FillRect` with the
    `Text` that follows it, and asserts 4.5:1 on whatever it finds. It caught
    **14 of 34**, second only to the ordered site table. It is not an echo
    (lesson 22) because nothing it asserts comes from the code under test: the
    4.5:1 floor comes from WCAG, and the required pair count (24 on the card,
    8 on the tooltip) is written out by hand, so a site that stops being drawn
    fails the count rather than silently passing an empty walk. **A contrast
    test should read its pairings from the rendered output; a hand-written
    pairing table can only catch a role moving underneath it.**
  - **…and it is still blind to a *transposition*, because contrast is
    symmetric.** `X×72` swaps the tooltip's fill and its ink, and the walk
    computes `contrast(fill, ink)` — a symmetric function — so it returns
    exactly the ratio it returned before. The declaration was wrong, not the
    test: legibility genuinely survives a transposition and the design
    genuinely does not, which is the ordered site table's job. Fixed by
    dropping the declaration and writing the reason into the test's doc
    comment. **Lesson: a contrast check cannot see its two operands exchanged.**
  - **A `zip` between an expectation table and a drawn list truncates, and that
    was a real hole in two tests, not a bad declaration.** `F×73` stops drawing
    the tooltip entirely. Both
    `every_site_the_drop_highlight_draws_moves_with_the_mode` and its drag-card
    twin asserted only that the two modes drew *the same number* of colours —
    which a vanished site satisfies, since it vanishes from both — and then
    zipped the three-entry site table onto a one-entry drawn list, so the two
    missing sites fell off the end of the zip and were never asked whether they
    move. The role tests already pinned their length against the table; the
    mode tests now do too. This is lesson 24's shape once more (*a site nothing
    renders is a site nothing checks*) arriving through a new door: not a
    branch nobody exercised, but a `zip` quietly shortening the question.
  - **The colour extractor flattens alpha, so every colour test in the module
    was blind to the deliberate translucency.** `colors()` forces alpha to 255
    so that a role comparison is not defeated by a card drawn at 220. That is
    right for the role tests and wrong as a total picture: the drag card is
    220/255 and the drop tooltip 200/255 on purpose — a drag decoration must
    not hide what is being dragged *onto* — and nothing asserted it.
    `only_the_card_and_the_tooltip_are_translucent` pins both alpha vectors
    exactly, and is the sole catcher of the two defects that make them opaque.
    The alphas are also deliberately *not* `p.panel_alpha`: a drag decoration
    is not a panel, and tying it to the panel setting would let a user who
    likes opaque panels lose the drag preview.
  - **A geometry test must locate by geometry.**
    `a_multi_item_description_stops_before_the_count_badge` is a *layout* test,
    and it found the count badge as "the only peach fill on the card". That
    made it silently assert the badge's colour as well, and it would have
    stopped working the instant the role changed — reporting a colour defect in
    the vocabulary of overlap. It now locates the badge by the only thing that
    is actually about geometry: `width == COUNT_BADGE_W`, a named constant
    introduced for the purpose.
  - **Sweep: 34 caught, 0 escaped, 0 never asked, 2 under-caught, 5
    under-declared**, on a preflight of `34 build, 0 do not, 0 not applied`.
    Both under-catches were genuine findings rather than noise — one a wrong
    declaration (the symmetric contrast test), one a real hole in two tests
    (the truncating zip) — and both were re-run to green after the fix. Every
    run ended `restored: all files match their recorded SHA-256`.
  - Catcher census (34 defects):

    | Test | Caught | Sole catcher |
    |---|---|---|
    | `every_drag_overlay_site_draws_the_role_it_claims` | 17 | 3 |
    | `every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on` | 14 | 0 |
    | `every_drop_highlight_site_draws_the_role_it_claims` | 8 | 1 |
    | `every_site_the_drag_overlay_draws_moves_with_the_mode` | 7 | 0 |
    | `every_colour_the_drag_overlay_draws_comes_from_its_palette` | 6 | 0 |
    | `an_unlabelled_target_draws_no_tooltip` | 6 | 1 |
    | `each_drop_effect_wears_its_own_role` | 5 | 1 |
    | `the_drop_tooltips_label_is_readable_on_its_fill` | 5 | 0 |
    | `only_the_card_and_the_tooltip_are_translucent` | 4 | 2 |
    | `the_item_count_chip_is_never_an_effect_colour` | 3 | 0 |
    | `every_colour_the_drop_highlight_draws_comes_from_its_palette` | 3 | 0 |
    | `every_site_the_drop_highlight_draws_moves_with_the_mode` | 3 | 0 |
    | `a_single_item_drag_draws_no_count_chip` | 1 | 1 |
    | `a_multi_item_description_stops_before_the_count_badge` | 1 | 0 |

    Counts are after the two fixes below; the two `moves_with_the_mode` rows
    each gained one catch when the truncating `zip` was closed.

    - **The ordered pin's share fell to 17 of 34 — the lowest in the series —
      and that is the point.** Its share was 33/37 in `session_mgr` because
      that module is one renderer with one scene. `file_drop` has two
      renderers, a four-way semantic mapping, two conditional branches and two
      deliberate alpha values, and the census shows each of those needing its
      own test: the mapping is caught only by the pinned effect vector, the
      branches only by the two absence tests, the alphas only by the
      translucency test. A falling sole-catcher count on a rising defect count
      is what a well-decomposed suite looks like.
    - **All five under-declarations are one test: `an_unlabelled_target_draws_no_tooltip`.**
      It asserts that an empty label draws exactly one command, and then — to
      confirm the survivor is the border rather than a stray fill — that its
      colour is `p.green`. That second clause is an accidental value-keyed
      locator: every defect that changes what `DropEffect::Copy` resolves to
      (copy/move transposed, all-blue, all-accent) or that draws the border in
      some other role trips it, in the vocabulary of tooltip suppression. It is
      the calendar's variety of accidental locator, and errs only in the
      stricter direction: it can fail where it was not asked, never pass where
      it should have failed. **Five modules in, every under-declaration this
      harness has produced has been an alignment or collision artefact of a
      test locating its site indirectly.**
- [x] `input_method.rs` — 5 constants over 7 colour sites, done 2026-08-24. 39
  tests in the module (twelve new), harness defects Ax74–Hx75 (thirty-four).
  - **The layout name was `MOCHA_BLUE`, which is the stock accent's value in
    disguise.** `Palette::for_mode(false).accent` *is* `blue` unless the user
    has chosen otherwise, so a hardcoded `#89B4FA` title is indistinguishable
    from `p.accent` on a default install and diverges silently the moment
    anyone picks a different accent — at which point the pop-up's heading is
    the only thing in the shell still wearing last month's accent. This is
    `backup_settings.rs`'s blue/accent collision seen from the other side: there
    a role-named binding secretly meant the accent, here a literal did.
  - **It became `p.text`, not `p.accent`, and the negative is pinned.**
    `focus_assist::render_settings` draws its panel title `p.text` bold and
    states the rule the shell follows: the accent is reserved for "you chose
    this" — a selected item, an active toggle — and a heading that is merely
    *present* has not been chosen. A layout preview is a description of the
    keyboard, so nothing in it is a choice. `no_site_in_this_module_wears_the_accent`
    asserts that against an off-palette accent in both modes, which makes the
    decision a test rather than a comment.
  - **The role-shaped return of that bug is invisible to every test except the
    ordered one.** `N×74` sets the title to `p.accent`. On the stock palette
    that draws the same bytes as `p.blue`, so a membership sweep sees a legal
    role, the contrast walk sees 7.79:1 on `base` and passes, and the mode test
    sees a value that moves correctly between Mocha and Latte. Only the ordered
    site table, which says *this site is `text`*, can tell the difference. This
    is lesson 9's shape (a set cannot see a permutation) applied to a single
    site: **a membership test cannot distinguish two roles that are equal.**
  - **Three fills that have to stay three, and contrast has nothing to say
    about any of the pairings.** The pop-up is `base`, its border `surface1`,
    its key caps `surface0` — three neighbouring neutrals, so a defect that
    collapses any two produces a picture with no border, or caps that dissolve
    into the pop-up, while every ink on top stays perfectly readable. Contrast
    is a property of *ink on fill*; `surface0` on `base` is 1.32:1 in Mocha and
    the design is fine, so no legibility floor can be asked about it.
    `the_popup_its_border_and_its_key_caps_are_three_different_values` asserts
    the construction directly — all three pairings, both modes — which is the
    "assert the construction, not the consequence" corollary of lesson 24. It
    was widened to cover `edge != fill` **before** the sweep rather than after,
    on the reasoning that an invisible border is exactly the defect an ordered
    table would catch for the wrong reason.
  - **The §530 contrast walk now pairs an ink with the most recent fill, not
    the adjacent command.** `file_drop`'s version used `cmds.windows(2)` and
    paired each `FillRect` with the `Text` immediately after it. That works
    where every fill is followed by its own label, and breaks here: the pop-up
    draws background, then border, then title, so a naive adjacency walk would
    check the title against the *border* colour rather than against the surface
    it actually sits on. The walk now carries `under: Option<Color>`, updated by
    every `FillRect` and read by every `Text`, and panics if an ink is drawn
    before any fill. It still counts its pairs (`checked == 12`) so a site that
    stops being drawn fails the count rather than passing an empty walk.
  - **A real layout bug: a key label could paint 2px onto the neighbouring
    cap.** The label starts 6px inside its cap, so its clip width has to be
    `key_size` less *both* insets; it was `key_size - 4.0`, which is neither
    inset subtracted properly and leaves a wide glyph overhanging. Now
    `key_size - 12.0`, with the arithmetic spelled out in a comment.
    `a_key_label_is_clipped_inside_its_own_cap` measures it, and — carrying
    module 45's lesson — finds each cap by `width == KEY_SIZE` rather than by
    its colour, which required promoting the cap size to a named constant. A
    geometry test that locates by colour silently asserts a role.
  - **Both `moves_with_the_mode` tests were born with the table-length pin.**
    Module 45 found the truncating `zip` the hard way, as an under-catch; here
    the pin was written in from the start, so `E×75` (the preview stops drawing
    entirely) had somewhere to fail that was not the absence tests. A lesson
    is only learned once if it is applied to the next module before the sweep,
    not after it.
  - **Two absence tests, because two branches exist.** `render_preview` returns
    empty when `preview_visible` is false or when there is no active layout,
    and `render_tray_indicator` draws `"??"` in the latter case rather than
    nothing. `a_manager_with_no_layouts_draws_no_preview` and
    `a_manager_with_no_layouts_still_draws_a_tray_chip` cover the branch from
    both sides — a branch nothing renders is a branch nothing checks
    (lesson 24), and the second half of that pair is the one that says the tray
    does *not* silently disappear.
  - **Sweep: 34 caught, 0 escaped, 0 never asked, 0 under-caught, 0
    under-declared** — the first fully clean sweep in the series, on a
    preflight of `34 build, 0 do not, 0 not applied`, ending `restored: all
    files match their recorded SHA-256`. Six modules in, this is the first with
    no under-declarations at all, and the reason is worth recording because it
    is repeatable rather than lucky: every declaration was predicted from
    *arithmetic* (all eighteen contrast figures computed before any defect was
    written) rather than from intuition, and the module has no value-keyed
    locators left — `a_key_label_is_clipped_inside_its_own_cap` finds its cap by
    `width == KEY_SIZE`, which is what every previous module's stray catches
    came from.
  - Catcher census (34 defects, 83 catches, 2.44 per defect):

    | Test | Caught | Sole catcher |
    |---|---|---|
    | `every_preview_site_draws_the_role_it_claims` | 20 | 4 |
    | `every_ink_this_module_draws_is_readable_on_what_it_sits_on` | 16 | 0 |
    | `every_tray_site_draws_the_role_it_claims` | 9 | 0 |
    | `a_manager_with_no_layouts_still_draws_a_tray_chip` | 8 | 0 |
    | `the_popup_its_border_and_its_key_caps_are_three_different_values` | 6 | 0 |
    | `every_site_the_preview_draws_moves_with_the_mode` | 5 | 0 |
    | `no_site_in_this_module_wears_the_accent` | 4 | 0 |
    | `every_colour_the_preview_draws_comes_from_its_palette` | 4 | 0 |
    | `a_key_label_is_clipped_inside_its_own_cap` | 4 | 3 |
    | `every_colour_the_tray_draws_comes_from_its_palette` | 2 | 0 |
    | `every_site_the_tray_draws_moves_with_the_mode` | 2 | 0 |
    | `test_manager_render_preview_hidden` | 1 | 1 |
    | `test_manager_render_preview_visible` | 1 | 0 |
    | `test_empty_manager_tray_label` | 1 | 0 |

    - **The four defects only the ordered table caught are exactly the four
      predicted to be invisible to everything else**, which is the closest this
      harness has come to a controlled result. `H×74` makes the pop-up `mantle`
      — a legal role, 12.14:1 under `text`, correct in both modes. `N×74`
      returns the title to `blue` — the accent collision this module exists to
      fix, and on the stock palette *the same bytes* as `p.accent`. `V×74` and
      `W×74` transpose a cap with its label and the pop-up with its caps —
      invisible to membership (both values are members), invisible to the mode
      test (both move), and invisible to contrast, because contrast is
      symmetric. Module 45 learned that symmetry the hard way as an
      under-catch; here it was declared correctly in advance, and the sweep
      agreed.
    - **The §530 contrast walk caught 16 of 34 (47%), against 14 of 34 (41%) in
      `file_drop`.** Two modules of evidence now, on either side of a shape
      change (adjacency → most-recent-fill), both roughly half the defect set —
      versus the *zero* that four consecutive hand-written pairing tables
      caught. The walk is never a sole catcher in either module, which is the
      expected result rather than a disappointment: it is a floor, and a floor
      is meant to be redundant with the design.
    - **`no_site_in_this_module_wears_the_accent` caught 4 and was never a sole
      catcher.** The ordered table catches all four too. It is kept anyway,
      because the two tests fail in different vocabularies: the table says "the
      layout name should be `text` and is `accent`", which is a fact, and the
      accent test says "nothing here is a choice the user made", which is the
      *reason*. A test whose only job is to make a rule falsifiable is worth
      its runtime even at zero sole catches.
    - **The absence test caught 8, the widest breadth of any non-table test,
      and none of them solely.** `a_manager_with_no_layouts_still_draws_a_tray_chip`
      renders the `"??"` fallback and then checks its colours, so it is
      incidentally a second tray-site assertion. That is breadth acquired by
      accident rather than design — the same shape as the previous modules'
      under-declarations — but it produced none here, because every tray defect
      it trips was already declared against the tray table. An accidental
      locator is only a nuisance when it is also an *undeclared* one.
    - **Three pre-existing tests earned catches** (`test_manager_render_preview_hidden`,
      `test_manager_render_preview_visible`, `test_empty_manager_tray_label`),
      one of them solely. The conversion did not make the old suite redundant;
      it filled the gaps around it.
- [x] `blur.rs` — 3 constants over 6 tint sites, done 2026-08-24. 59 tests in
  the module (eleven new), harness defects Ax76–Fx77 (thirty-two).
  - **The headline defect here was not a colour at all: the module whose entire
    job is transparency ignored the transparency setting.** `BlurEffect`'s five
    presets each hardcoded a tint alpha (160/120/100/140/140), and nothing
    anywhere read `TransparencyLevel`. A user who set transparency to **Off**
    got a taskbar that was still 160/255 tint over a blur — about 37%
    see-through — which is the one end of the scale that is not a matter of
    taste: *Off* is a promise that nothing shows through, and it was being
    broken. The conversion is what surfaced it, because threading a `Palette`
    into the constructors put `panel_alpha` in scope at exactly the six sites
    that had been ignoring it.
  - **The fix anchors the existing weights at the end of the scale they were
    written for.** The five presets' numbers are a designed hierarchy (a menu
    tints least, the taskbar most), so they are not wrong — they are the
    *Full*-transparency row of a table with only one row. `scaled_tint(preset,
    panel_alpha)` interpolates each weight from its designed value at
    `TINT_ANCHOR = 160` (`TransparencyLevel::Full`) up to fully opaque at 255
    (`Off`), which keeps every existing look unchanged at the setting it was
    drawn for and makes the other three settings mean something. The anchor is
    named rather than inlined precisely so the "these numbers were written for
    Full" claim is checkable, and
    `the_tint_weights_are_written_for_the_full_setting` checks it.
  - **`impl Default for BlurEffect` was deleted rather than fixed.** `Default`
    takes no arguments, so it structurally cannot see a palette; any body it
    could have would return a hardcoded colour, which is the defect this whole
    task exists to remove. Keeping it and "fixing" it would have left a trait
    impl *guaranteed* to be wrong in light mode while looking like the blessed
    way to get a blur. Deleting it turned nine silently-wrong call sites into
    nine compile errors, each resolved to `BlurEffect::standard(p)` at a point
    where a palette was already in hand. **A trait whose signature cannot
    accept the context the correct answer depends on is not a trait to
    implement.**
  - **Six preset tests existed and not one of them read `tint`.**
    `test_preset_taskbar` and its siblings asserted radius, opacity, noise and
    saturation — every field except the colour. That is the same shape as
    lesson 24 (a delegate is a site): the tests were not weak, they were
    *aimed elsewhere*, and a field nothing asserts is a field nothing
    protects. Three of the thirty-two defects (`Dx77`, `Ex77`, `Fx77`) are
    non-colour drifts kept in the set purely to confirm those six still work
    as the guard for the fields they *do* cover.
  - **`palette_check` grew a value-shaped entry point.** Every previous module
    fed `assert_drawn_from` a `Vec<RenderCommand>`, but `BlurEffect` never
    renders — its tint is a struct field consumed by the compositor. Rather
    than have the tests synthesise fake `RenderCommand`s to be allowed through
    the door, `assert_colours_from(p, &[(label, colour)], derived, what)` takes
    the values directly; both entry points now delegate to one `assert_one`, so
    the membership rule (RGB-only, black at any alpha, the two `readable_on`
    endpoints) has exactly one definition. Two self-tests were added to
    `palette_check` itself: one proves a leftover Mocha value fails *and names
    its site*, one sweeps every role at every alpha through the value path.
  - **A hole was found by predicting catchers, before the sweep rather than
    after.** The first version of the weights test was an ordering chain
    (`menu < title < notification < taskbar`), and `standard`'s weight appears
    nowhere in it — it sits between no two others, so nothing constrained it at
    all. **A pure ordering assertion cannot constrain a value that sits between
    no two others** (lesson 25); the fix was a hand-written exact six-entry
    table, with the ordering clause kept as a statement of *why* those numbers,
    not as the check. Defects `Tx76` and `Ux76` exist only because that hole
    was found.
  - **Sweep: 32 caught, 0 escaped, 0 never asked, 0 under-caught, 0
    under-declared** — the second fully clean sweep in a row, on a preflight of
    `32 build, 0 do not, 0 not applied`, ending `restored: all files match
    their recorded SHA-256`.
  - Catcher census (32 defects, 46 catches, **1.44 per defect**):

    | Test | Caught | Sole catcher |
    |---|---|---|
    | `every_preset_tints_with_the_role_it_claims` | 16 | 9 |
    | `the_tint_weights_at_full_transparency_are_the_ones_they_were_designed_as` | 9 | 9 |
    | `every_tint_comes_from_its_palette` | 4 | 0 |
    | `every_tint_moves_with_the_mode` | 4 | 0 |
    | `no_blur_tint_wears_the_accent` | 3 | 0 |
    | `less_transparency_is_never_more_see_through` | 3 | 0 |
    | `transparency_off_leaves_no_blurred_surface_see_through` | 2 | 0 |
    | `the_tint_weights_are_written_for_the_full_setting` | 1 | 1 |
    | `a_panel_alpha_below_the_anchor_is_clamped` | 1 | 0 |
    | `test_preset_taskbar` | 1 | 1 |
    | `test_preset_none` | 1 | 1 |
    | `test_default_effect` | 1 | 1 |

    - **1.44 catches per defect against `input_method`'s 2.44, and that drop is
      the module's shape rather than a weakness.** Twenty-two of thirty-two
      defects have exactly one catcher. Every previous module rendered, so a
      defect passed through a membership test, a mode test, a contrast walk and
      an ordered table on its way out; `blur` produces no `RenderCommand` at
      all and has no ink-on-fill pair anywhere, so there is no contrast walk to
      be redundant with, and its entire observable surface is six struct
      fields. **Redundancy between tests is a property of how many independent
      views of the output exist, not of how carefully the tests were written** —
      a module with one view gets one catch per defect however hard you try.
      The consequence to carry forward is that a module like this has no margin:
      a single missing test is a defect class that escapes outright, which is
      exactly what the pre-sweep hole would have been.
    - **The exact-table test was the sole catcher on all nine of its catches,
      and three of those nine would have escaped the ordering chain it
      replaced.** `Bx77` (every tint opaque) and `Cx77` (every preset scaled
      with the menu's weight) flatten the hierarchy, so the discarded
      `menu < title < notification < taskbar` chain would have caught them, as
      it would `Px76`–`Sx76`. But `Tx76` (the standard surface drifts to 200),
      `Ux76` (the standard surface goes opaque) and `Vx76` (the no-blur
      fallback is put through the scaling) touch only values the chain never
      names, and would have gone out clean. Lesson 25 is therefore not a
      theoretical hazard — it was worth exactly three escapes on the one module
      where it was checked before the run rather than after.
    - **All three pre-existing preset tests earned a catch, each solely.**
      `test_preset_taskbar`, `test_preset_none` and `test_default_effect` cover
      radius, opacity and noise — the fields the new colour tests deliberately
      say nothing about — so `Dx77`, `Ex77` and `Fx77` have exactly one catcher
      each and it is the old one. The eleven new tests did not subsume the old
      six; the two sets partition the struct, and the sweep shows the partition
      has no gap in it. `test_default_effect` survives the deletion of `impl
      Default` because it was rewritten around `BlurEffect::standard(p)` —
      keeping the test while removing the trait is what turned an unfixable
      API into a fixed one without losing its coverage.
    - **`every_preset_tints_with_the_role_it_claims` caught half the set (16 of
      32) and nine of them solely,** which makes it by a wide margin the most
      load-bearing test in the module. That is the expected shape for a
      six-row hand-written role table checked against six constructors: it is
      the only test that knows *which* role belongs to *which* preset, so every
      substitution and every transposition lands on it and on nothing else.
      Note the contrast with modules 42–45, where a hand-written table caught
      nothing: those tables paired an ink with a fill and asserted a *contrast*,
      which is a fact about the palette. This one asserts an *identity* —
      "the taskbar's tint is `base`" — which is a fact about the module, and a
      fact about the module is the only kind a test of the module can falsify.
- [x] `wallpaper.rs` — 1 constant over 2 sites, done 2026-08-24. 92 tests in
  the module (fifteen new), harness defects Ax78–Wx78 (twenty-three).
  - **A one-constant module turned out to be a live instance of §528: the
    substituted default was being laundered into the user's file as a
    choice.** `WallpaperConfig::color` was a plain `Color` defaulting to
    `palette::BASE`, and `save_config` writes `color=` unconditionally — so the
    first save after startup wrote the shell's *guess* into the user's config
    as though the user had picked it. Converting the constant to `p.base`
    would have made the guess mode-dependent and left the laundering intact: a
    desktop that started in dark mode and was then saved would be pinned to
    Mocha's `base` forever, in light mode too, with nothing anywhere recording
    that it had ever been a default. **A default that a save turns into a
    choice is a one-way door**, and the door was already there before this task
    touched it.
  - **The fix is `Option<Color>`, not a better default.** `color: None` means
    "follow the theme", and `background(&self, p)` resolves it as
    `self.config.color.unwrap_or(p.base)` at paint time, so an unset background
    is a *role read on every frame* rather than a value captured once.
    `save_config` writes no `color=` key at all when it is `None`, `from_yaml`
    loads a file without that key as `None`, and `follow_desktop_base()` makes
    the unset state reachable again after a choice — which matters, because
    without it the door is still one-way, just harder to notice. Four tests pin
    the round trip end to end, and
    `choosing_the_base_and_following_it_are_different_states` states the point:
    picking the colour that happens to equal today's `base` and following
    `base` are different things, and must stay different when the mode changes.
  - **`DynamicTheme`'s five phase colours are exempt, and the exemption is
    written down rather than assumed.** They are a picture of the sky at five
    times of day — dawn purple through midnight near-black — not roles, and
    theming them would mean a "dynamic" wallpaper that stops being a sky. The
    module header now says so, and `every_mode()` renders the dynamic mode by
    setting `config.mode` directly, so the sweep sees the module's own phase
    table rather than a copy handed in by the test.
  - **The exemption's first draft was an echo, and would have widened the sweep
    to admit the very defect it was written to allow past.** `sky_tones()`
    originally read the five phases out of `DynamicTheme::default()` — the code
    the exemption excuses — so a phase changed to Mocha's `base` would have
    been declared derived *by the test itself* and passed. Lesson 22 again, in
    the one place where being wrong is invisible: an expectation derived from
    the code under test cannot fail. Replaced with a hand-written `const SKY`
    plus `the_five_sky_tones_are_the_ones_the_module_was_written_with`,
    committed separately so the fix is legible as a fix.
  - **Predicting each defect's catcher before the run found a test that could
    not fail.** `render_slideshow_without_images_still_renders` passes whether
    or not a *populated* slideshow draws anything, because an empty slideshow
    is the only case it constructs — so nothing in the module asserted that a
    slideshow with pictures in it shows one. `a_slideshow_with_images_draws_one`
    was added before the sweep, and `Vx78` fired against it.
  - **First sweep: 23 caught, 0 escaped, 0 never asked, but 3 under-caught and
    2 under-declared.** All three under-catches turned out to be real defects
    in the *instruments* rather than noise, and they are the most valuable
    thing this module produced.
  - **Under-catches 1 and 2 (`Bx78`, `Kx78`) exposed a hole spanning every
    module swept to date, and are why `design-decisions.md` §532 exists.**
    `Ax78` reintroduced the desktop background as a Mocha `base` literal and
    was caught; `Bx78` reintroduced the identical site as a *Latte* `base`
    literal and was not. Two defects differing only in which mode's leftover
    they represent had been getting different answers — because `palette_check`
    unconditionally allowed the two values `readable_on` can return, and
    `0xEFF1F5` **is** Latte `base` while `0x11111B` **is** Mocha `crust`. The
    sweep had been told to wave past precisely the two most likely leftovers in
    the shell, in all forty-eight modules converted so far. The exemption is now
    a per-module declaration (thirteen modules, one line each — *measured*,
    against the old comment's estimate of "most of them"), and
    `a_readable_on_endpoint_is_not_exempt_in_the_mode_that_lacks_it` pins it.
    **A membership exemption for a value that is also a role silently
    un-checks that role everywhere; an exemption must be declared by the module
    that needs it.**
  - **The refactor immediately paid out on already-swept modules.** Re-running
    five older defects that reintroduce one of the two endpoints found no
    regression (5 of 5 still caught) and *two new catches*: `Ex43`
    (`default_apps`, the current chip's ink frozen to `CRUST`) and `Px60`
    (`accessibility_settings`, the chosen tab's lettering frozen to Mocha
    `crust`) are now caught by their modules' membership sweeps, which
    previously could not see them at all. Both declarations were updated in the
    harness. That is two defects which, had they been introduced by a real edit
    rather than by the harness, would have relied entirely on a single per-site
    table to catch them.
  - **Under-catch 3 (`Fx78`) was a hole in a pre-existing test, and a new
    lesson.** `render_solid_produces_one_fill` set the user's chosen colour to
    `Color::from_hex(0x1E1E2E)` and rendered with the dark palette — whose
    `base` **is** `#1E1E2E`. The test therefore could not distinguish "draws
    the colour the user chose" from "ignores the user and draws the theme", and
    `Fx78`, which does exactly the latter, passed it. **A fixture value that
    equals a role cannot distinguish the choice from the role** — the fixture
    has to be off the palette for the same reason lesson 21 says it has to be
    off the instruments. Fixed with an off-palette constant *plus an assertion
    that it is off-palette*, so the property cannot rot silently the next time
    a role changes.
  - **Both under-declarations were catches, not misses,** and both were folded
    back into the harness: `Ex78` (the background falls back to the accent) is
    also caught by `an_unset_background_moves_with_the_mode`, because the
    fixtures set one accent for both modes so an accent-backed desktop stops
    moving; `Ox78` (choosing a colour records no choice) is also caught by
    `config_save_load_roundtrip_solid`, because a choice never recorded cannot
    survive a round trip — and, after the `Fx78` fix, by
    `render_solid_produces_one_fill` as well.
  - **Re-sweep after the three fixes: 23 caught, 0 escaped, 0 never asked, 0
    under-caught, 0 under-declared** — 72 catcher-slots over 17 distinct tests,
    median 3 catchers per defect (min 1, max 6). Every touched file restored to
    its recorded SHA-256.
  - **The five single-catcher defects name five *different* tests, and that is
    the useful shape.** `Hx78` (an image's letterbox underlay stops going
    through the resolver) rests only on `an_images_underlay_follows_the_theme_too`;
    `Nx78` (the saver drops a colour key that was chosen) only on
    `config_save_load_roundtrip_solid`; `Rx78` (the dynamic gradient darkens by
    twice as much) only on `every_colour_this_module_draws_comes_from_its_palette`;
    `Ux78` (dawn and evening transposed) only on
    `the_five_sky_tones_are_the_ones_the_module_was_written_with`; `Vx78` only
    on the test written for it. No test is the sole guard of more than one
    defect, so no single deletion opens more than one hole — unlike module 47,
    where one table was the sole catcher nine times.
  - **`Ux78` is lesson 9 demonstrated rather than argued.** Transposing dawn
    and evening changes no *value* the module draws, only which hour draws it,
    so the membership sweep — which caught nine other defects here, including
    `Rx78` alone — is structurally blind to it. The ordered hand-written `SKY`
    table is the only instrument in the module that can see a permutation, and
    it is exactly the table whose first draft was an echo. Had that draft
    shipped, `Ux78` would have escaped outright: the echo would have agreed
    with the transposition.

- [x] `a11y.rs` — 1 constant over 4 sites, plus two translucent Mocha-`base`
  fills and a pair of white crosshairs, done 2026-08-24. **The last of the
  49.** 63 tests in the module (twelve new), harness defects Ax79–Yx79
  (twenty-five).
  - **The dangerous defect in this module was not the leftover constant. It
    was the ink the leftover constant was holding in place.** The magnifier
    lens was `Color::rgba(30, 30, 46, 200)` — Mocha `base` at partial alpha —
    and its crosshairs were white at half alpha. Converting only the fill to
    `p.base` would have been a clean-looking diff that produced a **1.06:1**
    hairline in light mode: measured, half-alpha white over Latte `base` is
    very nearly invisible, and it would have been invisible *in a screen
    magnifier*, over the enlarged copy of the thing the user cannot otherwise
    read. The crosshairs are now `readable_on(p.base)` at full alpha —
    **14.50:1 dark, 16.58:1 light**. The lesson generalises past this module:
    **a fill converted to a role drags its ink with it. An ink left as a
    literal beside a fill that moved is a light-mode bug waiting for the fill
    to move.** Defect `Ex79` (choose the crosshair ink for the rim rather than
    for the fill it sits on) exists to keep that pairing pinned.
  - **The lens is opaque now, and deliberately does not read the transparency
    setting** — `design-decisions.md` §533. The two alphas it used to carry
    (200 for the circular lens, 220 for the docked strip) had no comment and
    no test between them; nothing had chosen them. Behind this overlay is not
    other content, it is *a second rendering of the very thing being read*, so
    any alpha at all is a double image at the point of regard. `Kx79` and
    `Lx79` reintroduce each alpha; both are caught only by
    `the_lens_is_opaque_in_both_modes`.
  - **A second bug fell out of the same read: the docked strip assumed the
    screen was 1920 wide.** The literal was there with a comment promising it
    would one day be the real width. `render_overlay` now takes `screen_w`.
    `the_docked_strip_spans_the_screen_it_was_given` checks 1280 *and* 3840,
    because a single width cannot tell "uses the argument" from "returns a
    constant that happens to match the fixture."
  - **Two kinds of exception, and only one of them needs an exemption.**
    `HighContrastTheme`'s four schemes exist precisely to *replace* the theme
    for someone who cannot read it, so routing them through the palette would
    delete the feature — but module 48 established that an exemption with
    nothing behind it is an unchecked region, so they are pinned by an exact
    hand-written twelve-colour table, by
    `no_high_contrast_colour_is_a_palette_role`, and by a self-legibility
    floor. `ColorFilter` needs no exemption at all: it is a function of the
    colour it is handed and holds none of its own — and that claim is *checked*
    (`a_colour_filter_introduces_no_colour_of_its_own`: every filter fixes
    black and white, `None` is the identity, alpha is untouched) rather than
    asserted in a comment.
  - **Predicting each defect's catcher before running the sweep found two
    holes that would otherwise have escaped.** `Cx79` (the lens falls back to
    `mantle` instead of `base`) and `Hx79` (the docked strip's underline falls
    back to `blue` instead of the accent) are both role-for-role swaps, which
    the membership sweep is structurally blind to, and the rim test as first
    written only inspected the circular lens's `StrokeRect`. Both were closed
    before the run — `the_lens_is_the_desktops_base_in_both_shapes_and_both_modes`
    is a new test written for the first, and the rim test was extended to the
    docked `Line` for the second. This is the third module in which the
    prediction step, not the sweep, was what found the hole.
  - **Sweep: 25 caught, 0 escaped, 0 never asked** — 42 catcher-slots over 16
    distinct tests, median **1** catcher per defect (min 1, max 5). Every
    touched file restored to its recorded SHA-256.
  - **This is the module where the shared instrument helped least, and the
    reason is worth keeping.** The membership sweep caught 5 of 25 defects
    here (20%), against 9 of 23 in module 48 (39%), and the median catcher
    count fell from 3 to 1. Not because the module is worse tested — nothing
    escaped — but because most of its defects are not *"a wrong colour was
    drawn"*; they are *"the right colour was drawn, from the wrong place"*: a
    fallback that stops following the accent, a chosen colour that is ignored,
    a `follow_*` method that silently does nothing, an alpha that comes back.
    A set-membership instrument cannot see any of those. **The sweep is a
    floor, not a ceiling; every site whose provenance matters needs a
    site-shaped test**, and in this module fifteen of the twenty-five defects
    rest on exactly one.
  - **`Bx79` is a new and sharp instance of that blindness, and it is worth
    stating in general terms because it applies to thirteen modules.** The
    defect replaces the lens fill with the *Latte* `base` literal `0xEFF1F5`.
    The membership sweep cannot reject it in **either** mode: in light mode
    `0xEFF1F5` is `base`, an actual role; in dark mode it is
    `readable_on(Mocha base)`, which this module legitimately declares in
    `derived`. The same is true of its counterpart `0x11111B`, which is Mocha
    `crust` in dark mode and the light-mode ink. So **both `readable_on`
    endpoints sit permanently inside the allowed set, in both modes, for every
    one of the 13 modules that derive them** (§532). Contrast `Ax79`, the
    *Mocha* base literal `0x1E1E2E`, which the sweep does catch — because it
    is a role in one mode only. The declaration was corrected to name the two
    site-shaped tests that actually catch it, with the reasoning recorded at
    the harness entry rather than left as a silent omission.
  - **Two under-declarations folded back.** `Nx79` also breaks
    `following_the_accent_is_reachable_again_after_choosing` (it reads the
    resolved colour too), and `Yx79` also breaks the four older inverter tests
    — the inverter being the most-tested filter in the module. The new filter
    test is still the only instrument that would catch the same stray constant
    appearing in any *other* filter, which is why it was written.
  - **One finding was left for the operator rather than fixed silently.**
    `every_high_contrast_scheme_is_legible_with_itself` measures the four
    highlight colours at 19.56 / 8.59 / 16.75 / **6.70**:1 against their own
    backgrounds. The outlier is "green on black", whose magenta highlight is
    the dimmest colour in the set — and it is dim *for a reason*, being the
    only candidate that stays distinguishable from green under red-green
    colour blindness. Changing it is a user-visible appearance policy, so the
    test's highlight floor was set to 4.5:1 with the outlier named at the site
    and the question queued as `open-questions.md` **C-Q7** (recommendation:
    pale magenta `#FF80FF`, 9.78:1, which keeps the hue and clears the strict
    bar). Answering it would let that floor rise to 7:1 and hold every future
    scheme to it.

**Part 2 is closed.** Measured at the end rather than asserted: `gui/desktop/src`
now contains **zero** `const <NAME>: Color` in production code. The 22 that a
grep still finds are all inside `#[cfg(test)]` modules and all of them exist to
be *off*-palette — `OFF_PALETTE`, `CHOSEN`, `USER_PINK`, `SAMPLED`, `A`/`B` —
which is the opposite defect and the thing that makes the tests able to fail.
The shell draws its colour from one place.

**What the 49 modules taught, condensed.** The single most valuable habit was
not the sweep; it was **predicting each defect's catcher before running the
harness**. That step, not the run, is what repeatedly found tests that could
not fail — an expectation echoing the table it checked (module 47), a fixture
value that equalled the role it was meant to distinguish from (48), a
role-for-role swap no membership test can see (47, 48, 49), a rim assertion
that never reached the second shape (49). The sweep then *confirms* a
prediction rather than discovering the hole, and a `[MISSING:]` becomes
information — it means the prediction was wrong, and finding out which half was
wrong is where the remaining bugs were.

The second habit worth keeping is treating an **exemption as a debt**. Every
"this one legitimately isn't a role" — the dynamic sky, the high-contrast
schemes, the readable-on extremes — is an unchecked region until something
pins it, and twice the exemption itself turned out to be the bug (§532, and
`Bx78`/`Kx78` before it). An exemption now ships with an instrument: an exact
table, a role-collision proof, or a claim about *provenance* rather than value.

**Trigger:** this is not blocked on anything. It is sequenced after the shell
event loop (`TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`) only
because a shell that cannot be driven cannot demonstrate a theme change
either — with the loop in place, changing the mode and watching the desktop
repaint is the test that proves this fixed. It also subsumes
`TD-FOUR-APPEARANCE-SETTINGS-THE-SHELL-STILL-IGNORES`, which is the same
defect observed from the settings end.

**If never fixed:** the appearance settings page stays decorative. A user who
picks Light gets a desktop that is light in five places and dark in every
other, which reads as a broken theme rather than an unimplemented one — worse
than having no setting at all.
