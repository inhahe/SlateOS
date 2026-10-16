## 1073. What SlateOS installs is decided by the installer's question, and "desktop" means everything but the network servers

**Date:** 2026-10-11 (answered 2026-10-10)
**Lane:** B
**Decided by:** Operator (answering B-Q23 with "Claude's recommendation":
option E, with option B as what "desktop" means). Given in chat to lane F's
session and relayed by it (`operator-answers/2026-10-10-chat-to-lane-f.txt`);
the operator's answers file of 2026-10-09 had labelled the same answer
"B-Q24".

**In short:** SlateOS has about 470 programs. Which of them a computer gets is
decided when SlateOS is installed, by the question the installer already asks
-- what is this machine for? A desktop gets everything except the six programs
that other computers connect *to* (`sshd`, `ftpd`, `telnet`, `inetd`, `ntpd`
and `finger`, the six B-Q23 listed), which a desktop does not need and which
are a way in even when idle. Nothing is deleted: a program left out can be installed later with the
package manager. Until the installer exists, the disk image keeps carrying
everything, as decided in §1053.

**The operator's answer, verbatim:** "yes, for b-q23, i want claude's
recommendation."

### What each answer to the installer's question installs

| the machine is for | installed | left out, installable later |
|---|---|---|
| desktop | every program except the network servers | the six network servers |
| development | the same as desktop -- the developer tools are already in it | the six network servers |
| gaming | the same as desktop | the six network servers |
| server | the command-line tools, the system tools and the network servers | the desktop applications and the games |

`design.txt` has the installer ask this question to tune memory and
scheduling; it now picks the program set as well, so the two cannot disagree.

### Why not one set for everybody

- **Everything for everyone** (the question's option A) makes every desktop
  run servers it never asked for.
- **Leaving the developer tools out of a desktop** (option C) puts "install
  the developer package first" in front of someone at exactly the moment they
  are trying something; `make` and `gdb` cost little.
- **A lean desktop without games** (option D) is a smaller image for a cost
  every user who wants a game pays.

The question is per machine, which the one image cannot be; so the image holds
everything and each installation takes its part.

### Where it is done

- **The package groups** -- one per answer above -- are lane B's, in `pkg/`'s
  package definitions (`roadmap.md`, lane B's backlog).
- **The image** is unchanged: §1053 (everything that builds is staged) stands
  until an installer exists to choose.
- **The installer's question** is where the choice is made; the installer has
  no owner yet. When it is built, its "what is this machine for" answer selects
  one of the groups.
- The six Linux security frameworks that configure what SlateOS does not have
  (`apparmor`, `selinux`, `audit`, `firejail`, `polkit`, `sanitize`) are a
  separate matter, checked one program at a time under §1006.
