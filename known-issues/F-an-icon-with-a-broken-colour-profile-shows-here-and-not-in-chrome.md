### [F] An icon with a broken colour profile shows here, and not in Chrome -- 2026-09-25

**Status:** OPEN — lane F's; a known divergence, low priority.

**In short:** an icon image whose header says it carries an embedded colour
profile, and whose profile is garbage, is refused by Chrome and shown here.
No real icon does this; it takes a damaged or hand-made file.

**Where.** `gui/imagecodec/src/ico.rs`, `decode_bmp`: the profile is checked
to be present and non-empty, not parsed. Chrome parses it with Skia
(`skia::ColorProfile::Make`) and fails the icon if that fails.

**The proper fix,** when the crate gains colour management (see the crate's
docs on PNG's `iCCP`): parse the profile there, refuse the icon when it will
not parse, and apply it when it does. Parsing it only to refuse it, before
anything uses profiles, would mean an ICC parser whose one job is to agree
with skcms on what is malformed -- a second port for a file nobody has.
