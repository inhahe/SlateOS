## 1456. Window frames are a theme axis: the title bar, its buttons, the border, the shadow

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** What a window's frame looks like -- how tall its title bar
is, where the title sits, which end the minimise, maximise and close
buttons are at and in what order, their shape and size, the border and
the shadow -- can now come from a theme, chosen on its own as the colours,
the controls and the motion already are. One theme can give a macOS-like
frame (buttons on the left, round, the title centred) while another gives
the colours. Until the compositor draws from it (lane F), windows look as
they always have, and the built-in frame is exactly that look.

**What was there.** The compositor drew every frame from five constants of
its own (`TITLE_BAR_HEIGHT` and the rest); the theme format named a
window-decorations axis and nothing read one (`roadmap-detailed.md` ->
*Themes and Appearance*: "Title bar: height, button layout ...").

**How it works:** `appearance::decorations::DecorationStyle` (the shape;
`AERO` is today's frame), the theme's `window-decorations` section
(`themes::DecorationTheme`, read as every section is: a setting left out
keeps the built-in one, one not understood is listed for the author),
`theme.decorations` in `appearance.yaml`, and
`AppearanceSettings::decorations()`. `DecorationStyle::title_bar` gives
the buttons' and title's places on a bar, so the compositor, its hit
tests and a preview in Settings agree. Lane F draws from it
(`requests/c-f-draw-window-frames-from-the-theme.md`); lane E chooses it
(`requests/c-e-choose-the-window-frames-in-settings.md`).

**Choices:**

| Choice | Taken | Alternative | Why |
|---|---|---|---|
| Where the shape lives | the appearance crate, beside the frame's colours (`DecorationColors`) | the toolkit, beside `WidgetStyle` | the compositor draws frames and already reads appearance for the corner radius; no toolkit control has a frame |
| The geometry | computed here, `title_bar()` | left to the compositor | the buttons are drawn in one place and clicked in another, and a preview draws them a third time: one function, or three answers |
| The button order | the three, each once, or the whole order refused | any subset (a theme without minimise) | every window needs all three; a theme cannot take a function away from the user. Which button an order with a duplicate meant to leave out cannot be guessed |
| A window without maximise | the others close up | a gap where it would be (today's) | a gap is a button that is not there; asked of lane F in case the gap was deliberate |
| Sizes out of range | the nearer end, with a note | refused | the direction is plainly what was meant -- the widget-style axis's rule |
| A title too long for its bar | `title-bar.overflow`: clip, ellipsis (the default, today's) or keep-tail, one setting for window titles and their taskbar labels | a setting of each | `design.txt` asks the two to share the vocabulary "so tabs and titles behave identically"; one setting is the plainest way to make sure they do |
| What is not here | the frame's colours, the corner radius, the shadow's colour, blur and offset | -- | colours are the colours axis's and the radius is already a setting (`window_corners`); the shadow is one reach until the compositor's shadow has the other dimensions to set |
