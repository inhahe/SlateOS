### TD-OILS-AN-INDIRECT-BRACE-EXPANDS-ITS-POINTER-THE-WRONG-NUMBER-OF-TIMES. `${!m[$(f)]}` runs `f` three times, `${!p[$(f)]#x}` once, where bash runs it once and twice — 2026-08-09 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, the `${!name[sub]}` indirection path —
`Shell::indirect_ref_part` / `Shell::indirect_op_part` (the silent recognisers)
and `Shell::expand_indirect`, all of which reach `Shell::pointer_lookup` and so
expanded the subscript word again. Every classification pass osh made cost one
more expansion of the pointer; bash's count is fixed by its own structure and
does not track osh's passes in either direction.

**Values were correct everywhere** — this was purely how many times a side effect
in the pointer's subscript happens.

**Reproduce.** An arithmetic subscript counts the readings in the shell itself:
`$((i++, 0))` bumps `i` on every evaluation and always yields the same key, so
the reference is the same one each time.

```text
                             bash   osh-was
declare -A m=([k0]=K)         -      -    # referent K names an unset scalar
${!m[k$((i++, 0))]}           1      3
"${!m[k$((i++, 0))]}"         1      3
${!m[k$((i++, 0))]:-D}        1      3
declare -A s=([k0]=sv)        -      -    # referent names a set scalar
${!s[k$((i++, 0))]}           1      3
${!s[k$((i++, 0))]#S}         2      3
${!s[k$((i++, 0))]:1:1}       2      3
${!s[k$((i++, 0))]@Q}         2      3
declare -A p=([k0]='n[@]')    -      -    # referent names a whole array
${!p[k$((i++, 0))]}           1      1
${!p[k$((i++, 0))]#x}         2      1
${!p[k$((i++, 0))]@Q}         2      1
declare -A u=([k0]=nosuch)    -      -    # referent names an unset scalar
${!u[k$((i++, 0))]:=D}        2      1
declare -A el=([k0]='n[9]')   -      -    # referent is an absent element
${!el[k$((i++, 0))]:=D}       2      1
```

**Why — bash's rule, exactly.** The pointer is expanded

1. **once, always**, by `parameter_brace_expand_indir` (subst.c:9825), which is
   the `want_indir` branch of `parameter_brace_expand`; and
2. **once more** by whichever of bash's *two* re-readers the operator reaches.

The first re-reader is `get_var_and_type`, which re-resolves the name it was
given — and for an indirect that name is still `!ref[…]`, subscript and all:
`want_indir = *varname == '!' … ; vname = parameter_brace_find_indir
(varname+1, …)` (subst.c:8261-8265). Five handlers call it, each guarding first,
which is why an unset referent usually costs only one. `V` below is the text the
reference resolved to — `None` for an unset target, an absent element or a
zero-element array, `Some("")` for an empty scalar or a one-empty-element array:

| handler | operators | reads again when | guard |
|---|---|---|---|
| `parameter_brace_remove_pattern` | `#` `##` `%` `%%` | `V` set and non-empty **and** the pattern's *source text* non-empty | dispatch subst.c:10082, then 5874 |
| `parameter_brace_substring` | `:off[:len]` | `V` set | subst.c:8799 (`$@`/`$*` exempt — an indirect never is, its name starts `!`) |
| `parameter_brace_patsub` | `/pat/rep` | `V` set | subst.c:9119 |
| `parameter_brace_casemod` | `^` `^^` `,` `,,` `~` | `V` set | subst.c:9366 |
| `parameter_brace_transform` | `@op`, a bad `op` included | `V` set **or** `op` starts `a`/`A` | subst.c:8665 |

The trim guard reads the pattern's *unexpanded* text: `${!s[…]#}` reads once
while `${!s[…]#""}` and `${!s[…]#$(g)}` read twice whatever `g` prints. And the
second reading comes **before** the operator's own operand — `${!s[…]#$(g)}`
reports `f f g`.

The second re-reader is `parameter_brace_expand_rhs` (subst.c:7841-7862), which
`:=`/`=` use to name the destination they store through — **after** the
right-hand side has expanded, so that one reports `f D f`. It is reached only
where the operator is inactive and so actually assigns, and it refuses anything
that reading does not spell as a plain identifier:

```c
vname = parameter_brace_find_indir (name + 1, …, 1);
if (vname == 0 || *vname == 0)
  report_error (_("%s: invalid indirect expansion"), name);
if (legal_identifier (vname) == 0)
  report_error (_("%s: invalid variable name"), vname);
```

The destination is that *second* reading's, which is observable:
`i=0; e=(nosuch other); ${!e[i++]:=D}` leaves `other=D` and `nosuch` unset. A
whole-array referent ends in the second refusal, `legal_identifier("ea[@]")`
being false — so `${!pe[k]:=D}` with `pe=([k]='ea[@]')` says
`ea[@]: invalid variable name`, where the written-out `${ea[@]:=D}` says
`bad array subscript` and an *associative* referent would otherwise have had the
literal key `@` assigned.

The bare form, `${#…}`, and the `:-`/`:+`/`:?` operators never re-read at all.
Neither does a pointer the indirection itself already refused — an empty target
name, one that is no kind of variable reference (`1abc`), or a pointer variable
that does not exist: those complain during `parameter_brace_expand_indir` and
read once. Nor does a **nameref** pointer: this family never looks through one,
so `${!r:=D}` is the name `r` holds, always non-empty, hence always active.

**Fixed by** giving the part being expanded a `PointerPart` value — its one
first resolution, whether its one second resolution has happened, and the
pointer an assignment still owes a reading of. `Shell::pointer_lookup` is
memoised through it so the classification passes cost nothing;
`Shell::pointer_lookup_now` bypasses the memo for the readings that are real.
`Shell::indirect_op_reresolves` encodes the table above and
`Shell::indirect_reresolve` makes that reading once per part however many of
osh's paths ask; `Shell::indirect_arm_rebind` /
`Shell::indirect_rebind_store` carry the assignment's reading to the store,
which is where bash makes it. The scope is one part: `enter_pointer_walk` /
`begin_pointer_part` / `leave_pointer_walk` bracket the twelve part loops, so a
part evaluated twice resolves twice and a `${!…}` nested inside another's
subscript gets a scope of its own.

*Rejected:* keying the memo on the `WordPart`'s address — a dropped
runtime-built `Word` can be reallocated at the same address, giving a stale hit
and a **wrong value**. *Rejected:* the original plan to rewrite the part up front
for every referent shape — a scalar referent's diagnostics
(`indirect_special_unbound`, `indirect_whole_set_unbound`, "invalid variable
name", the nounset name `!ref`) all live in `expand_indirect` and would have to
be replicated onto a synthetic `WordPart::Param`.

**Known deviation.** bash's second reading supplies `get_var_and_type` with the
`SHELL_VAR` behind the name while the *value* stays the first reading's; osh
discards the second answer entirely. Observable only where the subscript is
non-deterministic **and** changes the variable's *kind*. The assignment path
does use its second answer, since that is the destination.

**Covered by** `tests/corpus/an-indirect-resolves-its-pointer-once-per-operator-not-once-per-pass.sh`,
`tests/corpus/an-indirect-assignment-names-its-destination-by-reading-the-pointer-again.sh`,
and the two unit tests of the same names.

**Found by** the probe series for
TD-OILS-A-BRACE-SUBSCRIPT-IS-NOT-EXPANDED-AS-A-STRING-OF-ITS-OWN.
