## 1008. The operator's grep features keep GNU's short flags and take long-form spellings; and the proximity rule, read out of the source rather than the example

**Date:** 2026-09-09
**Lane:** B
**Decided by:** Claude (autonomous), on a constraint the operator supplied

**In short:** the operator asked for their own `grep`'s extra features to be
added to SlateOS's, "so that it has all the GNU grep features plus my
additions". Four of their flags already mean something else in GNU grep, so
"all the GNU features plus mine" cannot be spelled the way their tool spells
it. Their sentence decides which side gives way: GNU's meanings keep the short
flags, and the additions get long names. Nothing is lost at the command line,
because their tool already has a config file that can alias the short forms
back.

**Where this came from.** `§919` records the operator's request; lane A then
read their source, found the collisions, and filed
`requests/a-b-the-operators-grep-has-features-ours-lacks-and-four-of-them-collide-with-gnu-flags.md`
with a proposed resolution. This entry adopts it. Lane A's own note is worth
repeating: §919 said lane B owned the port and nobody filed anything, so for two
days the request lived only in a decisions file that lane B had no reason to
re-read.

### The collisions and the resolution

| Addition | Operator's spelling | Collides with | Resolved spelling |
|---|---|---|---|
| proximity matching | `-P NUM` | `-P` = `--perl-regexp` | `--near NUM` (see below) |
| filename globs | `-f PATTERN` | `-f` = patterns from file | `--name PATTERN` |
| case-sensitive filenames | `-c` | `-c` = `--count` | `--name-case-sensitive` |
| conjunction across patterns | repeated `-e` | `-e` repeated = alternation | `--every-pattern` (opt-in) |

The last is the sharp one, and the reason this is a decision rather than a
rename: it is not a spelling clash but an **opposite meaning on identical
syntax**. `grep -e a -e b f` prints lines matching either under GNU and prints
nothing unless the file contains both under the operator's. Silently choosing
either would make a command that already appears in scripts mean something
new, so conjunction becomes opt-in and alternation stays the default.

### The name is `--every-pattern` and not `--all-patterns`, because the shorter one broke an abbreviation

Written first as `--all-patterns`, which is the better name and could not be
used. `scripts/getopt-ambiguity-check.py` refused the push and gave the reason:

```text
grep: --a we say ambiguous, GNU resolves it;
      matches ['after-context', 'all-patterns']
```

GNU grep has exactly one long option beginning with `a`, so `grep --a 3 file`
resolves to `--after-context` today. A second `a` option makes that
abbreviation ambiguous, and an abbreviation that works now would stop working
— which is precisely the GNU behaviour this decision promised would survive.

`--every-pattern` has no such prefix. GNU grep has four options starting with
`e`, so `--e` is already ambiguous on both sides and stays that way; nothing
starts with `ev`, so every deeper prefix is one GNU rejects today and we accept
now. That is a divergence rather than a regression: nothing that worked stops
working.

**The general rule, worth more than this instance:** a new long option may not
share a prefix with a GNU option unless that prefix is *already* ambiguous in
GNU. The safe construction is a name whose first letter GNU spends on two or
more options, diverging from all of them before the depth at which GNU resolves
uniquely.

The gate now carries an `INTENTIONAL_EXTRAS` table so that a deliberate non-GNU
option is not reported as a transcription error. Its exemption is deliberately
narrow: it permits only "we resolve, GNU has never heard of it", never "we made
ambiguous something GNU resolves". That was verified rather than reasoned about
— re-adding `all-patterns` *with an exemption in place* still fails the gate on
`--a`.


### `--proximity` was also unusable, by this entry's own rule

The rule above was written after `--all-patterns` broke `--a`. Applying it to
this entry's *other* proposed name shows the same defect, caught this time
before any code was written:

```text
$ echo hello | grep --p hello
hello
```

