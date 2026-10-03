## 883. Text falls back to a chosen list of faces, one per job, resolved from the font directories alone

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** A character the UI font has no drawing for used to come out as
a box. Lane F's font engine can now draw it from another font instead
("fallback"); which fonts those are is decided here. The toolkit tries, in
order: a font with wide coverage (Noto Sans), an emoji font, symbol and maths
fonts, then one font per writing system -- each only if installed, and only
one per job. The UI font itself is now Open Sans, the typeface of the Aero
design the operator made the default look.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| The default UI family | Open Sans first, Inter second (`DEFAULT_UI_FAMILIES`, `FontSettings::default`) | Inter, as §400 had it | §815 made the Aero reference the default theme, and its `font-family` is Open Sans; the image will carry Open Sans and not Inter (`requests/f-cd-...`). |
| The shape of the fallback list | groups of interchangeable families -- "the emoji face", "the CJK face" -- each giving its first installed member (`DEFAULT_FALLBACK_FAMILIES`) | one flat list; or every installed face | every fallback face is parsed in every process that draws text. A flat list with Noto Color Emoji *and* Segoe UI Emoji loads both where both exist, and four regional CJK faces are four times tens of megabytes for one set of characters. Every installed face is the same problem without a ceiling. |
| What the list depends on | the font directories alone (`resolve_fallback_families(&FontDb)`) | leaving out the process's own UI family, or anything else the process chose | the toolkit's cache measures and the compositor's draws; the two must fall back to the same faces in the same order or a line is measured in one face and drawn in another. Anything per-process can differ between the two. The cost: where the UI family is itself on the list (a host whose UI face resolved to Noto Sans) that face is parsed twice and tried once for nothing. |
| A fallback face that will not load | left out; the rest keep their places | the group's next member takes its place | a file that loads in one process and not in another (a descriptor limit, a file replaced between the two scans) should change one face, not shift every face after it. |
| Weights | regular faces only | a bold face per family as well | `osfont` takes a variable fallback face to weight 700 for bold text itself; a static family's bold is a second parsed face for text that is by definition rare. |

### What is not done here

- The compositor's cache is lane F's: `guitk::text::install_fallback_faces`
  is to be called there beside `install_ui_faces`. Until it is, the
  compositor's measuring and drawing of a character only a fallback face has
  can disagree -- which was true of every such character before, as a box.
- Each process parses its own copy of each fallback face (`known-issues.md`
  `TD-C-EVERY-PROCESS-PARSES-EVERY-FALLBACK-FACE`).
