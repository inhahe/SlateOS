## 824. A login screen appears exactly when there is somebody to log in as

**Date:** 2026-09-08
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop now asks who you are before it lets you in. It asks
whenever the machine's list of user accounts has anybody on it. If that list
cannot be read at all — the file is missing, or unreadable — the desktop comes
up without asking, because there would be nobody to ask *about*: every name
would be refused, so the screen would be one that nothing could ever get past.
That is a machine nobody can use, which is worse than a machine that did not
ask.

**The decision.** `ShellSession::start` builds a `LoginScreen` when
`loginusers::offered_from_system()` returns a non-empty list, and does not when
it returns an empty one.

**Why the empty case cannot be "show it anyway".** The screen is answered by
`authlib`, which resolves names against `/etc/users.yaml`. That is the same file
the list is read from — deliberately, so a name on the screen is a name that can
answer. So if the file cannot be read, `authlib` answers `Unusable` to every
name, for every password, for ever. A screen shown in that state is not a
locked machine; it is a bricked one, with no recovery path that does not involve
another computer. Note that this is not the same as *no accounts in a readable
file*: that also yields an empty list, and also means there is nobody to
authenticate as.

**Why it is not "never show it", which is what the code did until now.** The
module has existed, fully written, since before the three-lane split and was
constructed by nothing — 2,417 lines of login screen that no keystroke could
reach (`known-issues.md`
`TD-C-FOUR-SHELL-FEATURES-ARE-BUILT-AND-NEVER-CONSTRUCTED`). §815 names the
login screen specifically as a thing that "stays in the shell and gets wired
up", so leaving it unreachable was not an option on the table.

**The rejected alternative: a setting.** An `appearance.yaml` key — "show a
login screen: yes/no" — is how a lot of desktops do it, and it was rejected for
two reasons. It puts a security-relevant default in the same file as the
wallpaper, where a user editing colours can turn off authentication by accident
and nothing tells them. And it answers a question the account database already
answers: a machine with accounts wants a login screen, a machine without one
cannot use it. A setting would let those two disagree, and the only way to
resolve the disagreement is to pick one — at which point the setting is doing
nothing except being wrong occasionally.

**What this does not decide.** Autologin. `LoginUser::autologin` is read from
the database and carried onto the screen, and `LoginScreen::autologin_user`
exists, but nothing acts on it yet: an account marked for autologin still gets
a prompt. Skipping the prompt is a second decision — it needs a rule for how a
user *escapes* an autologin to reach a different account, and a way to hold the
autologin at boot when the machine is being recovered — and doing it in the
same change as this one would have bundled a question with an answer. Logged in
`todo.txt`.

**A smaller call folded in.** With exactly one account the screen opens on the
password field rather than on a list of one row. A list the user must dismiss
to reach the field they were already looking at is a step that carries no
information. This matches the same file's existing treatment of Escape, which
clears the field rather than "going back" when there is only one account.
