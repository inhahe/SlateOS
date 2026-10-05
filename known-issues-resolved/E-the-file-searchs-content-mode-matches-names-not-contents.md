### [E] The file search's Content mode matches names, not contents -- 2026-09-25 -- **FIXED 2026-09-25**
**Status:** FIXED 2026-09-25 -- the mode reads the files, on a worker thread; see the end of this entry

**In short:** the file search offers four ways to match a query -- name, glob,
regex, content -- and the fourth is not what it says. Choosing "Content" and
typing a word finds files whose *names* contain the word, exactly as "Name"
does, because the matcher's Content arm reads "Content search would need actual
file reading. For now, match against name as fallback". A person searching
their documents for a phrase is told, silently, that no document contains it.

**Where.** `apps/filesearch/src/main.rs`, `SearchCriteria::matches`, the
`SearchMode::Content` arm.

**The proper fix** reads the files: a worker thread searching the candidates
the other filters leave, bounded in the size of file it reads, streaming
matches back while the window keeps drawing -- and cancelled when the query
changes, because content search is too slow to run to completion on every
keystroke the way a name match can. Until then the mode's label should not
promise what it does not do.

**Fixed as described, the same day.** `ContentSearch` reads the candidates the
other filters leave (files only) on a worker thread and streams matches back;
the window takes them in on a 50 ms clock that runs only while the worker does
(it cannot wake the window: `requests/e-f-wake-an-application-for-its-own-descriptor.md`),
sorted into the table as they arrive. A new query, a changed filter or a change
of mode drops the search, which stops the worker at its next file. Files over
16 MiB are skipped and the skip is counted in the status line. Without match
case, a UTF-8 file is lower-cased as text (`ÉCOLE` finds `école`) and any other
file has only its ASCII letters folded -- never a lossy decode. Seven tests and
five mutations in `apps/filesearch/mutate.py`.
