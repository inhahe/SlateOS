## 1414. The toolkit has one push button, the reference's, in the theme's colours

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** Every button was drawn where it was used, and no two alike: the
toolkit's dialogs had flat blue or grey slabs, the Open and Save window flat
fills with labels placed by guessed widths, the Run box its own. There is one
button now (`guitk::button`), drawn as the Aero reference draws one (its
search dialog's `aero-srch-btn`): a face brighter across its upper half, a line
round it, the label in bold; the button that does what the dialog is for
tinted with the accent, one that destroys something tinted red, a disabled one
grey. The dialogs, the Open and Save window and the Run box all use it.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| The colours | the palette's: `surface1` for the face, tinted towards the accent or red; pale tints on a light ground | the reference's blues | the operator's rule (§815): what is themeable follows the theme; the reference's shape is kept, its colours are the theme's |
| The label's colour | the theme's text colour where it reads on both halves of the face, else black or white -- and a face on which no one colour reads is tinted further until one does | the theme's text colour, always | the gloss makes the face two colours; near the point where dark text stops reading and light text starts, one of the two halves fails either way, and which themes put a face there cannot be known in advance -- a test holds every kind and state, both modes, four grounds, to the text floor |
| Primary on a light ground | the accent's pale form | the accent itself | a light-mode accent is deepened until it reads as text, so a face tinted towards it goes dark enough to lose a dark label and flip to white between hover and press |
| Geometry | the caller's: a dialog lays out its own row; the button draws into the rectangle given | fixed button sizes | three layouts already exist with their own heights and their tests; `button::width` is the one measure for a label's width |
