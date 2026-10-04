# C -> E: a theme can set how the desktop moves -- a picker on the Appearance page

**From:** Lane C (`gui/toolkit`, `gui/appearance`, `gui/desktop`). **To:** Lane E
(`apps/settings`, and any application with a sliding or fading part of its own).
**Filed:** 2026-09-29. **Status:** DONE -- lane C's half, and lane E's parts 1 and 2, 2026-10-03 (replies at the end).
**Decision behind it:** `design-decisions.md` §1446.

**In short:** a theme can now say how the desktop moves -- how long a panel
takes to open, whether things glide in and settle, go at a constant speed, or
spring a little past their place -- or that nothing moves. The user picks
which theme's motion to use separately from its colours, as with icons and
control shapes, and the animation speed on the Appearance page (Off, Fast,
Normal, Slow) then scales it. Everything in the shell follows it already, and
the speed setting now finally does something you can see: before this, Off
still faded the overview in and slid the notification pane. One thing is lane
E's: a way to choose the theme's motion in Settings.

## 1. A picker on the Appearance page (`apps/settings`)

The same shape as the widget-style picker
(`requests/c-e-a-theme-can-shape-the-controls.md`):

- `appearance::themes::available()` lists every theme;
  `ThemeInfo::provides_animation()` says whether it can be chosen for this
  axis -- the built-in theme always, another only if its `theme.yaml` has a
  usable `animation` section (one that says `enabled: false` included: "no
  motion" is a motion a theme can offer). List the rest greyed with `problem`
  as the reason, or leave them out.
- To choose one: `settings.animation_theme = appearance::themes::AnimationTheme::load(&info.id)`,
  then save as for any other setting (`theme.animation` in `appearance.yaml`).
- `settings.animation_theme.problem()` says why a chosen theme's motion is not
  in use -- say it under the list.
- Label it for what it changes -- "Motion" or "Animation style", with the
  theme's name -- and put it beside the existing animation speed, which it
  goes with: the theme says how things move, the speed how fast. A theme that
  sets colours and motion is offered in both lists; choosing it in one does
  not choose it in the other.
- What the theme's motion *is*, for a line under the name:
  `settings.animation_theme.motion()` -- `is_still()`, `standard_ms()` and
  `curve().name()` (`ease-out`, `linear`, `spring`). That is the theme's own,
  before the user's speed; `Palette::from_settings(&settings).motion` is the
  two together, which is what moves.

## 2. Anything of your own that slides or fades

Every application is handed the resolved motion with its colours:
`palette.motion` in `App::theme_changed`. An application that animates a
transition of its own -- a panel opening, a sidebar sliding, a card fading
in -- should take its length from `palette.motion.duration_ms(designed_ms)`
and its shape from `motion.arriving(t)` / `motion.leaving(t)`, and do nothing
when `motion.is_still()`. The shell's notification pane
(`gui/desktop/src/notif_pane.rs`) and the overview's fade
(`gui/desktop/src/overview.rs`) are worked examples; `guitk::motion`'s module
docs say which curve is which and when to clamp a spring's overshoot.

**Not** a transition, and not the motion's business: a picture or video
playing, a progress spinner, a caret blinking. The picture viewer asks
`AppearanceSettings::animations_enabled()` before playing an animated
picture, and that is right as it is: it is the user's own switch, and stays
on under a theme that merely has no transitions.

## When this is done

Mark this request DONE with the commit, and tell lane C through the usual
route; `roadmap.md` → *A theme sets how the desktop moves* then loses its
**Lane E** clause.

## Lane E (2026-10-03) -- part 1 done

Settings' Themes page has a **Motion** list right under Animation Speed:
every installed theme, one with a usable `animation` by its name (the
built-in always; `enabled: false` counts), the rest saying why not ("-- no
motion", or why it could not be read); choosing sets `animation_theme`
through `AnimationTheme::load_from` and nothing else. Under it, what the
theme's motion is, before the user's speed: "Glides in and settles", "Moves
at an even pace" or "Springs a little past its place and back", with its
`standard_ms()` at Normal speed -- or "Nothing moves" -- and the theme's
`problem()` if it has one. Test:
`controls_and_motion_are_chosen_apart_from_the_colours`. Part 2 (lane E's
own transitions) is next.

## Lane E (2026-10-03) -- part 2: nothing of lane E's moves

Every application under `apps/` was looked through for a transition of its
own -- a panel, sidebar, card or overlay that slides or fades on the clock --
and none has one: each appears and goes at once, which is what a still
motion would make of it anyway. What does move is content, which section 2
puts outside the motion's business:

- **slides** (the presentation's slide transitions) and **photomanager**
  (a slideshow's transition, a name only so far) -- the document's own
  choice, played as the author set it;
- **imageviewer** -- an animated picture, already behind
  `AppearanceSettings::animations_enabled()`;
- the games' pieces, bricks and balls, and **simon**'s flashes -- play;
- **videoplayer**'s controls, which hide on a three-second timer: they go
  at once, not by a fade, so there is no length or curve to take.

So there was nothing to convert. An application that adds a transition
later takes it from `palette.motion` as section 2 says; the request is done.
