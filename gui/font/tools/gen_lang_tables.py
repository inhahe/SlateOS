"""Generate `gui/font/src/lang_tables.rs` from HarfBuzz's own tag table.

    python gui/font/tools/gen_lang_tables.py [--source PATH_OR_URL] [--out PATH]

Why this is generated and not written
-------------------------------------

BCP 47 language tag to OpenType language system tag is a *registry*, not an
algorithm: `tr` is `TRK `, `de` is `DEU `, and no rule derives one from the
other. The registry has about 1100 entries and changes when new languages are
registered, so a hand-copied version would be wrong in ways nobody would find
-- a single mistyped tag mis-shapes one language on one script, silently, in
whichever fonts happen to file rules under it.

HarfBuzz maintains that mapping in `src/hb-ot-tag-table.hh`, generated in turn
from the OpenType registry and the IANA subtag registry. This reads it and
emits the same data as Rust, so that `lang.rs` agrees with the shaper we
measure ourselves against by construction rather than by inspection. See
`tools/harfbuzz_sweep.py` for the same principle applied to shaping output.

What is read
------------

Four sorted arrays, taken verbatim:

* `ot_languages2`  -- two-letter BCP 47 subtag to tag
* `ot_languages3`  -- three-letter subtag to tag
* `ot_languages3_multi` + `ot_languages3_multi_values` -- three-letter subtags
  with several tags; only the first is kept, since selection picks one
  language system and HarfBuzz's own fallback order puts the best first
* `ot_languages3_blocked` -- three-letter subtags where the "uppercase it"
  fallback would produce a tag that means a *different* language, so it must
  not be applied

and the body of `hb_ot_tags_from_complex_language`, which handles tags that
need more than their first subtag -- `ro-MD` is `MOL `, `zh-Hant` is `ZHT `,
anything `-fonipa` is `IPPH`. That function is C control flow rather than a
table, but it is generated C: every branch is one of four shapes, which this
parses back into rules. A branch shape it does not recognize is a hard error
rather than a skipped line -- a silently dropped rule is exactly the failure
this file exists to prevent.
"""

import argparse
import os
import re
import sys

DEFAULT_SOURCE = (
    "https://raw.githubusercontent.com/harfbuzz/harfbuzz/main/src/hb-ot-tag-table.hh"
)

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_OUT = os.path.join(HERE, "..", "src", "lang_tables.rs")

# HB_TAG('A','F','R',' ')
TAG = re.compile(r"HB_TAG\s*\(\s*'(.)'\s*,\s*'(.)'\s*,\s*'(.)'\s*,\s*'(.)'\s*\)")


def read_source(source):
    if source.startswith(("http://", "https://")):
        import urllib.request

        with urllib.request.urlopen(source, timeout=60) as f:
            return f.read().decode("utf-8", errors="replace")
    with open(source, encoding="utf-8", errors="replace") as f:
        return f.read()


def array_body(text, name):
    """The text between `name[] = {` and its closing `};`."""
    start = text.index(name + "[] = {")
    end = text.index("};", start)
    return text[start:end]


def tags_in(line):
    return ["".join(m.groups()) for m in TAG.finditer(line)]


def pairs(text, name):
    """`{HB_TAG(a), HB_TAG(b)},` rows, as (a, b)."""
    out = []
    for line in array_body(text, name).split("\n")[1:]:
        found = tags_in(line)
        if len(found) == 2:
            out.append((found[0], found[1]))
        elif found:
            sys.exit(f"{name}: unexpected row with {len(found)} tags: {line.strip()}")
    return out


def singles(text, name):
    """`HB_TAG(a),` rows, as a list of a."""
    out = []
    for line in array_body(text, name).split("\n")[1:]:
        found = tags_in(line)
        if len(found) == 1:
            out.append(found[0])
        elif found:
            sys.exit(f"{name}: unexpected row with {len(found)} tags: {line.strip()}")
    return out


def multi(text):
    """`ot_languages3_multi` resolved through `ot_languages3_multi_values`.

    Only the first tag of each run is kept: `select` picks one language system,
    and HarfBuzz orders the alternatives best-first.
    """
    values = singles(text, "ot_languages3_multi_values")
    out = []
    for line in array_body(text, "ot_languages3_multi").split("\n")[1:]:
        found = tags_in(line)
        if not found:
            continue
        m = re.search(r"\)\s*,\s*(\d+)\s*,\s*(\d+)\s*\}", line)
        if len(found) != 1 or not m:
            sys.exit(f"ot_languages3_multi: unparsed row: {line.strip()}")
        offset = int(m.group(1))
        if offset >= len(values):
            sys.exit(f"ot_languages3_multi: offset {offset} past values")
        out.append((found[0], values[offset]))
    return out


