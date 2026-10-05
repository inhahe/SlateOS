### TD-OILS-PRINTF-Q-HIGH-BYTES. `%q`/`@Q` deliberately disagree with the reference bash about which *characters* are printable — 2026-08-03 — ⚠️ **KNOWN DEVIATION, NOT A BUG** — narrowed 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — `needs_ansi_c_quote` (the shared
predicate), its four callers `printf_quote`, `shell_quote`,
`quote_declare_value`, `quote_declare_key`, and `ansi_c_quote` (the renderer).
Rationale in `design-decisions.md` §101, narrowed by §104.

bash decides between showing a value as itself and showing it as `$'…'` by
asking `ansic_shouldquote` (lib/sh/strtrans.c), which walks the value and
answers yes as soon as a character is not `isprint` — a **locale** question,
and one the C libraries do not agree on.

**What was a deviation and is now fixed (2026-08-07).** Two parts of the answer
are host-independent, and osh was getting both wrong:

* a **byte that decodes to no character**. Every libc agrees: bash reaches this
  through `ansic_wshouldquote`, which quotes when `mbstowcs` fails. osh passed
  the raw byte through. Fixed — `a\xffb` is now `$'a\377b'` in all four
  surfaces.
* a **non-ASCII Unicode control** (`U+0080`–`U+009F`). osh's `%q` tested
  `is_ascii_control` on *bytes*, so `U+009F` — written `\302\237` — was invisible
  to it. Fixed, and the escape is written per byte as bash writes it
  (`$'a\302\237b'`, not `$'a\237b'`), because `ansic_quote` decodes to decide but
  writes the original bytes back.

Q38/§104 is what made these bugs rather than a fork: with osh declared UTF-8-only
and the corpus measured against a UTF-8 bash, "does this decode?" finally has one
answer. Pinned by
`tests/corpus/a-byte-that-is-no-character-forces-the-ansi-c-quoting-form.sh`.

**What remains a deviation.** Everything past those two is a libc's character
database, and §101's decision not to model one still stands. Measured against
the reference bash (newlib) in a UTF-8 locale, osh does *not* quote:

| code point | category | reference bash | osh |
|---|---|---|---|
| `U+00AD` soft hyphen | `Cf` | `$'a\302\255b'` | raw |
| `U+200B` zero-width space | `Cf` | `$'a\342\200\213b'` | raw |
| `U+2060` word joiner | `Cf` | `$'a\342\201\240b'` | raw |
| `U+FEFF` BOM | `Cf` | `$'a\357\273\277b'` | raw |
| `U+E000` private use | `Co` | `$'a\356\200\200b'` | raw |
| `U+0378` unassigned | `Cn` | `$'a\315\270b'` | raw |

…and the disagreement runs the other way too: `U+2028`/`U+2029` (`Zl`/`Zp`) are
printed raw by newlib but are non-printable to glibc. `U+00A0`, `U+2007`,
`U+3000` (`Zs`) and combining marks are printable to both, and to osh.

That last row is the whole argument: there is no single "correct" table to
encode. `Cf` and `Co` would be short enough to carry, but `Cn` (unassigned) is a
full Unicode assignment table that drifts with every Unicode release, and
picking newlib's answers for `Zl`/`Zp` would be picking a host artifact over
glibc's.

**Observable difference:** the table above, in `printf %q`, `${v@Q}`,
`declare -p` values and `declare -p` associative keys.

**Consequence for the corpus:** a differential case must not put a format,
private-use or unassigned code point through any of those four. Bytes that are
no character, and controls, are now fair game and are covered.

**What would change this:** a decision to carry a Unicode general-category
table. That is a bigger commitment than it looks — see the `Cn` point above —
and it would still have to choose between newlib and glibc on `Zl`/`Zp`. §101's
"how to reverse" is the seam: `needs_ansi_c_quote` is now the single predicate
all four surfaces ask, so a category model has exactly one place to land.
