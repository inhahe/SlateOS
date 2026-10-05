### TD-OILS-ARITH-AT-SUBSCRIPT. `$(( a[@] ))` was a syntax error in osh where bash calls it a bad subscript and carries on — 2026-08-05 — ✅ FIXED 2026-08-05

**Symptom.** `@` and `*` are legal *bytes* in an arithmetic subscript as far as
bash's parser is concerned; it is the array lookup that rejects them, and only
for an **indexed** array (an associative one reads them as ordinary string
keys). The complaint is a warning, not an error: the operand is worth 0 and the
expression keeps going.

```sh
a=(1 2 3); declare -A m=([k]=4)

$ bash                              $ osh
$(( a[@] ))    a[@]: bad array subscript ; [0] rc=0     @: syntax error: operand expected (error token is "@")
$(( a[*] ))    a[*]: bad array subscript ; [0] rc=0     *: syntax error: operand expected (error token is "*")
$(( m[@] ))    [0] rc=0                                 [0] rc=0
$(( nosuch[@] ))  nosuch[@]: bad array subscript ; [0]  @: syntax error…
$(( a[@]+1 ))  a[@]: bad array subscript ; [1] rc=0     @: syntax error…
(( a[@] = 5 )) a[@]: bad array subscript ; rc=0, `a` unchanged   @: syntax error…
```

osh's is an *error*, so the whole expression is abandoned and the status is 1
where bash's is 0 — a difference in control flow, not just wording.

**Where.** `userspace/oils/src/arith.rs` — the subscript lexer. It hands the
subscript text to `Sub::parse`, which parses it as its own arithmetic
expression and so rejects `@`/`*` as operand bytes. bash instead keeps the
subscript's raw text and refuses it at *lookup* time, which is why the
associative path (a string key) is unaffected.

**Proper fix.** Recognise a whole-array subscript (`@` or `*`, after trimming)
in the lexer alongside the existing empty-subscript case — `Expr::EmptySub`
already models exactly this shape: a subscript that parses but names nothing,
reported by a `VarLookup` hook and worth 0. Add a sibling variant carrying the
offending spelling, whose read hook prints `NAME[@]: bad array subscript` once
(bash prints it once here, unlike the *empty* subscript's two lines) and whose
store hook prints the same and drops the write. The check must come *after*
[`VarLookup::is_assoc`], since an associative array takes `@` as a key.

**Found while** closing the arithmetic half of
TD-OILS-NAMEREF-CYCLE-ARRAY-WRITE; it is unrelated to namerefs and was simply
never probed before.

**✅ FIXED 2026-08-05**, along the lines above: `Expr::WholeSub(name, byte)`
beside `Expr::EmptySub`, recognised in the subscript lexer on the *exact* raw
bytes and only after [`VarLookup::is_assoc`], with a
`VarLookup::refuse_whole_array_subscript` hook called once from the read arm and
once from the store arm — which is what makes `(( a[@]++ ))` print the line
twice where `let "a[@] = 5"` prints it once.

Two details the measurement settled that a guess would have missed. The hook
resolves the name and *throws the answer away*: bash gets as far as finding the
array before refusing the subscript, so a circular chain earns its two
element-lookup warnings ahead of the refusal — but the array found has no
bearing on the complaint, which blames the name exactly as written
(`declare -n r=a; (( r[@] ))` is `r[@]`, and a reference that already names one
element is `e[@]` rather than the "not a valid identifier" it would otherwise
draw). And the store stops *before* the bind, so unlike a real element write it
does not drop the nameref attribute: `declare -p c1` after `(( c1[@] = 5 ))`
through a cycle still says `declare -n c1="c2"`.

**Coverage.** Corpus case
`an-arithmetic-subscript-of-at-is-refused-not-a-syntax-error.sh` and the lib
test `an_arithmetic_subscript_of_at_is_refused_not_a_syntax_error`.
