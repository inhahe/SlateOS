### TD-SORT-C-LOCALE-ONLY-COLLATION — the one that is not `sort`'s to fix

**In short:** `sort` compares text one byte at a time and knows only English
month names. In English that is nearly always the order you wanted; in a
language with accents it is not — `é` sorts after `z` instead of next to `e`,
because its bytes are numerically larger. The fix does not belong in `sort`: the
system has no collation tables (the per-language rules for which letter comes
before which) for anything to consult.

This is not a `sort` defect and must not be "fixed" inside it. When SlateOS
grows a locale layer, `sort`, `ls`, `join`, `comm`, `uniq` and `look` all need
to move onto it together — an ordering that disagrees between `sort` and `join`
silently breaks `join`, which requires both inputs sorted *the same way*.
`scripts/sort-diff.sh` pins `LC_ALL=C` so the harness compares like with like;
when a locale layer lands, that pin is the first thing to revisit.
