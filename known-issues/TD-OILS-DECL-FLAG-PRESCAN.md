### TD-OILS-DECL-FLAG-PRESCAN. bash decides which attributes are in force while a compound operand binds from a *pre-expansion* scan of the flag words — 2026-07-30 — OPEN (very low priority, deliberate)

**Where:** `userspace/oils/src/interp.rs` — `exec_declare_with_arrays`'s flag loop,
which reads the already-expanded `argv`.

**What.** A compound operand of a declaration builtin binds during the command's
word-expansion pass (see the phase comment in `exec_declare_with_arrays`), so the
attributes that transform its values have to be known *before* the words are
expanded. bash therefore reads them off the flag words **as written**, and a flag
letter that only appears after expansion is missed at bind time even though the
builtin still records the attribute afterwards:

```
f=l
declare -a$f v=(Ab); declare -p v   # bash: declare -al v=([0]="Ab")   -- not folded
declare -al  v=(Ab); declare -p v   # bash: declare -al v=([0]="ab")
```

osh folds in both spellings, because its flag loop runs over post-expansion
`argv`. Only the `a`/`A` kind and the `i`/`l`/`u`/`c` value transforms are
affected; `x`/`r`/`n`/`g` are applied by the builtin in both shells and so are
immune.

**The exact rule, measured 2026-08-04 against bash 5.2.37.** bash's
`fix_assignment_words` walks the leading words *before any expansion* and, for
each one whose text begins with `-`, scans that text **character by character**
for attribute letters. So what counts is not what the flag word expands to but
which letters literally appear in it, `$`, braces, quotes and all:

```sh
l=X;    declare -a$l   t=(Ab)   # the raw `-a$l` contains `l` → t is `ab`,
                                # even though the builtin then refuses `-aX`
qzz=l;  declare -a$qzz t=(Ab)   # `-a$qzz` has no `l` → `Ab` unfolded
        declare -a"l"  t=(Ab)   # quotes do not hide it → `ab`
        declare -a${l} t=(Ab)   # nor do braces → `ab`
ii=9;   declare -a$ii  t=(2+3)  # the `i` of the *name* makes it arithmetic → `5`
kA=q;   declare -$kA   t=([k]=v)  # the `A` of the name makes it associative
k=A;    declare -$k    t=([k]=v)  # …and without one it binds *indexed*, so the
                                  # builtin's own `-A` then fails with
                                  # `cannot convert indexed to associative array`
        declare -a$(echo l) t=(Ab)  # `echo` contributes `c` as well as the `l`,
                                    # and two case letters mean no fold at all
g=-al;  declare $g     t=(Ab)   # `$g` does not start with `-`, so the scan stops
                                # before it and no letter is seen at all
```

The last two are what make the rule unmistakable: the scan is textual, it is
subject to the same "two different case letters cancel" rule the expanded scan
already follows, and it stops at the first word that does not *look* like a flag
word rather than at the first one that is not.

**Why it is not fixed.** Matching it means scanning the *unexpanded* words for
flag letters, and osh's AST does not keep a word's source text: `ast::Word` is a
`Vec<WordPart>`, so the scan would have to run over a *re-rendered* spelling of
the word. That renderer is exactly where the quirk would go wrong — `$'\154'`
and `` `echo l` `` render as something other than what was written — so the fix
would trade one divergence for a subtler one. Doing it properly means keeping a
source span on `ast::Word`, which is worth it only when something else wants one
too. Note that neither of the two things that came close was that trigger: the
`decl_arrays` positional info added for TD-OILS-XTRACE-ARRAY-DECL
(`ast::DeclArray`) records where each compound operand sat among the words, not
their text, and the per-operand interleaving added for
TD-OILS-DECL-COMPOUND-NOT-SEEN-BY-A-LATER-SCALAR passes the *expanded* `argv`
prefix for the same reason.
