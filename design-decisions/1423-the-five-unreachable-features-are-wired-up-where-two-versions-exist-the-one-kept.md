## 1423. The five unreachable features are wired up; where two versions exist, the one kept gets everything both could do

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended C, starting with the installer; the operator chose A) &middot; **Lane:** C (the rule), with E (the five are in its programs)

**In short:** Five programs each held a finished, tested feature no one could
reach: the installer's bootloader setup, the image viewer's video playing, the
process explorer's six tools, system information's hardware queries, and a
remote-settings page. All five are to be wired into their programs rather than
deleted. Where a feature also exists somewhere that *is* reachable -- the image
viewer's video player beside the separate video player program -- the one that
survives must end up with every ability of both, before the other goes.

**The question:** `open-questions.md` C-Q17 (now resolved). **Verbatim:**
"Option A. I think I saw somewhere in C-Q17 that one of the five finished
features already has a wired up version of itself. If true, make the one that
survives have all the features of both versions."

**The rule this sets, beyond the five:** dead code with a live twin is merged,
not merely deleted -- the survivor first takes what only the dead copy had.
Applied the same day to the shell's own unreachable launcher
(`TD-C-THE-DESKTOP-CRATE-CARRIES-A-SECOND-LAUNCHER-NOTHING-USES`): compared
feature by feature with `apps/launcher`, the live one already had everything --
search, frecency, categories, keywords, Tab completion, Ctrl+number -- except
an API for adding programs that nothing called; it now needs to list installed
programs as the start menu does, which is in the request to lane E.