`--p` **resolves** in GNU grep — `--perl-regexp` is its only `p` option — so
`--proximity` would have made it ambiguous and broken a working abbreviation.
The name is `--near NUM`. GNU spends six options on `n`, so `--n` is already
ambiguous on both sides; nothing starts with `ne`, so every deeper prefix is
one GNU rejects today and we accept now.

Worth noting that both names lane A proposed and this entry adopted were
unusable for the same reason, and neither of us checked. The rule is cheap to
apply and was not applied until a gate applied it for us.

### The README's equivalence claim is false, measured

The operator's `README.md` says:

> Because the two share a printing rule, `-P` with a NUM at least as large as
> the file is exactly equivalent to the default whole-file gate. That
> equivalence is asserted by the test suite.

It is not. Measured against the operator's own `grep.py` on a three-line file
containing `ALPHA`, `BETA`, `ALPHA`:

| invocation | prints |
|---|---|
| no `-P` (whole-file gate) | lines 1, 2, 3 |
| `-P 100` (≥ file length) | lines 1, 2 |

The cause is in the code and is not subtle: the `-P` loop calls
`last_match.clear()` when a window is satisfied, so the `BETA` on line 2 is
consumed by the window that ends there and the `ALPHA` on line 3 finds no live
`BETA`. The whole-file path has no such step — it emits every matching line
once the gate passes. The two do *not* share a printing rule, whatever the
size of NUM.

**This matters to the port beyond being someone else's bug.** The previous
entry named that equivalence as the first test worth writing, on the grounds
that it checks two implementations against each other rather than against my
reading of either. Had it been written it would have failed, and the obvious
response — "my window logic must be wrong" — would have been the wrong one.

**Ours follows the code, not the README**, because the code is what the
operator's own output does and therefore what they are used to. Raised for
them as `open-questions.md` → B-Q10, since only they can say which of the two
they meant.


### One of the nine features cannot be ported, and the reason is this entry

Lane A's inventory ends with the "your path became the regex" warning, and
singles it out: *"worth keeping even though it is not a feature in the usual
sense. It is a guard against a silent wrong answer, which is the class of bug
this project cares most about."*

It cannot come across. The hazard it guards is created by the operator's
argument grammar — their README states it plainly: *"the first non-option
argument is always the search regex … even when you supplied every pattern with
`-e`"*. So `grep -e math -e logic "d:\book\*.html"` silently searches for the
path. That grammar is exactly what this entry declined to adopt.

Under GNU's grammar, and therefore ours, the same command is loud. Measured on
both:

```text
$ grep -e math -e logic 'd:\book\*.html'
grep: d:\book\*.html: No such file or directory
```

A positional argument after `-e` is a **file operand**, so a path that does not
exist is an error rather than a search that quietly matches nothing. The guard
has nothing left to guard: porting it would mean first porting the grammar that
creates the danger.

Worth stating because it is the opposite of the usual finding. Every other
collision in this entry cost the operator a spelling; this one is a case where
keeping GNU's meaning removed a defect rather than trading one away.

### `--escape-control` covers the whole line, where the operator's covers the match

The operator's grep escapes control bytes *inside matched text*, "in their own
colour". Ours escapes the whole printed body, and the divergence is deliberate.

Escaping only the match is a display choice — it shows you what you matched.
Escaping everything is a safety one, and safety is the reading that survives:
an `ESC [ 2 J` in the unmatched half of a line clears the reader's screen
exactly as readily as one inside the match. An option that stopped some control
bytes and passed others would be worse than none, because its existence invites
the belief that the output is now safe to look at.

Measured, without and with:

```text
h i t   033 [ 2 J   a n d   \a b e l l        <- raw ESC and BEL reach the terminal
h i t   \ x 1 b [ 2 J   a n d   \ x 0 7 b e l l
```

Faithful in the details that are not about safety: `0x00`–`0x1f` except `\n`
and `\r`, tab included, `0x7f` left alone because the operator leaves it alone.
Off unless asked, because it changes GNU's byte-exact output.

