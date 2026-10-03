## 840. A gesture that is the absence of a keystroke gets its own request

**Date:** 2026-09-13. **Lane:** C. **Decided by:** Claude (autonomous).

**In short:** Alt+Shift switches keyboard layout on most desktops. It is not
a keystroke -- it is two modifiers going down and coming back up with nothing
pressed in between. The existing shortcut mechanism can only describe "this
key went down while those modifiers were held", so the nearest thing it could
say was "Shift went down while Alt was held", which is the first half of
Alt+Shift+Tab. Rather than bend the existing request to fit, the compositor
gained a second one that means only this.

**The alternative, and why not.** `grab_key(Key::LeftShift, alt)` is already
expressible and already compiles. Binding the layout switcher to it costs no
new protocol, no new event and no new state -- and fires the layout switch on
every reverse window-switch the user performs. The failure is silent and
intermittent: it only bites people who use both shortcuts, and it looks like
the layout changing at random.

A second alternative was to let `grab_key` take a modifier key and treat it
specially -- fire on release, and only if clean. That keeps one request, at
the price that two call sites spelling nearly the same thing mean entirely
different gestures, distinguishable only by whether the `key` field happens
to name a modifier. The one-request saving is not worth a foot-gun sitting in
the argument list of the busiest call in the protocol.

**What the second request costs.** Control version 11, input version 4, one
new `EventNotification` arm, one new `Event` arm, one `HashMap` and a
three-field state machine in the compositor. The state machine is the only
part with any subtlety: the modifier set is a *high-water mark*, because the
question is asked on a release, by which time the live set has already lost
the key being released.

**Delivered in addition to, never instead of.** A key grab takes the
keystroke away from the focused window; a chord does not. Withholding a
modifier's release would leave the focused window believing Alt is held for
the rest of the session -- the stuck-modifier bug `release_all_modifiers`
exists to prevent, and the same argument that governs `grabbed_modifiers`.
A chord is an observation about a gesture, not a claim on its keys.

**Two edges worth naming.** Caps Lock is a latch rather than something held,
so it counts as an ordinary keystroke and rules a chord out -- without that,
Alt+Caps+Shift would switch the layout. And a mouse button rules one out too:
Shift+click to extend a selection is the commonest way there is to hold Shift
on a desktop.
