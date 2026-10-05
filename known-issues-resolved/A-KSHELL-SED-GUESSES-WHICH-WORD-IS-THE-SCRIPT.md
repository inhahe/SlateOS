## `A-KSHELL-SED-GUESSES-WHICH-WORD-IS-THE-SCRIPT` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25 (`ba47c7086`)

**Where.** `kernel/src/kshell.rs` — `classify_sed_args` (~123228).

**Correction to this entry before the fix is described.** Row 3 of the table
below, as originally written, was **wrong about GNU**, and the correction is
worth more than the row was. It claimed `sed -n stuff.txt other.txt` should
answer `no command specified`. It should not: GNU takes the first operand as
the script *unconditionally*, so GNU also compiles `stuff.txt` as an `s`
command and also applies it to `other.txt`. That example is a case where the
shape guess and the positional rule happen to **agree**, not a divergence —
picked, embarrassingly, because it reads alarmingly rather than because it was
checked. The row is replaced below with the case that is genuinely wrong, which
is the same one reversed: `sed -i notes.txt 2d`.

The lesson is the entry's own subject matter turned on itself. "It starts with
`s`, so it is a script" and "it looks like a divergence, so it is one" are the
same move.

**What.** `sed` has no option parser. It has a `match` over three known flags
and, for everything else, a guess about *shape*:

```rust
s if s.starts_with('s') || s.starts_with('/') || s.ends_with('d') => {
    if out.scripts.is_empty() { /* it's the script */ } else { /* it's a file */ }
}
_ => out.files.push(part.clone()),
```

Three wrong answers fall out of that, all of them exit-0 or worse.

| typed | what it does | what it should do |
|---|---|---|
| `sed -e 's/a/b/' -e` | runs the first expression, exit 0 | `-e option requires an argument` |
| `sed -q -i 's/a/b/' f` | **rewrites `f`**, complains that `-q` is not a file | refuse the option and touch nothing |
| `sed -i notes.txt 2d` | takes `2d` as the script (it ends in `d`) and **rewrites `notes.txt`** | `notes.txt` is the script; `unknown command`, edit nothing |
| `sed -i.bak 's/a/b/' f` | ignores `.bak`, treats it as plain `-i` | either honour the suffix or refuse; never silently drop it |

**Why, one at a time.**

1. **`-e` with no argument is dropped in silence.** The arm advances the index
   and pushes only `if let Some(script) = parts.get(i)`. With nothing there it
   pushes nothing and the loop moves on. `sed -e` *alone* is caught downstream
   by the "no command specified" check, which is what has hidden this: the
   failing case is `-e` given **after** a good expression, where the script list
   is already non-empty. The user wrote an expression they meant to supply, got
   a partial run, and got exit 0 to say it went fine.

2. **An unrecognised option becomes a file operand.** There is no rejection path
   at all, so `-q` falls to the catch-all and is opened as a file. That produces
   an error — but the *wrong* one, and, decisively, **not before the other files
   are processed**: the loop in `cmd_sed` `continue`s past a file it cannot read.
   Under `-i` that means an unrecognised option still rewrites every file that
   does exist. GNU refuses the invocation and edits nothing. This is the one with
   a real cost attached: the edit is not recoverable.

3. **A file and a script can swap places.** The shape guess has no notion of
   position, so which operand becomes the script depends on how the two words
   are *spelled*, not on which came first. `sed -i notes.txt 2d` is the case
   that costs something: `2d` ends in `d`, so it wins the script slot, and
   `notes.txt` — the word GNU would have compiled and choked on — becomes a
   file that `-i` then rewrites. The user mistyped the argument order and got a
   successful in-place edit of the file they were describing.

   The reverse ordering is where the guess is accidentally right, which is why
   it survived: `sed -n stuff.txt other.txt` takes `stuff.txt` as the script,
   and so does GNU. Agreeing in the common ordering is exactly what let the
   disagreement in the uncommon one go unnoticed.

   The `-f` case is the same defect without the reordering: `sed -f script.sed
   f` sends `-f` to the file list and `script.sed` to the script slot.

**What the proper fix looks like.** The same shape `parse_tr_args` already has,
which is the pattern this file has been converging on:

- A real flag loop: `--` ends the options, `--long` forms named, short options
  bundled (`-ni` is `-n -i`), and **anything unrecognised is an error** rather
  than data. `-E`/`-r`, `-f`, `-s` and `-z` are real `sed` options this shell
  does not implement, and each should be refused by name — with, for `-E`, the
  note that the dialect is BRE (`sed_dialect_note` already has the wording).
- `-e` requires its argument. `MissingArgument('e')`.
- **The script is positional, not shape-matched**: if no `-e` was given, the
  first non-flag operand is the script and every later one is a file. If `-e`
  *was* given, every non-flag operand is a file. That is POSIX's rule and GNU's,
  and it removes the guess entirely.
- Errors carried out as a `SedArgsError` enum with a `report()`, the way
  `TrParseError` and `SedParseError` are, so `cmd_sed` and `cmd_sed_input`
  cannot drift apart on which ones they mention.

**Not a regression.** All of them have been true since the command was written.
Item 2 is the one to fix first if they are ever split up, because it is the only
one that destroys data.

**Related.** Same root as
`TD-KSHELL-COMMANDS-TAKE-A-FLAT-STRING-NOT-ARGV`: `sed` is already on
`command_parses_own_quotes`, so the *words* are right — what is missing is the
grammar over them.

### Fixed — `ba47c7086`

`classify_sed_args` returns `Result<SedArgs, SedArgsError>` and chooses by
position. Options first, `--` ends them, short options bundle, `-e`/`-i` take
attached or following arguments, long options take `=VALUE` or a following
word — and then, **if and only if no `-e` was given**, the first operand is the
script and every operand after it is a file.

| was | is |
|---|---|
| unknown short option → file operand | `sed: invalid option -- 'q'`, exit 1, nothing opened |
| unknown long option → file operand | `sed: unrecognized option '--quite'`, exit 1 |
| `-e` with nothing after it → dropped | `sed: option requires an argument -- 'e'` |
| `-i.bak` → suffix discarded | refused by name, with what the suffix would have meant |
| `-E`/`-r`, `-f`, `-s`, `-z` → file operands | refused each with its own reason |
| script chosen by first letter | first operand, or every operand is a file under `-e` |

`-E` is worth singling out: ignoring it is not a missing feature but a wrong
answer, because these patterns are BREs, so `a+` silently stops meaning
"one or more `a`" and starts meaning "an `a` followed by a plus sign" — and
matches things. Refusing says so.

**What it did not fix.** Nothing about the *script* grammar. A bare `p` is
still `unknown command` and `s///p` is still an unknown flag; the parser only
decides which word is handed to `parse_sed_command`. For kshell's current
grammar every *valid* script also satisfied the old shape test, so the payoff
here is entirely in the option loop and the reorder case — which is why rung 53
tests those and not new scripts.

**Covered by** rung 53, `sed takes its script by position, not by shape`. The
reorder case is run against a real file in `/tmp` and the file's bytes are
compared afterwards, because "refused" and "rewrote it anyway" are
indistinguishable from the diagnostic alone.
