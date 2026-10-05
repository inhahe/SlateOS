## 1214. The dictionary looks a word it lacks up at dict.org -- when asked, never as the reader types

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The dictionary program knew thirty words. It now also looks up
any other word in WordNet (a free English dictionary) on a public server,
dict.org, using the DICT protocol (RFC 2229 -- a plain-text way of asking a
dictionary server for a word). The word the reader asked about is sent to
that server, which this project does not run, so a lookup happens only when
the reader asks for one -- Enter on the offer, a click on a word -- and the
offer says where the word goes. Nothing is sent while typing.

### The choices

| Question | Chosen | Alternative | Why |
|---|---|---|---|
| Where definitions come from | WordNet, from dict.org over DICT | ship WordNet's data (about 30 MB) with the system | the data is not in the tree and packaging it is a larger decision than this program's; DICT is plain TCP, which an application can already open (`netscan` does), and WordNet's entries read cleanly into the shape the entry screen already draws. Revisit once packages can carry data files: a local copy needs no network and sends nothing. |
| When a lookup happens | on request: Enter on the "Look up ... online" row, a suggestion, a remembered word, or a click on a cross-reference the list lacks | as the reader types, like the built-in search | a lookup per keystroke sends every prefix of every word typed, including ones the reader thought better of, to someone else's server |
| Saying so | the offer reads "In WordNet, at dict.org -- the word is sent there"; an entry from it reads "From WordNet ..., at dict.org" | say nothing | a reader should know which words leave the machine, and which entries came from elsewhere |
| A miss | the words one edit away (`MATCH wn lev`), offered as rows | "not found" alone | a misspelling is the commonest reason for a miss |
| Which server | dict.org, one constant | a setting | nothing yet reads a setting for it; the constant is `online::DICT_ORG`, and `Dictionary::server` already carries it, so a setting is one line when wanted |

**Where it lives:** `apps/dictionary/src/online.rs` (the conversation and the
WordNet reader, tested against dict.org's own replies, recorded), and the
lookup rows, chips and answer handling in `apps/dictionary/src/main.rs`.

**How to reverse:** set `lookup` to a function that refuses; every row and
chip that offers a lookup then says why it could not.
