## E-Q2 — [E] The weather app can fetch forecasts now. From whom, given that whoever supplies them learns where the user is? — Status: OPEN (raised 2026-09-26)

**In short:** The weather app shows no weather: it had no way to reach the
internet, and says so rather than inventing a forecast. Applications can now
open internet connections (the network scanner and the dictionary do), so it
can be made to work — but a forecast is always *for somewhere*, so whichever
company supplies it is told where the user is, every time the forecast is
refreshed. Which supplier, if any, is your call.

*Promoted from `deferred-questions.md`, where lane C parked it on 2026-09-18
until a program could make a network request at all. That trigger has fired.*

| | Option | *What changes* |
|---|---|---|
| **A** | Open-Meteo (a free forecast service that needs no account or key) | *Forecasts work as soon as the user names a place; Open-Meteo sees the place's coordinates at each refresh.* |
| **B** | A commercial service that needs a key | *The same, plus a key this project must obtain, ship and keep secret.* |
| **C** | Nothing by default; the user types in a service's address | *The app stays empty until someone configures it; no company is contacted unless the user chose it.* |
| **D** | Never fetch; remove the app | *One fewer app; nothing is ever sent.* |

**One more thing you should know, whichever you pick:** this system has no
way yet to check a secure (https) site's identity, so a request would go in
plain text — anyone on the same network could see which place was asked
about. Open-Meteo answers plain requests (checked 2026-09-26). Waiting for
secure connections is possible, but nothing on the roadmap delivers them
soon; the kernel has a TLS implementation that does not check certificates
(`kernel/src/net/tls.rs`), which is not the same thing.

**Recommendation: A**, with the app asking nothing until the user adds a
place, saying in the window which service it asks, and letting the user turn
it off. It is the option under which the app is useful; the user's own
action (adding a place) is what starts any sending.

**Related decisions already made, which your answer may overrule:** the
dictionary now looks up words its built-in list lacks at dict.org, when the
reader asks (design-decisions §1214), and the speed test measures against
public test servers when Start is pressed (§1215) -- both decided by Claude,
both contacting a third party only on the user's action. A looked-up word
reveals less than a location, and a speed test nothing of the user's, but it
is the same kind of choice. If you would rather no program contacted a third
party by default, say so here and all three change.

### If never answered

Nothing breaks: the weather app stays empty and says why, as today. Nothing
gets worse with time.

**Where it bites:** `apps/weather/src/main.rs` (`render_cannot_fetch`); the
fetch itself would sit on `net/httpclient`'s request/response parsing and a
plain `TcpStream`, as `userspace/pkg` does.
