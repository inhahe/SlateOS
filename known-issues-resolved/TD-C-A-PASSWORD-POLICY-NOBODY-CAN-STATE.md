## `TD-C-A-PASSWORD-POLICY-NOBODY-CAN-STATE` (lane C, 2026-09-18) -- FIXED 2026-09-27

**Status:** FIXED 2026-09-27 (lane E). Filed among the resolved on 2026-10-05; what follows is the entry as it stood.

> **Earlier status: FIXED 2026-09-27 (lane E), with C-Q26 answered (option A,
> §1418).** The rules are drawn and changed on a Rules tab (`4`; Up/Down
> choose, Left/Right change, Space flips) and kept in `passwordgen.yaml`
> under `rules:` -- shortest and longest length, each kind of character,
> how many kinds to mix, the least strength in bits, refuse the common
> ones. A value there that cannot be used keeps its default and is said,
> in the status bar and on the tab. The tab also says what the rules make
> of the generated password and of the one in the analyser.
>
> **Four more things were wrong in the same program, found on the way:**
> the analyser never drew what was typed into it (the meter measured a
> password nobody could see); the digit keys switched tabs there, so
> "abc123" jumped to the generator at the "1"; switching tabs kept
> the old tab's strength on show beside the new tab's password, and the
> status bar judged the generated password even on the analyser; and a
> password's length was counted in bytes, so "pässwörd" was ten
> characters long. All four fixed in the same change, each with a test.

**In short:** `apps/passwordgen` checks every password it makes against a
rule set -- must contain a digit, must contain a symbol, must not be a common
word -- and shows a compliance mark when it passes. The rule set is written
into the program. If your employer requires sixteen characters and no
symbols, this program cannot be told that, and its tick means "it matched
*our* rules", not yours.

**Where it lives.** `PasswordPolicy` in `apps/passwordgen/src/main.rs`:
`require_digit`, `require_lowercase`, `require_symbol`, `require_uppercase`,
`disallow_common`, all read by `policy.check()` and `policy.is_compliant()`
(called from the status bar at line ~1799) and assigned nowhere outside
tests.

**Why it is not a keyboard fix, unlike the rest of that survey.** The four
generation options fixed in `1e8c105d8` are choices a person makes per
password -- longer, no symbols, one of each kind -- and a key is the right
home for them. A policy is not that. It is a *standard somebody else sets*,
it wants to persist across runs, and expressing "at least 16 characters" or
"at least 3 of the 4 classes" needs more than a toggle. Binding `Shift+R` to
"require a digit: no" would be a worse program: a compliance indicator whose
rules the person being checked can quietly relax is not a compliance
indicator.

**What the repair wants.** A settings file, whose shape is **C-Q26** --
this is the third app to want one (`apps/lockscreen`'s `show_clock_seconds`
and `show_date`, `apps/markdowneditor`'s `autosave_enabled`), and the shape
of that file is an operator question rather than a lane decision, since it
sets where user preferences live for every app in the tree. Until then the
policy is *safe* -- it is a reasonable default and it is honest about what it
checked -- so this is a missing capability, not a wrong answer.

**What is deliberately not on this list.** `PasswordAnalysis`'s
`has_digits`, `has_uppercase`, `is_common`, `rating` and the rest show up in
the same survey row and are not defects: they are measurements of a password,
computed at construction and correctly never changed afterwards. The survey
documents that false-positive mode; it is recorded here so the row is not
re-investigated a third time.
