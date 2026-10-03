## TD-B-TWO-BINS-DECLARE-ALIASES-UNDER-A-NAME-THE-GATE-CANNOT-SEE (lane B, 2026-09-12)

`scripts/getopt-ambiguity-check.py` reads a bin's alias table with

    ALIAS_HEAD_RE = re.compile(r"const\s+ALIASES\s*:\s*&\[\(&str,\s*&str\)\]\s*=\s*")

— the constant must be named exactly `ALIASES`. **`chmod.rs` and `chown.rs`
name theirs `LONG_ALIASES`**, so their `("silent", "quiet")` rows are invisible
to the gate and both bins are unchecked on the ambiguity axis.

Found while fixing `date`: I wrote `LONG_ALIASES` by copying `chown.rs`, and the
gate went on reporting the same disagreement with the table sitting right there.
Renaming to `ALIASES` cleared it.

Neither bin is currently *mis*-reported, because the gate falls back to comparing
names and `--silent`/`--quiet` share no prefix that collides. But that is luck,
not coverage: the day one of them gains an alias whose prefix matters, the gate
will report a disagreement that the source already answers.

**The fix is one regex** — accept `(?:LONG_)?ALIASES` — plus deciding which
spelling the tree wants and making the other three agree. It is in a shared
script and touches two lanes' reading of a gate, so it is filed rather than
done in passing.
