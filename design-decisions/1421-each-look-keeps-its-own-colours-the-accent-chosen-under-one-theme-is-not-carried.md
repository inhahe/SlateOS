## 1421. Each look keeps its own colours: the accent chosen under one theme is not carried into another

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended C, then A; the operator chose A and added keeping colours per theme) &middot; **Lane:** C, with E

**In short:** Under the optional "Filled" look, the accent you chose is drawn
deeper on the grey boxes so its text stays readable, and it can look muddier
than the swatch you picked. The boxes stay as they are. Instead, the accent --
and any other interface colour you set -- is remembered separately for each
look: switching to the Filled look brings back the colours you chose for it
(or its defaults), and switching back brings back the others, so a choice made
for one look never lands on the other.

**The question:** `open-questions.md` C-Q15 (now resolved).

**The operator's answer, verbatim:** "Can't you do A and save accent and
whatever other UI colors per desktop theme, so switching to filled theme won't
keep their previous accent change? Another option, if you think it's better to
carry accent, etc. changes across themes, is to measure all the contrasts
whenever they switch to the filled theme and then if something isn't enough
then ask the user if they want to reset the colors to default for the theme, or
maybe if they want to do color settings per theme (and then reset to the
default for the theme if they do)?"

**Chosen of the two:** colours kept per look. The alternative -- measuring on
every switch and asking -- puts a question in front of the user at the moment
they changed something else, and a colour that is fine under one look and
unreadable under the other is exactly what keeping them apart prevents.

| What | How |
|---|---|
| Where they live | `appearance.yaml`: the accent and the user's other interface colours under the look they were chosen for (Outlined, Filled); a file written before this -- one accent -- reads as that accent for both, so nobody's choice is lost |
| What Settings shows | the colours of the look being edited; changing a colour changes it for that look only -- lane E's page |
| Readability | unchanged: every colour still passes through the palette's legibility floor, under either look |

**As built (lane C, 2026-09-27).** `appearance::LookColours` -- today the accent
and the colour a custom accent names; an interface colour added later is kept
per look by being added to it.

- **In memory:** `accent_color` and `custom_accent` keep meaning "the accent",
  now the look in use's, so nothing that draws changed; the other look's are in
  `other_look_colours`. `set_surface_style` changes the look and trades the two
  over; choosing the look already in use trades nothing. `colours_for(look)` and
  `set_colours_for(look, ..)` reach either look without switching.
- **On disk:** the outlined look's colours stay at `theme.accent` and
  `theme.custom_accent`, where the one accent always was, so a desktop from
  before this still reads the default look's colours from a new file; the
  filled look's are under `theme.cards`. Whatever `theme.cards` does not say is
  taken from the outlined look -- which is the migration: an old file's one
  accent reads for both.
- **The one convention:** assigning `surface_style` directly does not trade the
  colours. The field stays public because a private one would break every
  `AppearanceSettings { .., ..Default::default() }` outside the crate; the
  field's doc says to use the setter, and Settings -- the one program that
  changes the look -- is asked to (lane E).
