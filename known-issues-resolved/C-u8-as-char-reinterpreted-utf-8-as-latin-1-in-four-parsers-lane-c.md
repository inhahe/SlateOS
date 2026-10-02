## `u8 as char` reinterpreted UTF-8 as Latin-1 in four parsers (lane C)

**Status: FIXED 2026-08-15** (lane C, commits `237636350` kanban, `3b6b60e39`
backup, `18f1e9abc` rssreader). Found while sweeping for byte-at-a-time text
walkers, and it is a *different* class from the byte/character-count confusion
above — worth keeping separate, because the symptom, the detection method and
the fix all differ. Three JSON readers and one XML reader carried it.

`apps/kanban`'s `JsonImporter::parse_string` built its result one byte at a
time:

```rust
} else {
    result.push(b as char);   // b: u8
}
```

`b as char` maps a **byte value** to the Unicode scalar with that value. That is
a Latin-1 decode. There is no count involved, nothing is truncated, and nothing
panics: an imported card titled `日本語` (E6 97 A5 ...) simply comes back as
`æ\u{97}¥...`. It is `String::from_utf8_lossy`'s failure mode reached by a
different route, and it is worse than a panic in one specific way — **the damage
persists.** The mojibake becomes the card's title in memory, and the very next
save writes it to disk as the new truth. Import a board, glance away, and the
original text is gone.

Why the count-confusion sweep would not have found it: there is no `len()`, no
slice, no guard, no wildcard. The tell is the cast itself. The generalisation
worth carrying forward is that **`u8 as char` is almost always a bug on text**;
it is sound only where the byte is already known to be ASCII — which is exactly
the distinction that made `apps/jsonviewer`'s identical-looking line correct.

The fix copies unescaped runs out as whole `&str` slices instead. That is sound
precisely because the two bytes that terminate a run — `"` and `\` — are ASCII,
and an ASCII byte can never occur inside a multi-byte UTF-8 sequence, so the cut
is always on a character boundary. (This is the same self-synchronisation
property that cleared so many near-misses in the sweep above; here it is what
makes the fix work rather than what made the bug absent.) It is also faster than
pushing char by char.

Fixing the function properly turned up two further defects in it:

- **`\uXXXX` was never decoded.** It fell through to the unknown-escape arm and
  came back as a literal backslash, `u`, and four digits. This was not
  hypothetical: our own `JsonExporter::escape_json` emits exactly that form for
  every character below U+0020, so **export followed by import did not
  round-trip** for any card whose text contained a control character. Now
  decoded, including leading/trailing surrogate pairs — which is how JSON spells
  anything outside the BMP, so emoji in a board exported by any other tool were
  equally unreadable. An unpaired surrogate degrades to U+FFFD rather than
  failing the whole import.
- **The unknown-escape arm had the same cast** (`result.push(esc as char)`) on a
  single byte, so a backslash followed by a multi-byte character both corrupted
  that character and left the scan stranded mid-sequence. It now consumes a
  whole character.

Non-vacuity was checked by reinstating the byte-at-a-time parser: all four new
tests fail under it while the ASCII control test and both pre-existing parser
tests keep passing — the profile that shows the new tests discriminate *and*
that the rewrite changed nothing for ASCII.

Violates `CLAUDE.md` self-review item 7 in its strongest form: this is not an
assumption about encoding, it is an actual re-encoding.

### The same bug in `apps/backup`'s manifest reader — the worst instance

`apps/backup`'s `parse_string` had the identical cast, but on data that makes it
far more consequential: **the strings in a backup manifest are file paths.** A
backup of `写真/2024.jpg` reads back as a path naming no file at all, so restore
cannot find it and verify reports it missing. The manifest is the only record of
what was backed up; corrupting it silently invalidates the archive.

Fixing it turned up two further defects in the same function, both worse than
the first:

- **A reachable panic.** The `\u` arm did `&input[i + 1..i + 5]` with no bounds
  or boundary check. On `"\u日本"` that cuts at byte 7, inside `本`, and Rust
  panics on the non-boundary slice. Reachable from merely *reading a manifest
  off disk* — no attacker needed, just a path that happens to follow a
  backslash-u with non-ASCII text. `parse_hex4` now uses
  `input.get(start..end).ok_or("incomplete unicode escape")?`.
- **Silent data loss on astral characters.** The `\u` decode used a bare `u16`
  with no surrogate pairing and `if let Some(c)` with *no `else`* — so an
  escaped emoji or CJK-extension character in a path did not become U+FFFD, it
  simply **vanished**, shortening the path to something else entirely. Now
  paired properly, with U+FFFD for an unpaired surrogate.

Three defects, so non-vacuity was checked with three separate breaks, each
confirmed to fail only its own tests while the ASCII control kept passing. The
last test builds a real `Manifest` of non-ASCII `FileEntry` paths and
round-trips it through `serialize`/`deserialize`. 46 tests pass.

### The same bug in `apps/rssreader` — the only remotely-fed instance

`XmlParser::read_attribute_value` accumulated with `value.push(b as char)` on
bytes straight off a downloaded feed, so any non-ASCII enclosure URL, title or
author arrived as mojibake. What makes this one instructive is that it was the
**outlier in its own file**: `read_until`, `read_name` and the text-node reader
all already sliced the range out whole and used `from_utf8_lossy` — which is
exact here, since `parse_xml` takes a `&str` and every delimiter is ASCII. The
correct pattern was sitting three functions away. Fixed to slice the same way.

Fixing it exposed an unrelated robustness defect in the same path, arguably more
damaging in practice than the mojibake: `decode_entity` returned `Err` for
anything outside XML's five entities, and `read_attribute_value` propagated it
with `?`. So **one `&nbsp;` in any attribute failed the parse and threw away the
entire feed** — as did a bare `&` in a query string, which is ubiquitous in
enclosure URLs. The same entity in a *text node* merely rendered literally,
because that caller fell back to the raw string; only the attribute path was
fatal. `decode_entities` is now infallible: unrecognised entities are emitted
exactly as written, bare `&` passes through, and twenty-six common HTML entities
are decoded rather than left as source text. Two breaks, two disjoint failure
sets. 147 tests pass.

The pattern across all four: **the cast is never the only bug in the function.**
Every site that had it also had at least one other defect in the same escape or
delimiter handling — a panic, a dropped character, or a fatal error on ordinary
input. Byte-at-a-time text handling seems to correlate with not having thought
about the hard cases at all.