Flagged here rather than done quietly. If the operator wants match-only
escaping, it is a smaller option than the one now implemented and can be added
beside it.


### Correction to the table above: `--name` is not being added, because we already have it

The resolution table near the top of this entry promises `--name PATTERN` and
`--name-case-sensitive`. Measuring the feature before implementing it shows
that three quarters of "built-in filename globbing" is already in our grep
under GNU's own spellings, and the fourth quarter belongs to the shell. The
table stays as written because it records what was decided from lane A's
inventory; this section records what measuring found, and it wins.

The inventory listed the whole group under "these have no GNU grep
equivalent". For file selection that is not so:

| Operator's flag | Ours | Measured |
|---|---|---|
| `--x_files GLOB` | `--exclude=GLOB` | identical |
| `--x_paths NAME` (one component) | `--exclude-dir=NAME` | identical -- both skip every `NAME` at every depth |
| `--x_paths 'node_*'` | `--exclude-dir='node_*'` | **ours is the stronger one**: theirs compares components for equality and matches nothing here |
| `-f GLOB`, positional globs | the shell | `osh` does pathname expansion, so `grep pat *.rs` already works |
| `-c` case-sensitive names | -- | nothing to turn on; see below |
| `--x_paths A/B` (two components) | -- | **the one real gap** |

`--name PATTERN` would therefore be a second spelling of `--include`, and a
tool with two spellings for one behaviour is worse than one with a single
spelling, whichever is prettier.

**`-c` is a flag about Windows, not about grep.** The operator's grep matches
filenames case-*in*sensitively by default and `-c` turns that off, because that
is what Windows does. `fnmatch` is case-sensitive, GNU's `--include` is
case-sensitive, and `design.txt` makes the filesystem case-sensitive, so
`--name-case-sensitive` would be a flag that switches on the only behaviour we
have. Note the useful flag here is the *inverse* of the one proposed --
a case-insensitive `--include`, which neither GNU nor we have -- and nobody
asked for it, so it is not part of this port.

### `--exclude-path=A/B`, the one thing in that group GNU cannot say

`--exclude-dir` matches a **name**, so it cannot say *which* `temp` to skip.
Ask it to and it agrees and does nothing: the pattern is compared against
`ent->fts_name`, which never holds a `/`, so a pattern containing one can never
match. Measured on GNU grep and on ours, on a tree holding `build/temp` and
`keep/temp`:

| command | skips |
|---|---|
| `--exclude-dir=temp` | both |
| `--exclude-dir=build/temp` | **neither**, silently |
| operator's `--x_paths build/temp` | `build/temp` |
| our new `--exclude-path=build/temp` | `build/temp` |

A silent no-op on a plausible command is the class of defect this project cares
most about, and it is exactly the gap the operator's `--x_paths` fills.

**The spelling passes this entry's own prefix rule, checked before writing
code** rather than by a gate afterwards. GNU spends four options on `e`;
`--exclude-` is *already* ambiguous there (`from`, `dir`), `--exclude-d` and
`--exclude-f` still resolve uniquely after the addition, and `--exclude-p` is
unknown to GNU today. Every prefix that works now still works.

**Two deliberate divergences from `--x_paths`, both supersets.** Their
components are compared for equality; ours are globs, so `--exclude-path='node_*/deep'`
works and a plain name still means itself -- a spec with no metacharacters
behaves exactly as theirs does. And the glob is applied *per component*, so `*`
cannot cross a `/` here even though gnulib lets it cross one inside a name:
`*/temp` means "a `temp` with a parent". Anything else would let a
two-component spec match a one-component path and undo the distinction the
option exists to draw.

**A spec that could never match is refused, not accepted.** `--exclude-path=`,
`--exclude-path=/a` and `--exclude-path=a//b` are usage errors. An option whose
whole reason for existing is that `--exclude-dir=a/b` silently matches nothing
must not be able to silently match nothing itself.


