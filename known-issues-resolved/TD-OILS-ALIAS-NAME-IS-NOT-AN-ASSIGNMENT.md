### TD-OILS-ALIAS-NAME-IS-NOT-AN-ASSIGNMENT. `alias` treated its operand as an assignment word and never checked the name — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `builtin_alias`, plus two new free
functions `legal_alias_name` and `alias_line`. `builtin_unalias` needed no
change; every behaviour probed there already matched.

**The root mistake.** `alias name=value` *looks* like an assignment, and the old
code read it as one: it assumed the name was variable-shaped and rejected
anything else. bash does the opposite — the name is a **command word**, and the
only bytes it refuses are the ones that could not stay inside one
(`legal_alias_name` = `shellbreak || shellxquote || shellexp || '/'`, i.e.
`` ` ' " \ $ ( ) < > ; & | ``, space, tab, newline, `/`). Four divergences fell
out of that one misreading, all measured against bash 5.2.37:

1. **No name validation at all.** `alias 'a b=2'` silently defined an alias
   that could never be typed. bash prints ``alias: `a b': invalid alias name``
   on stderr, gives the *call* a status of 1, and **carries on** with the
   operands after it — `alias f1=1 'a b=2' f2=3 'c/d=4' f3=5` defines f1/f2/f3,
   reports twice and returns 1. Conversely, osh refused nothing it should have
   accepted only because it never looked; `1a`, `a-b`, `a[0]`, `x!`, `a#b`,
   `a{b` are all legal alias names.
2. **The split point.** A word splits at its **first** `=`, and is a definition
   only when that leaves a name in front. So `a=b=c` defines `a` as `b=c`, and
   `=v` / `=` / `==v` are *queries* — for names spelled `=v`, `=`, `==v` that no
   definition could have created — answered with the plain `alias: ==v: not
   found` and status 1. osh split at the first `=` past index 0, which turned
   `==v` into a definition of `=`, and separately reported "invalid alias name"
   for the leading-`=` forms.
3. **`--` before a dashed name in the listing.** Each line has to be a command
   that would re-enter the alias, so `alias -- -x=1; alias` prints
   `alias -- -x='1'` — in the whole listing and in a single query alike. osh
   printed `alias -x='1'`, which reads back as options.
4. **`-p` is not "print instead of define".** It prints the table and *then*
   still handles the operands: `alias a=1; alias -p b=2` lists `a` and then
   defines `b`. The one thing that cuts the call short is an **empty table**,
   which returns 0 before it looks at an operand at all — so `alias -p nope`
   with nothing defined is silent and succeeds, while `alias nope` is 1.

**Already correct, confirmed by probe:** byte-sorted listing (the table is a
`BTreeMap`), `sh_single_quote` on every value, `alias -q` → invalid option with
status 2 and the usage line, the getopt stop (`alias c -p` treats `-p` as a
name), clustered `-pp`, `BASH_ALIASES` mirroring, and all of `unalias`
(`-a` with names still clears everything and returns 0; `-a` *after* a name is
a name; each missing name is reported separately; no name is ever refused).

**Coverage added:** `tests/corpus/an-alias-is-a-name-before-it-is-an-assignment.sh`
(the full byte survey both ways, the refused-in-the-middle case, the leading-`=`
queries, the `--` prefix, `-p`'s two behaviours, the getopt stop, the sorted
listing, the value quoting, `unalias`), and five lib tests named for the rules
they pin.
