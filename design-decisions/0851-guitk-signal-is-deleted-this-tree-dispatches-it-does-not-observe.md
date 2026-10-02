## 851. `guitk::signal` is deleted: this tree dispatches, it does not observe

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the toolkit contained a Qt-style signals-and-slots system — an
event mechanism where a widget announces "something happened" and any number of
listeners registered earlier are called back. It worked, it had twenty tests,
and in the whole tree nothing had ever used it. It is being deleted rather than
wired, because wiring it would give this project *two different ways* for a
click to reach the code that handles it, and the other way is used by 138
crates.

**What it was.** `Signal<T>`, `SignalGroup`, `EventBus`, `Rc<RefCell<..>>`
throughout, with a genuinely careful re-entrancy rule: a handler that emits the
signal it is handling has its emission deferred until the current one finishes,
so a loop cannot form. 853 lines. None of that is bad code.

**What the tree does instead.** Every application and every shell surface here
takes `fn handle_event(&mut self, event: &Event) -> Response` and returns what
the caller should do about it. The dispatch is direct, synchronous and owned:
there is no registry, no callback list, and no shared mutable state between a
sender and a listener. Counted rather than assumed: **138** crates under `apps/`
and `gui/` define a `handle_event`; **zero** connect a signal.

**The three crates that do hold `Box<dyn Fn ...>`** — `automator`, `dbviewer`,
`taskscheduler` — turned out to be render-function tables inside their own test
modules, sweeping panels to draw them. Not event callbacks, and not a
reinvention of this module. That check is the one that settles it: if something
had been rebuilding signals by hand, the answer would have been to wire this
instead.

**Why not keep it as a deliberate island.** The orphan ledger's own framing is
that a component with no consumer is either a feature the user cannot reach or
code to delete, and this one cannot become the first: an unreachable *widget*
becomes reachable when an application draws it, but an unreachable
*architecture* becomes reachable only by rewriting the architecture. Adopting
it anywhere would mean a click arriving through a callback in one program and
through a return value in the other 137 — two models of one fact at the scale
of the whole toolkit.

**Against deletion**, and it is real: 853 working lines with a subtle
re-entrancy guarantee are not free to write again. If this project ever grows
something genuinely asynchronous — a device-hotplug notification, a service
that appears mid-session — an observer mechanism is the right shape for it and
this one was well made. The answer is that git holds it: `design-decisions.md`
851 and the commit are enough to find it, and rewriting against a real consumer
would be better than fitting a real consumer to a mechanism written years
earlier with no consumer in mind.

**Precedent.** `gui/desktop/src/a11y.rs` was deleted outright on 2026-09-13 as
a dead parallel copy, and §810 removed a `Theme` of 32 widget-role colours for
being a second colour table. This is the same judgement a third time.

**What this closes.** `TD-C-SIX-TOOLKIT-WIDGETS-ARE-WRITTEN-TESTED-AND-USED-BY-NOTHING`
listed `signal` with the note "least obviously needed; check what it is before
deciding". It has now been checked: it is an observer mechanism, and this tree
does not observe.
