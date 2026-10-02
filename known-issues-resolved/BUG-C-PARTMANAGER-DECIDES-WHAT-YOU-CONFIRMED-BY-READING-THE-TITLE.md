## BUG-C-PARTMANAGER-DECIDES-WHAT-YOU-CONFIRMED-BY-READING-THE-TITLE — fixed in `e8e46e7c3`

**In short:** The partition manager asks "are you sure?" before it deletes a
partition, wipes a partition table, or writes queued changes to disk. When you
click the confirm button, it works out *which* of those three you just agreed to
by searching the dialog's **title text** for the words `Partition Table`,
`Delete`, or `Apply`. So the program's behaviour depends on the exact English
wording shown on screen. Reword a title, translate the app, or add a fourth
confirmation whose title happens to contain the word "Delete", and the wrong
thing happens — or, more likely, nothing happens at all, silently, with the
dialog closing as though it had worked.

**Where it lives:** `apps/partmanager/src/main.rs` — `handle_confirm_accepted`,
around line 3769:

```rust
let title = match &app.dialog { ActiveDialog::Confirm(d) => d.title.clone(), _ => String::new() };
if title.contains("Partition Table") { … }
else if title.contains("Delete") { … }
else if title.contains("Apply") { app.apply_operations(); }
```

**How to reproduce:** change the delete confirmation's title from
`"Delete Partition"` to `"Remove Partition"` — a change that looks purely
cosmetic and that no test would flag — and the confirm button becomes a no-op.
The dialog closes, the partition stays, and nothing is reported.

**Why it is not caught:** the three titles are constructed a few hundred lines
away from where they are matched, so nothing local ties the two together. There
is no compile-time relationship between the string written and the string
searched for; the `else if` chain simply falls off the end when none matches.

**What the proper fix looks like.** Carry the intent as data rather than as
display text. A `ConfirmIntent` enum (`CreatePartitionTable`, `DeletePartition`,
`ApplyOperations`) stored alongside the dialog — `ActiveDialog::Confirm { dialog,
intent }` — and a `match intent` in place of the string chain. Then the title is
free to say anything, a fourth confirmation must name its own intent to compile,
and a `match` with no wildcard arm makes the missing case a build error rather
than a silent no-op. This is planned as part of migrating the app off its
hand-rolled dialog onto `guitk::modal::AlertDialog`, which is where the intent
value has to live anyway once the app stops owning the dialog struct.

**Why it was worth fixing before it fired:** it worked, because the three
titles happened to contain the three words. It was a trap for the next edit,
not a live failure — but the failure mode when it sprang is the worst kind: a
destructive action's confirmation appearing to succeed while doing nothing.

**The fix**, landed with the migration onto `guitk::modal::AlertDialog`: the
`ConfirmIntent` enum described above, stored as `ActiveDialog::Confirm {
dialog, intent }`, and a wildcard-free `match intent` in
`handle_confirm_accepted`. A regression test —
`a_reworded_title_does_not_change_what_the_confirmation_does` — opens a delete
confirmation titled `"Remove Volume"`, which contains none of the three magic
words, and asserts the delete is still queued.