### `--allow-match-colors` is the third proposed name that breaks a GNU abbreviation

It is spelled `--keep-color-escapes`. `--allow-match-colors` cannot be used for
the same reason `--all-patterns` could not: GNU grep has exactly one long
option beginning with `a`, so `grep --a 3 file` resolves to `--after-context`
today and a second `a` option would stop it resolving.

That is now three for three -- `--all-patterns`, `--proximity` and
`--allow-match-colors` -- and only the first was caught by a gate. The rule
in this entry is worth stating as a *procedure* rather than a principle:
enumerate GNU's long options by first letter once, and read the answer off the
table before choosing a name.

```text
a h m o p q s t u v   one option each  -- a new name here BREAKS an abbreviation
b c d e f i l n r w   two or more      -- safe if the name diverges early
g j k x y z …         none at all      -- entirely free
```

`--keep-color-escapes` is in the third row, which is the strongest position
available: GNU grep has no long option beginning with `k` at all, so every
prefix of it, down to `--k`, is one GNU rejects today and we accept now.

### What `--keep-color-escapes` lets through is a whitelist, not a blacklist

The operator's `--allow-match-colors` "passes through ANSI colour sequences
already present in matched text while still filtering every other escape". Ours
recognises exactly one shape -- `ESC [`, parameter bytes, `m` -- and escapes
everything else. Written that way round because a blacklist has to be right
about every sequence that exists and a whitelist only has to be right about
one.

Measured on the built binary with `od -c`, all in one line of input:

| input | with `--escape-control --keep-color-escapes` |
|---|---|
| `ESC [ 3 1 m` … `ESC [ 0 m` | passes through raw |
| `ESC [ 2 J` (erase display) | `\x1b[2J` |
| `ESC ] 0 ; pwned BEL` (set window title) | `\x1b]0;pwned\x07` |
| `ESC [ 3 1` (unfinished) | `\x1b[31` |
| `BEL` | `\x07` |

The unfinished case is the one worth naming: a sequence with no final byte is
*not* passed on, because a terminal that receives it will swallow whatever
arrives next -- including the rest of the grep output -- looking for one.

**Stated honestly, because a safety option that overstates itself is worse than
none:** SGR is the whole graphic-rendition set, not only colour, so `ESC [ 8 m`
(conceal) survives and can make text invisible. The line this option draws is
that output cannot move the cursor, clear the screen, retitle the window or
provoke a reply from the terminal. It is not a promise that the text is
legible, and anyone needing the stronger guarantee leaves the option off, which
is the default.

**It is refused without `--escape-control`, not ignored.** On its own it exempts
something from an escaping that is not happening, so it could only ever be a
no-op -- the same defect `--exclude-path` was added to remove, and it would be
absurd to reintroduce it two commits later. It deliberately does not *imply*
`--escape-control` either: a flag whose name promises to keep something should
not quietly start rewriting everything else.

**Whole body, not the match**, for the same reason `--escape-control` is whole
body: colour in the unmatched half of a line is as much a colour as colour
inside the match, so covering only the match would leave half the output
looking like a bug.


### `--escape-control` introduced an ambiguity, and `GREP_COLORS` `ec=` answers it

Noticed while porting the operator's sixth colour element rather than while
writing the option, which is the wrong order and worth saying so.

Under `--escape-control`, a file holding a real `ESC` byte and a file holding
the four characters `\x1b` produce **the same output**. The option was shipped
without noticing that, and it is inherent: escaping without escaping the escape
always collapses those two inputs. `cat -v` has the same defect and no answer
to it. Doubling every backslash would resolve it and would make every ordinary
path in the output unreadable, which is the worse trade.

