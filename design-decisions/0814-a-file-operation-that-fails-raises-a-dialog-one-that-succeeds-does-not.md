## 814. A file operation that fails raises a dialog; one that succeeds does not

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** when the file manager could not do something you asked -- a
rename onto a name already taken, a folder it could not create, three of five
files it could not delete -- it used to mention that in the small grey line at
the bottom of the window, in exactly the same place and style as "Deleted 5
items". That line is replaced by the next thing you do, so the usual way to
find out an operation half-failed was to notice later that a file was still
there. Failures now also raise a dialog you have to dismiss. Successes still
just update the line at the bottom.

**What was there before.** Every operation ended by building one formatted
`String` and assigning it to `status_message`. `describe_outcome` produced
`"Deleted 5 item(s)"` and `"Deleted 3 item(s), 2 failed -- /etc/x: permission
denied"` through the same code path, and no caller could tell the two apart
without parsing the sentence back.

**The decision.** Operations return an `Outcome { message, failure: Option }`.
`report` puts `message` in the status bar always, and raises an
`AlertDialog::error` when `failure` is set. Every operation goes through it,
successes included -- see the note on why below.

**The tradeoff, which is real.** A dialog interrupts. A file manager that
raised one on every hiccup would train the user to dismiss dialogs without
reading them, which costs more than it saves and is the failure mode of every
program that over-warns.

Against that: the operations in question are *destructive and asked for
explicitly*. The user pressed Delete, confirmed a dialog, and then some of it
did not happen. There is no sensible reading in which that should be as quiet
as success. The asymmetry is the whole point -- it is not "important messages
get dialogs", which is a slope, but "you asked me to do something and I did
not do it", which is a small, checkable class.

**Why successes go through `report` too, when they raise nothing.** They were
originally left assigning `status_message` directly, since they have nothing
to raise. That made the rule have two implementations, and it showed up as a
test: `a_rename_that_works_raises_no_dialog` did not fail when `report` was
mutated to raise a dialog on *every* outcome, because the path it exercised
never called `report`. Routing successes through it turned that test from
decorative into load-bearing, and it now catches the mutation along with four
others. A rule with one implementation can be tested; a rule with two can only
be tested where it happens to be called.

**What was rejected.**

- *Colour the status line red.* Cheaper, and it does not interrupt. Rejected
  because the problem is not that the line is hard to read but that it is
  transient -- it is gone at the next click whatever colour it was.
- *A notification/toast.* Better than a status line, worse than a dialog for
  this specific case, and there is no notification service in the tree yet.
  Worth revisiting when there is; a dialog is the right answer for "the thing
  you just asked for did not happen" either way.
- *List every failure in the dialog.* A batch of two hundred failures makes an
  unreadable dialog. It names the first and gives the count; the full list
  belongs in a queue or log view, which does not exist yet.

**Where it bites.** `apps/explorer/src/main.rs` -- `Outcome`, `report`,
`describe_outcome`, and `Modal::Notice`.

**It is also a prerequisite.** The operator's deferred-filesystem-operations
request (roadmap 2.3) specifies for lane C "the file manager asking in the same
dialog that reports the failure". There was no such dialog. There is now, and
the deferral prompt has somewhere to go.
