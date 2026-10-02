## [B] The sudoers parser cannot reject a malformed `Defaults` or command list (2026-08-18)

**Status:** FIXED 2026-08-18 — see "Fixed" at the foot of this entry. Held here
rather than archived until the fix has survived a boot test on `main`.

**Where:** `userspace/sudo/src/main.rs` — `parse_defaults` and
`parse_cmnd_list`.

**What:** both are declared `-> Result<_, SudoError>` and neither has any path
that returns `Err`. Clippy pointed this out as `unnecessary_wraps` while the
crate was being brought under the workspace lint set; the signature was kept
deliberately, because their two siblings in the same family — `parse_alias` and
`parse_privilege_spec` — do reject input, every caller drives all four
uniformly with `?`, and these two functions are exactly where validation
attaches when it is written. An `#[allow]` with that reasoning sits on each.

**Why it matters:** the consequence is not a crash but silence. A `Defaults`
line the administrator misspelled is parsed into whatever falls out of the
scope-splitting, stored, and never mentioned — so `visudo -c`, whose entire job
is to say "this file is wrong before you install it", reports the file as fine.
The failure is therefore invisible at exactly the moment it is checkable, and
surfaces later as a policy that does not do what its author read it as doing.

**Proper fix:** decide what each directive's grammar actually is and reject
input outside it — a `Defaults` line with no `=` in a setting that requires
one, an unknown setting name, a tag prefix (`NOPASSWD:` and friends) with no
command after it, an empty command list. Each new rejection needs a test
alongside it, and `visudo`'s existing validation tests are the place they go.

**Why not now:** this is a behaviour change to the parsing of a security policy
file, not a lint cleanup. Files that parse today would start being rejected,
which is the correct outcome but is not something to slip into a commit whose
stated purpose is enabling a lint set. It wants its own change, its own tests,
and a look at whether the installer ships a `/etc/sudoers` that would newly
fail.

**Severity:** medium. Nothing is misgranted by it directly — an unparsed
`Defaults` setting keeps its default — but it defeats the one tool the
administrator has for catching their own mistake.

**Fixed 2026-08-18** in `334ae409d` (lane B). Both functions now return `Err`
for real, both `#[allow(clippy::unnecessary_wraps)]` are gone, and
`validate_sudoers_line` validates a `Defaults` line by *running the parser*
rather than by a separate check that would have to agree with it. The
error-versus-warning split, and the `=`-replaces-rather-than-adds behaviour
change, are recorded in `design-decisions.md` §332.

Writing the fix turned up four further defects the original entry did not know
about, all now fixed in the same commit:

| Defect | Consequence |
|---|---|
| The setting name was everything before the first `=`, so `env_keep += "X"` was stored under the name `env_keep +` | Two consumers compensated with triple key matches; `get_default` had no workaround and never saw a `+=` line, so `timestamp_timeout` and `env_reset` silently ignored one |
| `=` on a list setting *added* rather than replaced | A file that deliberately narrowed the kept environment did not narrow it — the unsafe direction |
| `get_default` returned the *first* match | A file that overrode a setting further down kept the earlier value |
| `Defaults:alice` with nothing after it | Silently discarded |

The first of those is the band-aid shape this tree keeps producing: one defect
in the parser, paid for separately at each consumer, with the consumer that
*didn't* get a band-aid simply broken. The fix went into the parser and the
band-aids came out.

The pre-existing test `env_keep_extended` asserted the old add-on-`=`
behaviour — the test encoded the bug — and was rewritten with a dated in-test
note saying so. ~25 tests were added; `cargo test -p sudo` is 234 passed, 0
failed. The precondition this entry set for making the behaviour change —
"a look at whether the installer ships a `/etc/sudoers` that would newly fail"
— was checked: `git grep -ln "etc/sudoers"` finds no shipped file, so no
existing configuration can newly be rejected.
