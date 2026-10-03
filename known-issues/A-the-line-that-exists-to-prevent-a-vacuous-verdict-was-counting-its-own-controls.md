### [A] The line that exists to prevent a vacuous verdict was counting its own controls -- 2026-09-17

**Status:** OPEN

**In short:** a check prints how many things it examined, so that finding
nothing wrong cannot be confused with looking at nothing. On its first
honest boot it printed `over 2 class(es) -- clean`. Both of those two were
its own test fixtures. It had examined nothing real, and said `clean`.

Boot `894c9b0b8`:

```
[lockdep] lock-context: 0 violation(s), 0 suspect(s), over 2 class(es) seen in interrupt context -- clean
```

`ctx-control` is acquired by the negative control in simulated interrupt
context with interrupts clear and a *blocking* acquire, so it sets
`CLASS_HARDIRQ_OFF`. `ctx-safe`, the irqsave-pattern control, sets it too.
`ctx-try` correctly does not, since `Acquire::Try` stopped being the
interrupt-side hazard earlier the same day. Two classes, both synthetic, and
a real corpus of **zero**.

**The cause is a half-applied rule, which is the transferable part.** The
*verdict* counters were split between real and deliberate the moment the
controls were written -- that is what `CTX_SELF_TEST_VIOLATIONS` is for, and
the reasoning is in its doc comment. The *population* was not split. One of
two places got the rule, and a rule applied in one of the two places it
belongs reads exactly like a rule that has been applied.

Fixed by flagging the classes touched while `IN_SELF_TEST` and excluding
them from the count. Excluded at the point of *counting* only: the controls
need the buckets to function, so they cannot be skipped at the point of
recording.

**What it says about the method.** This is 942 landing on the instrument
built to enforce 942, and it was not found by a boot failing -- the boot
said `clean` and the tree was in fact clean. It was found by asking what the
number was made of, which is the same question 942 is about, pointed at my
own output instead of somebody else's.
