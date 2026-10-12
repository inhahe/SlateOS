## 1236. No program contacts a website by default unless that is what it is for: the weather and the dictionary's online lookups wait to be turned on

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended option A; the operator changed it) · **Lane:** E

Answering E-Q2. The operator's answer, verbatim, from
`operator-answers/2026-10-09-open-questions-answers.txt`:

> E-Q2: Claude's recommendation, but I don't want weather reporting (or asking
> the user where they live for it) by default. And as for programs contacting
> other websites by default, I don't want that, so change those three things.
> If there any programs that obviously contact internet services by their very
> nature, those are okay to do by default.

**In short:** a program on SlateOS does not get in touch with another company's
server unless the user has turned that on -- with one exception, a program whose
whole purpose is to talk to the internet (a speed test, a chat client, a torrent
client), which may do so when used. The weather app does fetch forecasts now
(from Open-Meteo, the recommendation), but only once the user has switched
forecasts on: until then it neither asks where they live nor sends anything. The
dictionary keeps its thirty built-in words and looks a word up at dict.org only
after the user has turned online lookups on.

**How the answer was read.** "Those three things" is read as the three in the
operator's own sentence: weather reporting by default, asking where the user
lives by default, and programs contacting other websites by default. The last
sentence then decides each program: one that obviously contacts the internet by
its very nature may do so by default.

| Program | Contacts | Ruling | What changes |
|---|---|---|---|
| Weather | Open-Meteo, with the place's coordinates | not by default (said by name) | off until turned on; when on, it asks for a place and says which service it asks and what that service learns |
| Dictionary | dict.org, with the word | not by its nature: a dictionary works offline | online lookups off until turned on; a word not in the built-in list says so and says how to turn lookups on |
| Speed test | public test servers, when Start is pressed | by its nature: measuring the connection is the program | unchanged (§1215) |
| IRC client, torrent client, VPN manager, network manager, remote desktop | the servers the user names | by their nature | unchanged |
| Network scanner's WHOIS | the address registries, when the user asks who holds an address | by its nature: a network tool asking the network | unchanged |

This overrules §1214 in one respect: the dictionary still uses dict.org, but
only once the user has turned online lookups on. If a reading here is not what
the operator meant, the table is the thing to correct.

**What it obliges.** Lane E: the dictionary's online-lookup switch, off by
default and remembered; the weather app's forecasts from Open-Meteo behind a
switch that is off by default (`apps/weather`). The plain-text caveat in E-Q2
still holds: until the system can check a secure site's identity, a forecast
request goes unencrypted, and the weather app's switch says so.