The operator's grep already had the answer, and it is the reason their sixth
colour element exists: **colour the escape display**. An escape grep wrote is
painted; one the file contained is not. Ours is `GREP_COLORS` `ec=`, defaulting
to `94` -- the bright blue theirs uses -- because a disambiguation nobody
switches on disambiguates nothing.

**Two of the operator's six colour elements were genuinely missing; four were
already `GREP_COLORS` under other names.**

| Operator's element | Ours |
|---|---|
| filename | `fn` |
| colon separator | `se` |
| line number | `ln` |
| match text | `ms` / `mc` |
| escape code display | **new: `ec`** |
| error message | still missing -- see below |

`GREP_COLORS` is the *larger* set: `sl`, `cx`, `bn`, `rv` and `ne` have no
counterpart in the operator's six. An unknown key is ignored in silence by GNU,
measured, so a `GREP_COLORS` naming `ec` still works there minus the colour --
the safe direction for a divergence.

**SGR does not nest, so the implementation has to close and reopen.** `ESC[m`
is an absolute reset, not a pop, so an escape run inside a coloured match emits
`end(match) start(ec) …\xNN… end(ec) start(match)`. Getting this wrong leaves
the rest of the match uncoloured, which is why it has a test that reads the
whole byte sequence rather than checking for the presence of a colour.

**Still missing: the error-message colour**, and deliberately not added with
this. GNU never colours stderr, and `--color=auto` tests *stdout*, so a
faithful `er` would need its own check on fd 2 -- a different question from the
one this entry is answering, and the wrong thing to bundle into a commit about
stdout.


### The last two features, and what the whole exercise turned out to be about

**`--dotall` is `-zo`.** Measured on the operator's `grep.py`, on GNU and on
ours, on the same three-line file:

```text
$ python grep.py 'alpha.beta' -f dot.txt --dotall
dot.txt:alpha
beta

$ grep -zoH 'alpha.beta' dot.txt | od -c
0000000 d o t . t x t : a l p h a \n b e t a \0
```

Byte-identical but for the terminator, and ours matches GNU byte for byte.
`-z` makes the record separator NUL, so a file with no NUL is one record, `.`
crosses newlines because they are no longer the separator, and `-o` prints the
match rather than the record. Their `--dotall` prints the match too, which is
why their README disables line numbers under it -- a match can span lines.

Lane A's porting constraint said "ours will want a bound, since we have no
guarantee about file size". The bound question is real and it is `-z`'s, not a
new option's: our reader is `read_until(NUL, &mut line)` on a growable `Vec`, so
a whole-file record is a whole file in memory. That is inherent to what `-z`
*means* and is not new today, so it is left as it is rather than made to
diverge from upstream on the strength of a hypothetical.

**`--set-colors` splits into a real feature and a Windows workaround.** The real
part is the seventeen colour *names*: `--set-colors brightgreen brightblack
brightred default brightred brightblue` against `GREP_COLORS='fn=92:se=90:ln=91:ms=39:ec=94'`.
Any `GREP_COLORS` capability may now be written by name. The workaround part is
`--remember`, which writes them to a config file; a shell profile already
persists an environment variable, and `osh` reads `/etc/profile`,
`~/.bash_profile`, `~/.bashrc` and `$BASH_ENV` -- verified in
`userspace/oils/src/main.rs`, not assumed. A config file that duplicates the
profile would be a second place for the same setting to be wrong.

The direction of that divergence is worth naming, because it is the *unsafe*
one: GNU ignores a `GREP_COLORS` value that is not SGR parameters, so
`fn=brightgreen` works here and does nothing there. Accepted because
`GREP_COLORS` is per-user preference rather than a script interface, and the
failure mode on a foreign grep is the default colour, not a wrong answer.

### What nine features came to

