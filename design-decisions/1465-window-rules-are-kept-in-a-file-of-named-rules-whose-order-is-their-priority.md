## 1465. Window rules are kept in a file of named rules whose order is their priority

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the rules that say what happens to a program's windows as they
open -- "the terminal remembers where it was", "chat opens on desktop 2" --
used to exist only while the desktop was running, so nothing a user set
survived a restart. They are now kept in `window-rules.yaml` in the user's
settings folder, which the desktop reads when it starts and again whenever
the file changes. Each rule is written under its own name, and the rule
nearer the top wins where two disagree. A rule the desktop cannot read -- a
misspelt word, a number out of range -- is not applied at all, and the
desktop says so in a notification rather than quietly ignoring it. A file
that says nothing about rules means the built-in ones; a file whose rules
section is empty means none.

**Where:** `gui/windowrules` (new crate: the rules, the engine, and
`src/file.rs`, the file), `gui/desktop/src/lib.rs`
(`DesktopShell::poll_window_rules`), `gui/desktop/src/session.rs`
(`adopt_window_rules`, read at start and on `SettingsChanged`).

```yaml
rules:
  Terminal remembers its place:
    app: terminal
    position: remember
    size: remember
  Notes on the second desktop:
    title-contains: notes
    desktop: 2
    taskbar: false
```

### Why a crate of its own

The rules are written by Settings and read by the desktop. Under §815 a
screen you open from a menu belongs to Settings, so the window-rules page goes
there (requested of lane E: `requests/c-e-a-window-rules-page-in-settings.md`);
Settings linking the whole desktop shell to read one file would invert the
dependency every other program keeps. So the model, the engine and the file
format are one crate both sides build on, and there is one copy of each.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Order is priority** | a `priority:` number per rule (what the model holds) | The file reads top to bottom the way it is applied; a user moving a rule up does not renumber the others; no two rules can tie without the user being able to see which wins. | A tie is no longer expressible -- harmless, since a tie was broken by insertion order anyway, which is file order. |
| **Each rule under its name** (a mapping) | a list of rules each with a `name:` | A name is how a user finds and talks about a rule, and YAML shows a mapping as one heading per rule. | Two rules cannot share a name: the second is refused with a problem saying so, rather than silently merged. |
| **A rule that cannot be read is left out whole** | applying the parts that read | `can-close: flase` must not become a window that can be closed, and a misspelt `app` must not become a rule about every window. Half a rule is a rule nobody wrote. | One typo turns off a whole rule -- which is why it is said aloud (below). |
| **No `rules` key = the defaults; `rules:` with nothing under it = none** | deciding by whether the file exists | The desktop notices changes through `settingsfile::Watcher`, which reads a missing file and an unreadable one as an empty document, exactly as `settingsfile::load` does. Whether the file exists is a question it cannot ask, so the line has to be drawn at something the document says. Writing always leaves the key, so removing every rule keeps none instead of bringing the defaults back. | A file of comments alone means the defaults, which a user who wrote only comments might not expect. The header Settings writes says what the file is for, and the first store puts the key in. |
| **A malformed `rules` (`rules: none`, a `-` list) is a problem and no rules** | treating it as the defaults | The user meant to say something; applying the defaults would be guessing what. | Rules the user had intended are off until they fix it -- and they are told. |
| **A rule read again unchanged keeps what it has done** | resetting every rule on every read | The file is read again on every change, and most changes are about other rules. Without this, adding an unrelated rule re-armed every once-rule already used -- the next terminal would go wherever the first one was sent -- and zeroed every rule's count of windows matched. "Unchanged" is the same name, match and actions; turning a rule off and on, or moving it, is not a change. | A once-rule edited back to exactly what it was stays used. To fire it again, delete it, or change it. |
| **A rule that cannot be read is told in a notification, each read** | the error stream only | Settings writes only rules it can read, so a problem is a hand edit, and a typo in a hand edit that quietly does nothing is the failure that most needs a voice. Said on each read that finds it, so a problem left in the file is said again each session rather than once and never again. | A user who leaves a broken rule in the file sees the notice at every sign-in. That is the problem asking to be fixed, and it stops when it is. |

### What did not change

The engine's behaviour for a window as it arrives (§569, and
`TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO` for which
actions are carried out). The shell's own rules panel, which nothing draws,
stays until Settings has its page and is then deleted, as §815 says of every
screen that moves.

The pipe-separated `export_config`/`parse_rule_line` format the model carried
is gone with the move: nothing outside its own tests ever called it, and the
YAML file replaces it. Its two properties worth keeping -- what is written
reads back as itself, and a criterion the reader does not know is refused
rather than widened to "any window" -- are tests of the file now.
