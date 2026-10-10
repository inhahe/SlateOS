## F — When SlateOS is built for x86-64-v3, delete the hot loops' plain copies — deferred 2026-10-09 (lane F)

**In short:** some of SlateOS's video code has two copies of its busiest
loops: one for every x86-64 processor, and one using newer instructions,
chosen when the program starts by asking the processor what it has (§1373).
If SlateOS is ever built to run only on processors from about 2013-2015 on
(the "x86-64-v3" level, which has those instructions), the choice becomes
pointless: the plain copies, and the one unchecked line each choice costs,
should then be deleted. The operator asked for this to be remembered
(answering F-Q5).

**Trigger:** the workspace's target is raised to `x86-64-v3` or later (its
`target-cpu`, or the `x86_64-slateos` target's features, gaining AVX2).

**What to do then.** Nothing breaks on its own: Rust's
`is_x86_feature_detected!` is `cfg!(target_feature = ...) || a run-time check`,
so once the build enables the feature the check is a constant "yes" and the
newer copy is always chosen. What remains is the tidying: delete each plain
copy, its `#[target_feature]` twin's `unsafe` call, and the crate's named
exception to `forbid(unsafe_code)` (`gui/video/vp9`, `gui/video/vp8`, and any
other lane F crate §1373 is applied to; `grep -rn "is_x86_feature_detected" gui/`
finds them). No question for the operator remains then -- this is a
reminder, not a decision; it is here because nothing can act on it before the
trigger.
