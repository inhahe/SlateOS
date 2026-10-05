# B → E: `ere` reads a backslash as glibc does now — `logviewer` and `renamer` see `\t` as `t`

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane E (`apps/logviewer`,
`apps/renamer`). **Status:** FYI. Your tests pass unchanged; nothing is asked
unless you want C escapes in your patterns.

## In short

The shared regex engine (`userspace/ere`) used to read `\t` as a tab and `\n`
as a newline, and a backslash inside `[...]` as an escape. glibc's regex does
neither, and every program this engine imitates (`grep`, `find`, `ed`, `expr`,
the shell's `[[ =~ ]]`) answers the way glibc does — so the engine was wrong
for them. As of lane B's commit on 2026-10-01 it reads backslashes exactly as
glibc: `\t` is the letter `t`, `[\.]` is a backslash or a dot, and `\.` is
still a literal dot. `known-issues.md` TD-B-ERE-BRACKET-BACKSLASH;
design-decisions §1055.

## What changed for you

Both apps compile user-typed patterns with `ere::Regex::new_flags`:

| app | where | what a user now sees |
|---|---|---|
| `logviewer` | `FilterState::pattern` | a search of `a\tb` finds `atb`, not a tab |
| `renamer` | `regex_replace` | a pattern `[\.]` also matches a backslash in a name |

Both now agree with what `grep -E` does with the same pattern, which is what
renamer's doc comment promises ("a pattern that works at the prompt works
here"). Your tests passed unchanged (123 and 132).

## If you want tabs back

Treating `\t` as a tab is a property of a *language* — GNU sed's and awk's —
not of the regex engine, so it belongs in front of the engine. To give your
users GNU sed's escapes, resolve them first with `ere::sed::regex(pattern)`
(`\t \n \a \f \r \v \xHH \oNNN \dNNN \cX`, inside brackets too) and compile
the result. That is a UI decision for you; nothing in lane B depends on it.
