## §404 — Configuration is edited as text, not serialized: a format-preserving YAML document with a line index

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** `design.txt` mandates YAML for configuration, "processed with a
library that preserves comments and formatting", and nothing in the tree could
do the second half — the installer's parser is read-only and lossy, and there
is no YAML crate anywhere in the lock file. Meanwhile
`gui/desktop/src/appearance_settings.rs` had a `save()` that copied one struct
field into another and wrote nothing
(`known-issues.md`, TD-APPEARANCE-SETTINGS-ARE-NEVER-WRITTEN-TO-DISK), and
about twenty other `*_settings.rs` panels have the same shape. Persisting even
one of them required deciding what "preserves comments and formatting" means in
code.

**Decision.** `yamldoc` is not a serializer. A `Document` holds the file's
original lines; a write splices the new value into the one line that holds it,
and every region the edit did not name comes out byte for byte as it went in.
The line classification (`scan()`) is recomputed on demand rather than cached.

Four consequences follow, each chosen deliberately:

1. **Anything not modelled is opaque, not an error.** Anchors, tags, flow
   collections, block scalars and multi-document files are carried through
   untouched but are not indexed, so they cannot be read or written through the
   API. A block scalar's body is explicitly excluded from key indexing, so a
   user's prose containing a colon is never mined for a key.
2. **Parsing is infallible; every getter returns `Option`.** There is nothing
   useful a config reader could do with a "malformed YAML" error: its answer to
   a key it cannot read is the default, which is what `None` says.
3. **Paths are `&[&str]`, never dotted strings.** A dotted path does the wrong
   thing the first time a key holds a font family (`Noto Sans CJK.Bold`) or a
   hostname.
4. **Reading is liberal, writing is conservative.** `get_bool` refuses the
   YAML 1.1 spellings — `NO` is a country, not `false` — but `set_str` quotes
   them anyway, along with hex/octal/underscored numbers. The file is not only
   read back by us: the user's `yq`, their editor's highlighter and the next
   tool to touch it all get a look. The asymmetry is the point.

**Alternatives rejected.**

*Deserialize into a struct, mutate, re-serialize.* The obvious approach, and
the reason `design.txt` calls the requirement out. It is lossy by construction:
it discards every comment, blank line, deliberate ordering and choice of
quoting, and hands the user a machine-flattened version of what they wrote. Do
that once and nobody hand-edits a config again, because their annotations do not
survive the next time they touch a setting in the UI.

*Take a dependency (`serde_yaml`, `yaml-rust`).* Only twelve external crates
exist in the whole lock file, none of them YAML. `serde_yaml` is lossy in
exactly the way above and is unmaintained; a full round-trip implementation
(the `ruamel.yaml` model) is far more machinery than a settings file needs. And
configuration is read where there is no `std` — the service manager and the
package manager both run early — so the crate is `no_std` + `alloc` and
dependency-free, mirroring `tzrules` and `netproto`. A config reader that only
works once `std` is up cannot be used to decide how `std` comes up.

*Cache the line index inside the `Document`.* Rejected on the recurring lesson
of this lane: two loops over the same text coming to different answers is the
bug class that keeps costing the most (§401, §402). The lines are the single
source of truth, and a cached index that can disagree with them makes that
divergence representable. Config files are a few dozen lines and edits happen
when a user clicks Save, so the scan is not on any hot path.

**Cost.** `set_*` is O(lines) per call rather than O(1). A settings panel
writing twenty keys rescans twenty times. Measured against the alternative —
an index that can go stale — this is the right trade at this size, and if a
consumer ever writes thousands of keys the fix is a batch API, not a cache.

**Second decision, same commit: where configuration lives.**
`gui/desktop/src/config.rs` resolves `$XDG_CONFIG_HOME/slateos/<name>.yaml`,
falling back to `$HOME/.config/slateos/`. XDG because the rest of the tree
already assumes an XDG-shaped home (`~/.cache`, `~/.local/share/Trash`) and a
user who set the variable has said plainly where they want their config. With
neither variable set, `store` reports `NotFound` rather than inventing a
location: writing one user's personal preferences into a system-wide path
because `$HOME` was missing is worse than not writing them. Writes go to a
`.new` file beside the target and are renamed over it — rename within a
directory is atomic, so a crash mid-save leaves the whole old file or the whole
new one, never a truncated middle. The temporary is named after the target
rather than randomly, so a crash between write and rename leaves one
identifiable piece of litter the next save overwrites, not an unbounded pile.

**Third: config spellings are not UI labels.** Each settings enum gets a
`yaml_name`/`from_yaml_name` pair (generated by the `yaml_enum!` macro) that is
deliberately separate from `label()`. A label is screen text — "Extra Large
(96px)", "Accent Color" — and changes when the wording is improved or a preset
is retuned. A config spelling is part of the file format: change it and every
existing user's saved choice silently reverts to the default on next start. An
unknown spelling reads as `None` and leaves the field at its default, so a file
written by a newer desktop degrades rather than refusing to load.

**Where it lives.** `yamldoc/src/lib.rs` (the whole crate); the "Configuration
file" section of `gui/desktop/src/appearance_settings.rs`
(`read_from`/`write_into`, `yaml_enum!`, `color_to_hex`/`color_from_hex`);
`gui/desktop/src/config.rs` (`config_dir`, `path_for`, `load`, `store`).
