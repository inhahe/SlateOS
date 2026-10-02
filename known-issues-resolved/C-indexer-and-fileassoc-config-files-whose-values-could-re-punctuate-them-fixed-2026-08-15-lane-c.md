## indexer and fileassoc: config files whose values could re-punctuate them (fixed 2026-08-15, lane C)

The same audit again, on the two remaining `key = value` config formats. Both
bugs are silent-wrong-result rather than crash-or-corruption, and one of them
defeats a security control.

### indexer — a comma in a path defeated an exclusion

`/etc/indexer.conf` stored `index_paths`, `exclude_paths`,
`include_extensions` and `exclude_extensions` as **comma-joined** lists. On
this system a path may contain any byte but `/` and NUL, so a comma is an
ordinary filename character. Excluding `/home/u/Private, Ltd` wrote one line
that read back as *two* entries — `/home/u/Private` and `Ltd` — neither of
which named the directory the user meant. The directory was therefore not
excluded, and with `index_contents` on, its contents were read into a
searchable index.

That is the part worth stating plainly: `exclude_paths` is not a preference,
it is the mechanism by which a user keeps a directory out of a system-wide
search index. A format that cannot represent the user's answer is a format
that silently overrides it.

Fixed by giving each entry **its own line** — `index_path = …` repeated —
rather than escaping the comma. Escaping a comma works; a separator that never
appears is better than one that is escaped correctly, and it keeps an ordinary
config readable. The plural keys still parse for hand-written files, and the
first repeated key clears the built-in defaults so a config can shrink the list
and not only grow it. A related loss fell out of the tests: an explicitly empty
list used to reappear as the built-in defaults on the next read.

### fileassoc — the exporter and the importer disagreed, and nothing said so

`from_config_line` trimmed both halves; `export_config` wrote the raw strings.
An extension registered as `"txt "` is registerable — `register_file_type`
validates nothing and `set_default_app` only lowercases — so it exported as
`txt =myapp` and read back as `txt`, **silently reassigning a different
extension's default application**. No error is reported on any path: the line
parses, the extension exists, the app exists and supports it, so every
validation the importer performs passes.

`#` had the same shape in the other direction. A comment line is skipped
entirely, so an extension of `#txt` exported to a line the importer discards,
losing the association without a word.

Both are fixed by escaping through `textfmt::kv` with `=` and `#` named as the
grammar's structure characters, and by having `export_config` call
`Association::to_config_line` instead of keeping a second copy of it inline.
That second copy is the real lesson here: the writer and the reader were
*already* a matched pair on `Association`, and the drift happened because
`export_config` bypassed the writer and open-coded the format a third time. A
format with two writers has no invariant, only a coincidence.

### The band-aid, and where the escaper now lives

By fileassoc this was the fourth app in a row needing the same line-value
escaper, and the third place it had been written inline. Per CLAUDE.md's rule
about band-aid accumulation, it was extracted rather than copied again.

The extraction was not to `guitk`, where `csv` and `escape` already lived. The
components with the strongest need for these primitives turn out to be exactly
the ones that must not depend on a widget library: `apps/backup`,
`apps/indexer` and `apps/installer` are headless, and are three of the four
`apps/` crates with no `guitk` dependency. Unable to reach the shared
escapers, each had grown its own — which is the whole mechanism by which the
duplication happened. So the modules moved to `textfmt`, a dependency-free
`no_std` crate alongside `yamldoc` and `tzrules`, and `guitk` re-exports them
under their original paths so the 137 applications that say `guitk::csv` did
not have to change.

Two invariants are now documented in one place instead of being rediscovered:

1. **Decode in a single left-to-right pass, never a chain of `str::replace`.**
   Undoing `\n` before `\\` turns the two-character text `\n` — a legal
   directory name here — into a real newline. A single pass structurally cannot
   make that mistake, because it never re-examines what it has produced.
2. **An escape must not end in whitespace.** These parsers trim the value,
   which is the right leniency for a hand-edited file, but it means writing a
   trailing space as `\ ` leaves the file ending `...\`, which decodes to a
   stray backslash. Hence `\s`.

### Break-testing

Five breaks on fileassoc, each caught by a named test: removing the escape on
write, the unescape on read, the escape-aware split, `#` from the meta
character set, and routing `export_config` around `to_config_line`. That last
one is the break that reproduces the original bug exactly, and it is worth
keeping precisely because it will fail again the moment someone re-inlines the
format for convenience.
