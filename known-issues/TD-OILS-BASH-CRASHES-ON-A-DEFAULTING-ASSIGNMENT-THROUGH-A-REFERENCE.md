### TD-OILS-BASH-CRASHES-ON-A-DEFAULTING-ASSIGNMENT-THROUGH-A-REFERENCE. `n=(); declare -n r='n[1]'; : "${r:=D}"` segfaults bash 5.2.37 — 2026-08-05 — ⚠️ **NOT EMULATED (upstream bug)**

**Where:** nothing of ours — recorded so the gap in
`a-defaulting-assignment-through-a-reference-to-an-element.sh` is not mistaken
for an oversight.

**What.** `${ref:=word}` through a nameref designating an element crashes bash
outright for every subscript except indexed element **0**. Measured on bash
5.2.37 (MSYS), each in its own subshell:

```text
n=(); declare -n r='n[0]'                    `[D]`, `declare -a n=([0]="D")`
declare -a n; declare -n r='n[0]'            `[D]`
declare -n r='nosuch[0]'                     `[D]`, the base is made
n=(); declare -n b1='n[0]'; declare -n r=b1  `[D]`, through the chain

n=(); declare -n r='n[1]'                    Segmentation fault (139)
declare -A mm=(); declare -n r='mm[k]'       Segmentation fault (139)
declare -A mm=([z]=Z); declare -n r='mm[k]'  Segmentation fault (139)
n=(a); declare -n r='n[1]'                   `[a]` — wrong, and `n[1]` is `D`
```

The last line is the same fault surviving: the expansion answers the *other*
element's value while the store lands correctly, which is what a read through a
freed or mis-sized array looks like.

**Why it is not emulated.** There is no behaviour here to be byte-exact with. A
crash is not a specification, and the one non-crashing wrong answer is a
by-product of the same corruption. osh does the obvious right thing throughout
— it creates the element and expands to the new value, for every subscript and
both array kinds — which agrees with bash exactly where bash is standing up.

**If bash is ever fixed** the corpus case can be widened past element 0; until
then it stays where the two shells can be compared at all.
