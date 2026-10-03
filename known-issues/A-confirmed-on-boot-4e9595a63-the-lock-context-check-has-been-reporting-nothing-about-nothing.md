### [A] Confirmed on boot `4e9595a63`: the lock-context check has been reporting nothing about nothing -- 2026-09-17

**Status:** OPEN

With the controls excluded from the population, the line reads:

```
[lockdep] lock-context: 0 violation(s), 0 suspect(s), over 0 class(es) seen in interrupt context -- VACUOUS: no interrupt-context acquisition was seen all boot
```

So the earlier `over 2 class(es) -- clean` was entirely synthetic, as
predicted, and **every `clean` this check printed before today carried no
information about the kernel.** It was not wrong; it was empty, and those are
indistinguishable without the population number. The entry above explains
why the real corpus is legitimately ~0 -- the hazard is designed out, not
guarded -- so the honest reading is that this is a tripwire armed for a
future regression, and the `VACUOUS` word is what stops it reading as a
clean bill of health in the meantime.
