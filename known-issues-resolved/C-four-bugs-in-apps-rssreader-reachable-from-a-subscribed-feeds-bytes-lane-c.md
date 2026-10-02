## Four bugs in `apps/rssreader` reachable from a subscribed feed's bytes (lane C)

**Status: FIXED 2026-08-16** (lane C), commits `9a5b33aa9`, `bfeab0bfe` and the
`wrap_text` commit below. Each was verified by reverting the fix and watching
the named test fail with the message quoted here — none is argued from reading
the diff.

All four are reachable from **remote data**: an RSS reader's input is a URL the
user subscribed to and everything the publisher serves from it. There is no
crafted-file step, no privilege boundary to cross and nothing for the user to
click. Three of the four are denial of service against the reader process
rather than memory unsafety, but the first one is an abort with no unwinding,
so nothing at any call site can turn it into an error message.

### 1. Unbounded XML recursion — a feed can abort the process

`XmlParser::parse_element` recursed once per level of element nesting with no
cap. Measured in a debug build on a 2 MiB thread, nesting overflowed the stack
somewhere between 512 and 1024 levels, so roughly **seven kilobytes** of
`<a><a><a>…` is enough. A stack overflow is not a `Result` and not a panic: on
Windows the test binary died with `STATUS_STACK_OVERFLOW` (exit `0xc00000fd`),
no unwinding, nothing to catch.

Fixed with `MAX_XML_DEPTH = 100` and a `parse_child` wrapper that owns both the
increment and the decrement — kept in its own three-line function specifically
so the two cannot drift apart behind one of `parse_element`'s six early
returns. 100 is far below the cliff and unreachable by accident: RSS and Atom
nest four or five deep, and the deepest thing this program parses is an OPML
folder tree.

Pinned by `deeply_nested_elements_are_refused_rather_than_overflowing_the_stack`
(depth 2000), plus `nesting_within_the_limit_still_parses` and
`many_siblings_do_not_count_as_depth` (5000 siblings under one `<channel>`) so
the cap cannot be satisfied by a parser that simply refuses more than 100 of
anything. Reverting the guard takes the whole test binary down rather than
reporting a failure — which is as visible as a failure gets.

### 2. Two unbounded calendar loops — a `<pubDate>` can hang the reader

Date parsing ran `for y in 1970..year` to convert a year to days, and
`format_timestamp` ran the mirror loop back down. `year` was parsed straight out
of `<pubDate>` into a `u64` with no range check. Measured:
`"4000000-01-01"` took 107 ms for a single date; `"18446744073709551615-01-01"`
never returned. Both directions are reachable — a merely large year parses
slowly into a huge timestamp, which the formatter then walks back down on every
frame that draws the article.

Replaced with the two Hinnant closed forms, a `YEAR_RANGE` of `1970..=9999`,
and an `Option` return so an out-of-range date is refused rather than
approximated. Pinned by `an_absurd_year_is_refused_instead_of_hanging_the_parser`;
reverting the bound makes that test time out at 60 s.

Fixed as a side effect: impossible dates and clock readings
(`2023-02-29`, month 13, day 0, hour 25, minute 60) used to roll forward
silently, and pre-1970 dates answered as though the year were 1970. Leap
second 60 is still accepted, deliberately — it is a real reading, and folding
it into the following minute is closer than discarding the article's date.

### 3. `wrap_text` split a `&str` at a byte offset — a long word aborts the view

```rust
let max_chars = (max_width / char_width) as usize;
…
while remaining.len() > max_chars {
    let (chunk, rest) = remaining.split_at(max_chars);
```

`max_chars` is a count of characters, derived from a pixel width; `str::len`
and `str::split_at` are both in bytes. `split_at` does not truncate or round to
a boundary — it panics:

```
end byte index 77 is not a char boundary; it is inside '本' (bytes 75..78)
```

The caller is the article content view, drawing the article body, so any feed
containing one long non-ASCII word aborted the render on every frame that
reached it. Fixed by measuring in characters throughout, with a `split_at_chars`
helper that converts a character count to a byte offset via `char_indices`.

The same confusion was a **display** bug independent of the panic: the fit test
`current_line.len() + 1 + word.len() <= max_chars` compared bytes to a
character budget, so text wrapped at a fraction of its intended width whenever
it was not ASCII. `wrapping_counts_characters_not_bytes` wraps the same shape
of text in Latin and Greek and demands identical line widths; against the old
code it reports Latin `[23, 7]` against Greek `[11, 11, 7]` — under half.

A third defect was found while fixing these and is guarded by the same rewrite:
the break loop re-measured the untaken remainder on every pass, which is
quadratic in a word whose length the feed chooses. The loop now ends on the
emptiness of the tail, which is linear.
Pinned by `a_long_non_ascii_word_is_wrapped_rather_than_panicking`,
`wrapping_counts_characters_not_bytes` and
`a_very_long_word_wraps_without_dropping_text`.

### 4. `OfflineCache::cache_article` emptied the cache and then stored nothing

Not a panic — a plain logic defect, and the sweep's "the guard is downstream of
what it guards" pattern with a user-visible cost. The check rejecting an
article larger than the entire cache ran *after* the eviction loop, so an
oversized article drove that loop until the cache was empty and was then
declined. One oversized article in a feed therefore wiped the user's whole
offline cache on **every refresh** and cached nothing in its place.

Underneath it, a second one: the previous copy of an article was not removed
before making room, so re-caching an article — which a refresh does for every
article it re-reads — evicted as though the old and new copies had to coexist.
Both fixed by moving the size check to the first line and removing the old
entry before the loop.