# The four branch shapes `hb_ot_tags_from_complex_language` is generated with.
# `lang_str` has already been advanced past the switch's own first character,
# which is why the offsets are re-added when the rule is built.
COMPLEX_SHAPES = [
    # if (subtag_matches (p, limit, "-fonipa", 7))
    ("variant", re.compile(r'^if \(subtag_matches \(p, limit, "([^"]+)", \d+\)\)$')),
    # if (lang_matches (&lang_str[1], limit, "do-hant-hk", 10))
    (
        "prefix",
        re.compile(r'^if \(lang_matches \(&lang_str\[(\d+)\], limit, "([^"]+)", \d+\)\)$'),
    ),
    # if (0 == strcmp (&lang_str[1], "rt-lojban"))
    ("exact", re.compile(r'^if \(0 == strcmp \(&lang_str\[(\d+)\], "([^"]+)"\)\)$')),
    # if (0 == strncmp (&lang_str[1], "n-arab", 6)
    #     && subtag_matches (lang_str, limit, "-fonipa", 7))
    (
        "prefix_variant",
        re.compile(
            r'^if \(0 == strncmp \(&lang_str\[(\d+)\], "([^"]+)", \d+\)\s*'
            r'&& subtag_matches \(lang_str, limit, "([^"]+)", \d+\)\)$'
        ),
    ),
]


def complex_rules(text):
    """The body of `hb_ot_tags_from_complex_language`, as ordered rules.

    Order is preserved exactly: the function returns at the first branch that
    matches, so a reordering is a behaviour change. The leading
    `subtag_matches (p, ...)` block runs before the `switch`, and those rules
    therefore come first here too.
    """
    start = text.index("hb_ot_tags_from_complex_language (const char")
    body = text[start : text.index("\n}\n", start)]

    rules = []
    letter = None
    pending = None
    for raw in body.split("\n"):
        line = raw.strip()
        if line.startswith("case '") and line.endswith("':"):
            letter = line[6]
            continue
        if line.startswith("if ("):
            # Multi-line `if`s in this file are always the strncmp/subtag pair.
            condition = line
        elif pending is not None and line.startswith("&&"):
            condition = pending + " " + line
        else:
            if TAG.search(line) and pending is None and rules and rules[-1][-1] is None:
                # The result of the branch opened just above.
                kind, key, extra, _ = rules[-1]
                rules[-1] = (kind, key, extra, tags_in(line)[0])
            pending = None
            continue

        matched = None
        for kind, pattern in COMPLEX_SHAPES:
            m = pattern.match(condition)
            if m:
                matched = (kind, m)
                break
        if matched is None:
            if condition.startswith("if (0 == strncmp"):
                pending = condition  # the `&& subtag_matches` is on the next line
                continue
            if condition in (
                "if (limit - lang_str >= 7)",
                "if (!p || p >= limit || limit - p < 5) goto out;",
            ):
                continue
            sys.exit(f"unrecognized branch in complex_language: {condition}")
        pending = None

        kind, m = matched
        if kind == "variant":
            rules.append(("Variant", m.group(1), None, None))
        elif kind == "prefix":
            rules.append(("Prefix", prefixed(letter, m), None, None))
        elif kind == "exact":
            rules.append(("Exact", prefixed(letter, m), None, None))
        else:
            rules.append(("PrefixVariant", prefixed(letter, m), m.group(3), None))

    missing = [r for r in rules if r[-1] is None]
    if missing:
        sys.exit(f"{len(missing)} complex rules found no tag: {missing[:3]}")
    return rules


def first_per_key(rows):
    """One tag per subtag, sorted by subtag.

    HarfBuzz files a language with several possible tags as consecutive rows
    with the same key, best first -- `ga` is `IRI ` and then `IRT `, Irish and
    Irish Traditional. A run is shaped under one language system, so only the
    first is kept; taking the second would silently prefer the alternative that
    HarfBuzz itself ranks lower.
    """
    seen = {}
    for key, tag in rows:
        seen.setdefault(key, tag)
    return sorted(seen.items())


def prefixed(letter, m):
    """Re-attach the switch's own character to a `&lang_str[n]` key."""
    offset = int(m.group(1))
    if offset != 1 or letter is None:
        sys.exit(f"unexpected lang_str offset {offset} (letter {letter!r})")
    return letter + m.group(2)


def rust_tag(tag):
    if len(tag) != 4 or any(c == '"' or c == "\\" or not 0x20 <= ord(c) < 0x7F for c in tag):
        sys.exit(f"tag {tag!r} is not four printable ASCII bytes")
    return f'*b"{tag}"'


