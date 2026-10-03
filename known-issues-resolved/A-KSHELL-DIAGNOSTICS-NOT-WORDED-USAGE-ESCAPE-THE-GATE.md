## A-KSHELL-DIAGNOSTICS-NOT-WORDED-USAGE-ESCAPE-THE-GATE — ✅ FIXED 2026-08-25 (lane A)

**In short:** the shell has a build-time check that stops a command from
printing an error message and then telling the caller it succeeded. The check
found the message by looking for the word "Usage:". Messages that start
"Unknown subcommand…", "Invalid port", "Unknown sensor" and so on say exactly
the same thing in different words, and the check did not look for them — so 49
of them scolded the user and then reported success. The check now fires on the
*kind* of message rather than one word of it, and all 49 are fixed.

**Where:** `kernel/src/kshell.rs`; the gate is
`scripts/check-usage-status.py`, whose trigger was

```python
USAGE = re.compile(r'(?:console_println!|shell_println!)\s*\(\s*"\s*[Uu]sage\b')
```

and is now

```python
USAGE = re.compile(
    r'(?:console_println!|shell_println!)\s*\(\s*"'
    r'(?:\s*[Uu]sage\b|Unknown\b|Unrecogni[sz]ed\b|Invalid\b|Use:)'
)
```

**How the 49 were counted, and where they were.** The checker's own forward
walk, with only that regex widened, reported 51 sites. Two are report lines
that merely begin with one of those words — `thumbcache`'s "Invalidated {}
entries for: …" (a *success* line) and `vlan stats`' "  Unknown drops:    {}"
(a field label). Both are excluded **structurally rather than by allowlist**:
the new words must *start* the string (a field label is indented inside its
report) and are matched with `\b` (so "Invalidated" is not an "Invalid"
anything). `Usage` keeps its leading-whitespace tolerance because `cmd_memcg`
has an indented one already sitting in `ALLOWED`, and removing the tolerance
would have silently dropped a site the gate was watching.

The remaining 49, in 29 functions: seven `Invalid container ID '{}'` paths in
`cmd_container`, six `Unknown sensor`/`Unknown level`/`Invalid hours` arms in
`cmd_wakesensor`, three `Invalid port` (`telnetd`, `sshd`, `netsyslog`),
`cmd_theme` ×3, `cmd_progmgr` ×3, `cmd_swapcfg` ×2, `cmd_useracct` ×2,
`cmd_sysdiag` ×2, `cmd_secpolicy` ×2, `cmd_kernelbuild` ×2, and singles in
`cmd_colorscheme`, `cmd_overlay`, `cmd_fcompress`, `cmd_fspolicy`, `cmd_atime`,
`cmd_fstrim`, `cmd_appregistry`, `cmd_hotkey`, `cmd_scriptlang`,
`cmd_netsettings`, `cmd_parental`, `cmd_elog`, `cmd_logpersist`, `cmd_vmguest`,
`cmd_taskbar`, `cmd_startmenu`, `cmd_container_network`.

**Why this is the interesting part and not a footnote.** It is the third time
the same methodological failure has produced shipped bugs in this one file. The
710-site sweep keyed on "a `Usage:` print followed by `return;`" and missed 87
that fall off the end of a `match` arm. The checker written to replace it keyed
on the *semantic* property but kept the *lexical* trigger, so it inherited the
blind spot one level down: it was complete over messages containing the word
"Usage", which is not the set it is trying to guard. A gate's trigger is part
of its rule, and a trigger derived from the wording of the last bug is a
syntactic sweep wearing a semantic hat. Recorded as design-decisions.md §299.

**How the fix was applied.** By a scratch script that **imported the checker
and took its site list from the checker's own output**, rather than
re-implementing "find the diagnostics" a second time — §297's rule, that two
descriptions of one set maintained separately differ silently. Here the
divergence would have been invisible in both directions: a site the sweep
missed would look like a new bug, and one it fixed that the gate did not care
about would look like nothing at all. The script also **refused shapes it did
not recognise and printed the refusals** instead of trying its best; exactly one
site came out as `MANUAL` — `cmd_fstrim`'s `Unknown mode`, whose print is the
block's tail expression with no trailing `;` — and was fixed by hand. The
script was deleted after use; the gate is what persists.

**Verified in both directions.** After the sweep both gates are green
(`check-usage-status.py`: 22 allowed, 0 conflations; `check-query-status.py`:
2 allowed). Run against the *pre-sweep* tree the widened checker reports all 49,
which is the positive control for the widened trigger — a checker nobody has
watched fail is a checker nobody knows works. The original historical control
still holds too: on `9251e5a3d^` it still reports `cmd_fcompress` and `cmd_elog`.

**What is still not covered, stated rather than implied.** A diagnostic opening
with none of the five words — `"no such mode: …"`, `"cannot parse …"`,
`"expected a number"` — is not nominated and will pass. That residue is real.
If a fourth instance of this defect appears, the question is not "which word do
we add" but whether the trigger should become a property of the *block* (does
this branch correspond to input the command rejected?) rather than of the
string. §299 records the estimate of what that costs.

**Tested by:** kshell self-test rung 61, which pins four of the 49 chosen for
the four *shapes* the sweep had to handle rather than for the commands they
live in — a `None =>` arm whose whole body was the print, the tail-expression
variant that the mechanical pass refused, a `let … else { print; continue; }`
inside a loop over arguments, and a catch-all `other =>`. Each is paired with
the query reading next door (`fstrim mode`, `theme accent` with no argument)
that must keep exit 0, because the failure mode of fixing this class is
over-applying it.
