## 778. A private key the user *named* and cannot be used stops the client; a missing default one does not

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `ssh` can log in with a password you type, or with a *key file* —
a file on your disk that proves who you are without a password. You can point at
one with `ssh -i mykey host`, or `ssh` can quietly look in the usual place
(`~/.ssh/id_ed25519`) without being asked. The question is what should happen
when that file is missing or damaged. OpenSSH prints a warning and asks for a
password instead. This client instead **refuses to continue when you named the
file yourself**, and only stays quiet when it was guessing.

**The three cases and what each does now:**

| | file is not there | file is there but unusable |
|---|---|---|
| you wrote `-i` / `IdentityFile=` | error, naming the file | error, naming the file and the fault |
| you wrote neither | nothing at all | warning on stderr, then password |

**Why not just warn, as OpenSSH does.** Because the warning is not the last
thing that happens — the password prompt is. A user who typed `-i deploy_key`
and then sees `alice@host's password:` has, in the ordinary case, simply typed
their password and got in. The connection worked, so nothing ever told them the
key was not used; the same invocation in a script, where nobody is watching,
either hangs on a prompt or falls through to an interactive method that also
fails. "Why does it keep asking for my password" is a well-worn OpenSSH support
question, and this is one of its sources.

Refusing costs a retry with the path fixed, and buys an accurate answer the
first time. `-i` is an unambiguous statement of intent: the user knows the file
exists and knows which one they mean. Silently doing something else with their
credentials is not a service.

**Why the default path is the opposite.** Most invocations of this client have
no key at all. A message about `~/.ssh/id_ed25519` on every one of those is
noise that trains the reader to skim, and it names a file the user never
mentioned. So an absent default is passed over in silence.

But a default that *exists and cannot be read* is a third thing, not a second
one: something put a file there meaning it to be a key. That gets a warning —
the user should know their key is broken — without becoming a refusal to
connect, since they never asked for that file by name.

**Alternatives considered:**

- *Warn in all four cells (OpenSSH's behaviour).* **What changes:** `ssh -i
  typo host` prompts for a password and logs you in, saying only `Warning:` a
  few lines earlier. Rejected above.
- *Error in all four cells.* **What changes:** `ssh host` on a machine with no
  key at all fails instead of prompting. Wrong: password-only is a supported
  configuration, and the client guessed the path.
- *Error on an unusable default too, warn only on an absent one.* **What
  changes:** a stale or truncated `~/.ssh/id_ed25519` makes every `ssh` on the
  machine fail until it is deleted. Tempting, because a broken key is a real
  fault the user should fix — but it converts one bad file into a total loss of
  the tool, for a file the user did not point at. The warning says the same
  thing without that.

**What this does not cover:** a key file that is *encrypted with a passphrase*.
No tool in this tree can read one, so today it lands in the "unusable" column —
though with an accurate message, since the decoder recognises an encrypted
container and says so rather than calling the file damaged. Erroring is the
right answer only while there is no passphrase prompt to offer; once there is,
an encrypted file must prompt rather than fail, and this rule needs revisiting
alongside it. See known-issues.md
`TD-B-NO-SSH-TOOL-CAN-READ-A-PASSPHRASE-PROTECTED-PRIVATE-KEY`.

**Where it lives:** `userspace/ssh/src/lib.rs`, `load_identity` and
`load_identity_from`. The pair is split so the *default*-path policy is testable
without reading the developer's own `~/.ssh`.
