### TD-OILS-BASH-CRASHES-ON-A-READONLY-ELEMENT-AFTER-A-CIRCULAR-NAMEREF. resolving a circular chain leaves the reference shell able to segfault later — 2026-08-04 — WONTFIX (bash bug; a corpus-authoring hazard)

**What.** On its own, an assign-default into an element of a readonly scalar
reached from inside a function is refused and fatal, and osh matches bash
5.2.37 byte for byte — including the way the diagnostic is introduced, which
differs between the two spellings:

```
$ cat r.sh
f() { echo "[${t[1]:=w}]"; }; ( declare -r t=v; f ); echo "rc=$?"
$ bash --norc r.sh              $ osh r.sh
r.sh: line 1: t: readonly variable   (identical)
rc=2
```

But let the shell resolve a **circular nameref chain** two or more times first —
even inside subshells of a pipeline, which should leave the parent untouched —
and the same line crashes bash instead:

```
declare -n c1=c2; declare -n c2=c1        # …resolved twice, in subshells
f() { echo "[${t[1]:=w}]"; }; ( declare -r t=v; f )
bash: line 4: 128313 Segmentation fault      ( declare -r t=v; f )
rc=139
```

One prior resolution is not enough; two are. The crash needs *both* the prior
circular resolution and the function frame — `( declare -r t=v; t=9 )`,
`( echo hi )` and a plain function call all survive unharmed afterwards — so
this looks like state the circular-nameref walk leaves behind rather than
anything about the readonly refusal itself. (The reference bash here is the
MSYS build, whose `fork` is emulated, so the subshell isolation that ought to
contain it may be part of the story.)

**Consequence — a hazard when writing corpus cases.** A case that provokes
circular-nameref warnings and *later* does something in this neighbourhood will
diff a live shell against a crashed one, and the failure will look like an osh
bug. Keep the two apart. The readonly-element shape is covered by the
`an_assign_default_into_a_readonly_element_is_refused_and_fatal` lib test rather
than by the corpus for exactly this reason.
