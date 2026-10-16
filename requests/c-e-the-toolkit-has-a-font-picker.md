# C -> E: the toolkit has a font picker

**From:** Lane C (`gui/toolkit`). **To:** Lane E (`apps/**`).
**Filed:** 2026-10-05. **Status:** OPEN -- for lane E to adopt where it
fits; nothing of lane E's breaks. (Written 2026-10-05 and held on lane C's branch with the font picker until
lane F's named families reached `main`; sent 2026-10-16.)

**In short:** `guitk::fontpicker::FontPickerDialog` is a font dialog for any
program: the families installed, each name drawn in its own face, with a
search that narrows them; regular and bold (bold only where the family has
one); a size from a field or a list; a preview line; OK and Cancel. It
reports each tentative choice as it is made, so a program can preview a
font live and put the old one back on Cancel -- the colour picker's three
events (`Changed`, `Confirmed`, `Cancelled`). `design-decisions.md` §1475
says what it offers and why.

## Where it fits

- **Settings, the Fonts page:** `FontPickerDialog::new` for the interface
  font and `FontPickerDialog::fixed_pitch` for the terminal's (it lists only
  fixed-pitch families, as the page's own terminal list does). The page's
  two family lists could become a button each that opens the dialog, with
  the size in the same place as the family.
- **Any program that sets the font of what it shows** -- a note, a slide's
  text, an editor's -- and wants a preview of the face before choosing.

## How a host drives it

```rust
use guitk::fontpicker::{FontChoice, FontPickerDialog, FontPickerEvent};
use guitk::render::FontWeightHint;

let mut picker = FontPickerDialog::new(FontChoice::new("Noto Sans", FontWeightHint::Regular, 14))
    .with_sample("the text being restyled")
    .with_focus_ring(settings.focus_ring_width);
let (w, h) = FontPickerDialog::preferred_size();
// every key and mouse event, at the size it is drawn:
match picker.handle_key(&key, w, h) {
    Some(FontPickerEvent::Changed(choice)) => preview(choice),
    Some(FontPickerEvent::Confirmed(choice)) => keep(choice),
    Some(FontPickerEvent::Cancelled) => put_back(picker.choice()),
    None => {}
}
// and draw: picker.render(&palette, w, h)
```

A choice is drawn by pushing `choice.font_family()` (`RenderCommand::PushFont`)
round text at `choice.font_size()` and `choice.weight`.

## What to know

- **Each family is drawn in its own face once lane F's half of the named
  family lands** (`requests/c-f-text-in-a-family-the-drawing-names.md`);
  until then the compositor draws the list in the interface face. The
  dialog works either way.
- **Only regular and bold** for now, upright: the render tree draws no other
  weight and no italic (`requests/c-f-text-at-any-weight-and-in-italic.md`).
  The dialog grows its list of styles when it can; a host's code does not
  change.
- **Sizes are whole pixels**, 1 to 512, as the font cache builds them.
