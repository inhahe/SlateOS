## 1461. The shell's own dialogs wear the theme's window frame, starting with the run box

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** The run box (the "Run" dialog) looked like nothing else on the
desktop: a rounded box with a bold heading, the same under every theme. It
now wears the frame the theme gives windows -- a title bar with its title and
a close button, the border and the shadow -- and changes when the theme's
window frames do. Under the built-in theme it is the size it always was, and
its new close button does what Cancel does. The shell's two other
window-like dialogs -- the security prompt and the print dialog, neither shown
yet -- take it when they are wired up (`known-issues.md`
`TD-C-THE-SHELLS-OTHER-DIALOGS-DRAW-FRAMES-OF-THEIR-OWN`).

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where the frame comes from | the window-decorations axis (§1456), drawn by `desktop::dialog_frame` with `DecorationStyle::title_bar`'s geometry | a dialog style of the shell's own | a dialog and a window side by side should look like one desktop; one geometry also means a dialog is clicked where a window would be |
| The title's weight | the theme's (regular, as the built-in frame's) | bold, as the box's heading was | the box's heading was a heading; on a title bar it is a title, and titles are written as the theme writes them |
| The close button | a face in the theme's shape, with no mark, as the compositor draws its buttons; a glyph theme's cross | a drawn cross on every face | the same button a window has, until lane F draws marks on windows' buttons too |
| The box's size | the content fixed, the frame around it: the box grows with a theme's taller bar | the box fixed, the content squeezed | the field and the buttons keep their room under every theme; the box is centred as a whole either way |
| The other dialogs | the security prompt and the print dialog when each is wired up, tracked; not the clipboard flyout or the shortcut card, which are not windows | all of them now | both are orphans that nothing shows yet, so converting them now changes nothing anyone sees; a flyout wearing a window's title bar would be wrong |
