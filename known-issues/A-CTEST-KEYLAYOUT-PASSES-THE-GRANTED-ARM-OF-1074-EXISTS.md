## A-CTEST-KEYLAYOUT-PASSES-THE-GRANTED-ARM-OF-1074-EXISTS (lane A, 2026-09-16) — **Status: PASSED**

First time this has ever run:

```
[spawn]   keyboard layout set from ring 3, confirmed through /proc/keylayout,
          an unregistered name refused without moving the active layout, and
          the original restored: OK
```

`SYS_KEYLAYOUT_SET` works end to end from userspace with the capability
held. **The granted arm of the gate now exists**, which is what
design-decisions 946 and the dispatch probe's own caveat said was missing:
a probe that only ever gets refused cannot tell "the gate refuses
everyone" from "the gate works". It works.

Three properties the fixture proves that a weaker one would not:

* it confirms through **`/proc/keylayout`**, the publisher, not through a
  getter -- which is only possible because 1074 deliberately has none. A
  getter would have made this the `vconsole.conf` round trip in a fixture's
  clothes: asking the setter whether the setter worked.
* an unregistered name is refused **and the active layout does not move**.
  A refusal that still changed something is worse than an acceptance,
  because nothing downstream expects it.
* the original layout is restored and confirmed, so the rung leaves no
  residue in state that `/proc` publishes.

It also retires the withdrawn entry above by demonstration rather than by
argument: `/proc/keylayout` is openable from ring 3, by a process that
holds `(File, READ)`.