| | outcome |
|---|---|
| proximity matching | built: `--near NUM` |
| conjunction across patterns | built: `--every-pattern` |
| control bytes as `\xNN` | built: `--escape-control` (+ `--keep-color-escapes`, + `ec=`) |
| six colour elements | four were `GREP_COLORS` already; `ec` built; names built; `er` left |
| filename globbing | `--exclude`/`--exclude-dir` already; **`--exclude-path` built** for the one gap |
| `--dotall` | already `-zo` |
| `--allow-match-colors` | built as `--keep-color-escapes` |
| persistent colour config | the shell profile, already |
| "your path became the regex" | unportable: the hazard needs their grammar |

**Three of the nine exist because their tool runs on Windows and this one does
not.** `-f`'s built-in globbing is there because `cmd.exe` does not glob and
`osh` does; `-c`'s case-sensitive filename matching is there because Windows
matches names case-insensitively and `design.txt` makes our filesystem
case-sensitive; `--remember` is there because `cmd.exe` has no profile to put
an environment variable in. None of the three is a grep feature at all -- each
is a shell or filesystem capability that their grep had to supply itself.

That is the finding worth keeping from the whole exercise. The request was
"integrate my grep's additional features", and the honest answer was not nine
ports: it was six new things, four already present under GNU's own spellings
(and one of ours *stronger* than theirs), and three that dissolve on a system
with a real shell.


**Against the choice, honestly:** it is the operator's OS, and their muscle
memory is a real cost that falls on them rather than on a hypothetical GNU
user. The mitigation is only a mitigation — aliasing the short forms back
through their config file restores the typing, not the habit of `-P` meaning
proximity everywhere else. If they would rather their spellings won, that is
their call to make and this entry is the thing to overrule.

### The proximity rule, and why the README's example is not enough to derive it

This is recorded because it nearly cost a wrong implementation. `README.md`
gives one worked example: `ALPHA` on line 3, `BETA` on line 5, `ALPHA` again on
line 7, with `--proximity 3`, and says lines 3 and 5 print while line 7 does
not, "its `ALPHA` has no `BETA` within 3 lines".

But `|7 - 5| = 2`, which *is* within 3. Every obvious reading of the sentence —
nearest-neighbour distance, or a forward window of NUM lines — either
contradicts the stated output or prints line 7. The rule is not derivable from
the example.

`grep.py`'s own header states it:

> a history buffer of `proximity + before_context + 1` lines is kept. For each
> line all regexes are checked and `last_match[idx]` updated; entries older
> than `proximity` lines expire. When every regex has a live match the window
> is satisfied, and the matching lines within it (expanded by before/after
> context) are printed from the buffer. **`last_match` is then cleared** to
> look for the next window.

The clearing is the missing piece: the window ending at line 5 *consumes*
`BETA@5`, so when `ALPHA@7` arrives there is no live `BETA` left and no window
is satisfied. Windows are non-overlapping and greedy, earliest-first.

**The lesson, since it generalises:** a worked example pins down what the
output is, not what the rule is, and a rule inferred from one example is a
guess that happens to fit. The implementation is the specification when the
two are available; the prose is a summary of it.

**Two porting constraints**, from lane A's request and kept here because they
are easy to lose:

* **No UTF-8 assumption.** Both of the operator's builds emit UTF-8 and
  reconfigure the console for it — right on Windows, wrong here. SlateOS
  filenames may contain every byte but `/` and NUL, so names and matched text
  stay `&[u8]`/`OsStr`. Forcing UTF-8 would corrupt exactly the filenames the
  tool handles well.
* **`--dotall` reads whole files**, which their README notes disables line
  numbers. Ours needs a bound; we make no guarantee about file size.

**If it is never revisited:** nothing degrades — our grep stays GNU-compatible
and simply lacks the additions. The cost is only that the operator keeps two
greps.

**Superseded in part, 2026-09-27:** the proximity rule above -- the operator's
program read as the rule, windows used up once satisfied -- is replaced by
§1044. Answering B-Q10, the operator ruled their README right and the
program wrong; both of their builds and ours now let a match belong to any
number of windows. The flag spellings and the porting constraints stand.