def emit(out, languages2, languages3, blocked, rules, source):
    w = out.write
    w(f"""//! BCP 47 language subtags to OpenType language system tags.
//!
//! Generated by `tools/gen_lang_tables.py` from HarfBuzz's
//! `src/hb-ot-tag-table.hh`, which is itself generated from the OpenType
//! language system registry and the IANA subtag registry. Do not edit: rerun
//! the generator.
//!
//! Source: <{source}>
//!
//! The mapping is a registry and not a rule -- nothing derives `TRK ` from
//! `tr` -- so the only defensible way to have it is to take it from the same
//! place the shaper we measure against takes it. See [`lang`](crate::lang) for
//! how the tables are used and what happens to a language none of them names.

/// How a [`COMPLEX`] rule matches a full BCP 47 tag.
///
/// These come from HarfBuzz's `hb_ot_tags_from_complex_language`, which is
/// generated C rather than a table because its keys are not all of one shape.
/// The four variants are the four branch forms that function is generated
/// with, and they are tried in the order they appear in it: the first match
/// wins, so the order is part of the data.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Rule {{
    /// The whole tag is this, exactly.
    Exact(&'static str),
    /// The tag begins with this, and what follows is `-` or the end. HarfBuzz's
    /// `lang_matches`.
    Prefix(&'static str),
    /// The tag begins with these bytes -- with no boundary required, so
    /// `az-arab` is matched by `az-arab` and by a longer subtag starting the
    /// same way -- *and* carries the second string as a subtag. HarfBuzz's
    /// `strncmp` plus `subtag_matches`.
    PrefixVariant(&'static str, &'static str),
    /// The tag carries this subtag anywhere after its first component.
    /// HarfBuzz's `subtag_matches` against the whole string.
    Variant(&'static str),
}}

/// Tags needing more than their first subtag to resolve, in HarfBuzz's own
/// order. Consulted before [`LANGUAGES_2`] and [`LANGUAGES_3`].
pub(crate) static COMPLEX: &[(Rule, [u8; 4])] = &[
""")
    for kind, key, extra, tag in rules:
        if kind == "PrefixVariant":
            w(f'    (Rule::{kind}("{key}", "{extra}"), {rust_tag(tag)}),\n')
        else:
            w(f'    (Rule::{kind}("{key}"), {rust_tag(tag)}),\n')
    w("];\n\n")

    w("""/// Two-letter subtags, sorted, for binary search.
///
/// The key is the subtag padded to four bytes with spaces, which is how
/// HarfBuzz stores it and what keeps every table here one shape.
pub(crate) static LANGUAGES_2: &[([u8; 4], [u8; 4])] = &[
""")
    for key, tag in languages2:
        w(f"    ({rust_tag(key)}, {rust_tag(tag)}),\n")
    w("];\n\n")

    w("""/// Three-letter subtags, sorted, for binary search.
///
/// Includes the ones HarfBuzz files separately as having several possible
/// tags; only the first of each is kept, since a run is shaped under one
/// language system and HarfBuzz lists the alternatives best-first.
pub(crate) static LANGUAGES_3: &[([u8; 4], [u8; 4])] = &[
""")
    for key, tag in languages3:
        w(f"    ({rust_tag(key)}, {rust_tag(tag)}),\n")
    w("];\n\n")

    w("""/// Three-letter subtags for which the "uppercase it" fallback is wrong.
///
/// The fallback exists because an unregistered ISO 639-3 code and its
/// OpenType tag are usually the same letters in different case. These are the
/// codes where the uppercased form is already *another* language's tag, so
/// applying it would shape one language by another's rules. Sorted.
pub(crate) static BLOCKED_3: &[[u8; 4]] = &[
""")
    for key in blocked:
        w(f"    {rust_tag(key)},\n")
    w("];\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--source", default=DEFAULT_SOURCE, help="path or URL of hb-ot-tag-table.hh")
    ap.add_argument("--out", default=DEFAULT_OUT, help="where to write lang_tables.rs")
    args = ap.parse_args()

    text = read_source(args.source)

    languages2 = first_per_key(pairs(text, "ot_languages2"))
    languages3 = first_per_key(pairs(text, "ot_languages3") + multi(text))
    blocked = sorted(set(singles(text, "ot_languages3_blocked")))
    rules = complex_rules(text)

    # Binary search is only correct on sorted keys, and a duplicate key means
    # two answers for one language -- both are the generator's problem, not the
    # reader's.
    for name, table in (("ot_languages2", languages2), ("ot_languages3", languages3)):
        keys = [k for k, _ in table]
        if keys != sorted(keys):
            sys.exit(f"{name} is not sorted by key")
        if len(set(keys)) != len(keys):
            dupes = sorted({k for k in keys if keys.count(k) > 1})
            sys.exit(f"{name} has duplicate keys: {dupes[:5]}")

    out_path = os.path.abspath(args.out)
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        emit(f, languages2, languages3, blocked, rules, args.source)

    print(f"{out_path}")
    print(f"  complex rules   {len(rules)}")
    print(f"  two-letter      {len(languages2)}")
    print(f"  three-letter    {len(languages3)}")
    print(f"  blocked         {len(blocked)}")


if __name__ == "__main__":
    main()
