### [A] The leaf-claim cap was reporting 27% of the truth: 89 distinct site pairs, not 24 -- 2026-09-17

**Status:** OPEN

With the counting cap raised to 256 and the printing cap left at 24, boot
`3a29fd0d2` reports:

```
[sync] leaf-claim check: 1256 acquisition(s) inside a PreemptSpinMutex, 89 distinct site pair(s) named above
```

and no `PAIR TABLE FULL` suffix, so **89 is a total rather than a floor**.
Every previous boot said 24, which was the cap reading itself back.

**Two things this settles that the saturated number could not.**

*The control does not pollute the corpus -- now measured, not reasoned.* No
report names `leaf-ctl-out` or `leaf-ctl-in`, the control's own locks. The
previous commit had to state that the slot exclusion was verified only by
construction, because a table at its cap reports the same figure whether or
not a fixture occupies a slot. Headroom turned the count into the evidence.

*The print/count split works.* 24 lines printed, 89 counted. A hundred lines
of the same shape would not have been more informative; the number of them
is.

**And it corrects my own estimate in A-Q16.** I wrote there that 24 site
pairs "is perhaps 12-15 distinct lock pairs", reasoning that several site
pairs share an inner lock. The site count is 89, so that inference was built
on a number that was itself a cap reading. dd-938: an artifact true when
written does not say when it stopped being true -- except this one was never
true, it was extrapolated from a saturated instrument. A-Q16 now carries the
measured figure.

What it means for dd-70's premise is a matter of degree rather than kind: the
"true leaf" claim does not fail in a dozen places, it fails in 89 distinct
code sites across 1256 acquisitions per boot. Still not a deadlock report --
nothing here has been shown to form a cycle -- but the reason those locks are
safe remains "no pair happens to be taken in both orders", and now that is
unchecked in 89 places.
