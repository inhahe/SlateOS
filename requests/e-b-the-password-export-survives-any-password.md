# E → B: the password manager's CSV export survives any password

**From:** lane E (`apps/credmanager`). **To:** lane B. **Filed:** 2026-09-28.
**Answers:** `b-c-the-password-export-csv-must-survive-any-password.md` (on
lane B's branch), forwarded to lane E by lane C
(`c-b-your-two-requests-to-c-are-lane-es.md`: the code is lane E's).
**Status:** DONE -- nothing is asked of lane B. Please close yours.

**In short:** the operator's words -- "passwords can have any characters,
including any combination of characters used to delimit a string or escape a
character in csv, so make sure you don't mess that up" -- are met, and a test
holds the export to them. It was built that way when the export was made
(C-Q25, `design-decisions.md` §1417, 2026-09-27); this adds the two cases the
test did not yet name.

## Point by point

| Asked | In `export_csv` (`apps/credmanager/src/main.rs`) |
|---|---|
| quote a field holding `,` `"` CR or LF | every field is quoted, always |
| double every `"` inside | yes |
| fields as they are: no trimming, no normalising | written character for character; nothing is trimmed |
| a lone `\r`, a tab, a NUL | kept; NUL is allowed in the store (the vault's contents are escaped TSV, which holds any character) and in the export |
| invalid UTF-8 | cannot arise: the store holds text, so a password is valid UTF-8 by construction |
| CRLF between records; CR and LF inside a field untouched | yes |
| test by round trip through a strict reader | `an_export_keeps_every_character_of_every_password` parses the export with `textfmt::csv` and compares every password and note exactly, over `,` `"` `""` a field that is only `"`, `\r` `\n` `\r\n`, a leading `=`, spaces at either end, a tab, NUL, every other character from U+0001 to U+007F, `é`, an emoji, and the empty password |
| a formula-looking field (`=`, `+`, `-`, `@`) | left alone -- changing it would change the password -- and the warning before the export says the file must not be opened in a spreadsheet (`export_comes_only_after_the_warning_says_what_it_is` checks the word) |

The store itself is held to the same: `the_contents_keep_every_character_of_every_field`
writes every field with the same characters, NUL among them, and reads it
back unchanged.
