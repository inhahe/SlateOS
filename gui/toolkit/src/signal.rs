#![allow(dead_code)]
//! Signals and slots event system for widget-to-widget communication.
//!
//! Inspired by Qt's signals/slots but designed for Rust's ownership model.
//! This is a single-threaded system using `Rc<RefCell<...>>` for shared state.
//!
//! # Architecture
//!
//! - **Signal<T>** — a typed event emitter. Connected handlers are called
//!   synchronously when the signal is emitted.
//! - **Slot** — any `Fn(&T)` or `FnMut(&T)` closure connected to a signal.
//! - **SignalGroup** — manages multiple connections for bulk disconnect.
//! - **EventBus** — a named signal registry for decoupled communication.
//!
//! # Re-entrancy Safety
//!
//! If a handler emits the same signal it was called from, the emission is
//! deferred until the current emission completes. This prevents infinite
//! recursion and ensures all handlers for the current emission run first.

use core::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// ConnectionId
// ---------------------------------------------------------------------------

/// Opaque handle identifying a signal connection. Used to disconnect later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

/// Global monotonically-increasing connection counter.
//
// The initializer is already a `const { ... }` block — exactly the form
// `missing_const_for_thread_local` asks for — but the lint still fires on
// rust-1.95 because it doesn't recognise the macro-emitted form. Suppress
// at the function level since `#[allow]` on a macro invocation is ignored.
#[allow(clippy::missing_const_for_thread_local)]
fn next_connection_id() -> ConnectionId {
    thread_local! {
        static COUNTER: Cell<u64> = const { Cell::new(1) };
    }
    COUNTER.with(|c| {
        let id = c.get();
        c.set(id.wrapping_add(1));
        ConnectionId(id)
    })
}

// ---------------------------------------------------------------------------
// Slot storage
// ---------------------------------------------------------------------------

/// Shared `Fn(&T)` slot handler.
type FnSlot<T> = Rc<dyn Fn(&T)>;
/// Shared `FnMut(&T)` slot handler — needs `RefCell` for interior mutability.
type FnMutSlot<T> = Rc<RefCell<dyn FnMut(&T)>>;

/// A callable handler — wraps either Fn or FnMut behind Rc so we can
/// snapshot the slot list without holding the RefCell borrow during calls.
enum SlotHandler<T: 'static> {
    /// Immutable handler: `Fn(&T)`.
    Immutable(FnSlot<T>),
    /// Mutable handler: `FnMut(&T)` behind RefCell for interior mutability.
    Mutable(FnMutSlot<T>),
}

// Manual Clone impl to avoid requiring T: Clone (we only clone the Rc, not T).
impl<T: 'static> Clone for SlotHandler<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Immutable(f) => Self::Immutable(Rc::clone(f)),
            Self::Mutable(f) => Self::Mutable(Rc::clone(f)),
        }
    }
}

impl<T: 'static> SlotHandler<T> {
    fn call(&self, value: &T) {
        match self {
            Self::Immutable(f) => f(value),
            Self::Mutable(f) => {
                let mut handler = f.borrow_mut();
                handler(value);
            }
        }
    }
}

/// A connected slot entry in the signal's handler table.
struct SlotEntry<T: 'static> {
    id: ConnectionId,
    handler: SlotHandler<T>,
}

// ---------------------------------------------------------------------------
// Signal<T>
// ---------------------------------------------------------------------------

/// Internal state of a signal, held behind `Rc<RefCell<...>>`.
struct SignalInner<T: 'static> {
    slots: Vec<SlotEntry<T>>,
    /// True while we are currently emitting (re-entrancy guard).
    emitting: bool,
    /// Deferred emissions queued due to re-entrancy.
    deferred: Vec<T>,
}

/// A typed event emitter. When emitted, all connected slots are called
/// with the value in FIFO connection order.
pub struct Signal<T: 'static> {
    inner: Rc<RefCell<SignalInner<T>>>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: 'static> Signal<T> {
    /// Create a new signal with no connections.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(SignalInner {
                slots: Vec::new(),
                emitting: false,
                deferred: Vec::new(),
            })),
        }
    }

    /// Connect an immutable handler (`Fn(&T)`).
    /// Returns a `ConnectionId` that can be used to disconnect later.
    pub fn connect<F>(&self, handler: F) -> ConnectionId
    where
        F: Fn(&T) + 'static,
    {
        let id = next_connection_id();
        let mut inner = self.inner.borrow_mut();
        inner.slots.push(SlotEntry {
            id,
            handler: SlotHandler::Immutable(Rc::new(handler)),
        });
        id
    }

    /// Connect a mutable handler (`FnMut(&T)`).
    /// Returns a `ConnectionId` that can be used to disconnect later.
    pub fn connect_mut<F>(&self, handler: F) -> ConnectionId
    where
        F: FnMut(&T) + 'static,
    {
        let id = next_connection_id();
        let mut inner = self.inner.borrow_mut();
        inner.slots.push(SlotEntry {
            id,
            handler: SlotHandler::Mutable(Rc::new(RefCell::new(handler))),
        });
        id
    }

    /// Disconnect a handler by its `ConnectionId`.
    /// Returns `true` if a connection was found and removed.
    pub fn disconnect(&self, id: ConnectionId) -> bool {
        let mut inner = self.inner.borrow_mut();
        let len_before = inner.slots.len();
        inner.slots.retain(|entry| entry.id != id);
        inner.slots.len() < len_before
    }

    /// Returns `true` if at least one slot is connected.
    pub fn is_connected(&self) -> bool {
        !self.inner.borrow().slots.is_empty()
    }

    /// Returns the number of active connections.
    pub fn connection_count(&self) -> usize {
        self.inner.borrow().slots.len()
    }

    /// Emit the signal, calling all connected handlers with the given value.
    ///
    /// If called re-entrantly (a handler emits the same signal), the emission
    /// is deferred until the current emission completes.
    pub fn emit(&self, value: T)
    where
        T: Clone,
    {
        // Check if we are already emitting (re-entrancy).
        {
            let mut inner = self.inner.borrow_mut();
            if inner.emitting {
                inner.deferred.push(value);
                return;
            }
            inner.emitting = true;
        }

        self.dispatch(&value);

        // Process deferred emissions (from re-entrant calls).
        loop {
            let deferred = {
                let mut inner = self.inner.borrow_mut();
                if inner.deferred.is_empty() {
                    inner.emitting = false;
                    break;
                }
                core::mem::take(&mut inner.deferred)
            };
            for deferred_value in &deferred {
                self.dispatch(deferred_value);
            }
        }
    }

    /// Internal: snapshot the handler list and call each handler.
    ///
    /// The borrow on `inner` is released before calling handlers, so handlers
    /// are free to connect/disconnect/emit without causing a borrow conflict.
    fn dispatch(&self, value: &T) {
        // Take a snapshot of handlers (cheap Rc clones).
        let handlers: Vec<SlotHandler<T>> = {
            let inner = self.inner.borrow();
            inner.slots.iter().map(|e| e.handler.clone()).collect()
        };

        // Call each handler without holding the borrow.
        for handler in &handlers {
            handler.call(value);
        }
    }
}

impl<T: 'static> Default for Signal<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SignalForwarder — connect one signal's output to another signal's input
// ---------------------------------------------------------------------------

/// Connect signal `source` to signal `target` so that when `source` emits,
/// `target` also emits the same value. Returns the connection id on `source`.
pub fn forward<T: Clone + 'static>(source: &Signal<T>, target: &Signal<T>) -> ConnectionId {
    let target_clone = target.clone();
    source.connect(move |value: &T| {
        target_clone.emit(value.clone());
    })
}

// ---------------------------------------------------------------------------
// SignalGroup
// ---------------------------------------------------------------------------

/// A closure that disconnects one specific connection from its owning signal,
/// returning true if a connection was actually removed.
type Disconnector = Box<dyn Fn(ConnectionId) -> bool>;

/// Manages multiple signal connections for bulk disconnect.
/// Useful for widget cleanup — disconnect all handlers at once.
pub struct SignalGroup {
    connections: Vec<(Disconnector, ConnectionId)>,
}

impl SignalGroup {
    /// Create a new empty signal group.
    pub fn new() -> Self {
        Self {
            connections: Vec::new(),
        }
    }

    /// Track a connection. When `disconnect_all` is called, this connection
    /// will be removed from its signal.
    pub fn track<T: 'static>(&mut self, signal: &Signal<T>, id: ConnectionId) {
        let inner = Rc::clone(&signal.inner);
        let disconnector: Disconnector = Box::new(move |conn_id| {
            let mut state = inner.borrow_mut();
            let len_before = state.slots.len();
            state.slots.retain(|entry| entry.id != conn_id);
            state.slots.len() < len_before
        });
        self.connections.push((disconnector, id));
    }

    /// Disconnect all tracked connections.
    pub fn disconnect_all(&mut self) {
        for (disconnector, id) in self.connections.drain(..) {
            let _ = disconnector(id);
        }
    }

    /// Number of tracked connections.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Whether the group has no tracked connections.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

impl Default for SignalGroup {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// EventBus
// ---------------------------------------------------------------------------

/// Type-erased signal entry in the event bus.
struct BusSignal {
    /// `TypeId` of the signal's value type `T`.
    type_id: TypeId,
    /// The actual `Signal<T>` stored as `Any`.
    signal: Box<dyn Any>,
}

/// A named signal registry for decoupled widget communication.
///
/// Emitters and receivers don't need direct references to each other;
/// they communicate through named signals registered on the bus.
pub struct EventBus {
    signals: RefCell<HashMap<String, BusSignal>>,
}

/// Error type for `EventBus` operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusError {
    /// The named signal does not exist.
    NotFound(String),
    /// The type provided does not match the signal's registered type.
    TypeMismatch {
        name: String,
        expected: TypeId,
        got: TypeId,
    },
}

impl core::fmt::Display for BusError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "signal '{}' not found in event bus", name),
            Self::TypeMismatch { name, .. } => {
                write!(f, "type mismatch for signal '{}' in event bus", name)
            }
        }
    }
}

impl EventBus {
    /// Create a new empty event bus.
    pub fn new() -> Self {
        Self {
            signals: RefCell::new(HashMap::new()),
        }
    }

    /// Register a new named signal of type `T`.
    /// If a signal with this name already exists, it is replaced.
    pub fn register_signal<T: Clone + 'static>(&self, name: &str) {
        let signal = Signal::<T>::new();
        let entry = BusSignal {
            type_id: TypeId::of::<T>(),
            signal: Box::new(signal),
        };
        self.signals.borrow_mut().insert(name.to_string(), entry);
    }

    /// Emit a value on a named signal.
    /// Returns an error if the signal doesn't exist or the type doesn't match.
    pub fn emit<T: Clone + 'static>(&self, name: &str, value: T) -> Result<(), BusError> {
        let signals = self.signals.borrow();
        let entry = signals
            .get(name)
            .ok_or_else(|| BusError::NotFound(name.to_string()))?;

        if entry.type_id != TypeId::of::<T>() {
            return Err(BusError::TypeMismatch {
                name: name.to_string(),
                expected: entry.type_id,
                got: TypeId::of::<T>(),
            });
        }

        let signal = entry
            .signal
            .downcast_ref::<Signal<T>>()
            .expect("type_id matched but downcast failed — this is a bug");
        signal.emit(value);
        Ok(())
    }

    /// Subscribe to a named signal with an immutable handler.
    /// Returns an error if the signal doesn't exist or the type doesn't match.
    pub fn subscribe<T: Clone + 'static, F: Fn(&T) + 'static>(
        &self,
        name: &str,
        handler: F,
    ) -> Result<ConnectionId, BusError> {
        let signals = self.signals.borrow();
        let entry = signals
            .get(name)
            .ok_or_else(|| BusError::NotFound(name.to_string()))?;

        if entry.type_id != TypeId::of::<T>() {
            return Err(BusError::TypeMismatch {
                name: name.to_string(),
                expected: entry.type_id,
                got: TypeId::of::<T>(),
            });
        }

        let signal = entry
            .signal
            .downcast_ref::<Signal<T>>()
            .expect("type_id matched but downcast failed — this is a bug");
        Ok(signal.connect(handler))
    }

    /// Check if a named signal exists.
    pub fn has_signal(&self, name: &str) -> bool {
        self.signals.borrow().contains_key(name)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Common signal event types
// ---------------------------------------------------------------------------

/// Emitted when a button or clickable widget is clicked.
#[derive(Clone, Debug)]
pub struct Clicked;

/// Emitted when a value changes (sliders, spinboxes, etc.).
#[derive(Clone, Debug)]
pub struct ValueChanged<T: Clone> {
    /// The new value.
    pub value: T,
}

/// Emitted when text content changes (text inputs, text areas).
#[derive(Clone, Debug)]
pub struct TextChanged {
    /// The new text content.
    pub text: String,
}

/// Emitted when selection changes (lists, grids, trees).
#[derive(Clone, Debug)]
pub struct SelectionChanged {
    /// Indices of selected items.
    pub selected: Vec<usize>,
}

/// Emitted when a toggle widget changes state (checkboxes, switches).
#[derive(Clone, Debug)]
pub struct Toggled(pub bool);

/// Emitted when a text input is submitted (Enter pressed).
#[derive(Clone, Debug)]
pub struct Submitted(pub String);

/// Emitted when an item is activated (double-click in list/tree).
#[derive(Clone, Debug)]
pub struct Activated(pub usize);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_connect_emit() {
        let signal = Signal::new();
        let received = Rc::new(Cell::new(0i32));
        let r = Rc::clone(&received);
        signal.connect(move |val: &i32| {
            r.set(*val);
        });
        signal.emit(42);
        assert_eq!(received.get(), 42);
    }

    #[test]
    fn multiple_handlers() {
        let signal = Signal::new();
        let count = Rc::new(Cell::new(0u32));

        let c1 = Rc::clone(&count);
        signal.connect(move |_: &()| {
            c1.set(c1.get() + 1);
        });

        let c2 = Rc::clone(&count);
        signal.connect(move |_: &()| {
            c2.set(c2.get() + 1);
        });

        signal.emit(());
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn handler_ordering_is_fifo() {
        let signal = Signal::new();
        let order = Rc::new(RefCell::new(Vec::new()));

        let o1 = Rc::clone(&order);
        signal.connect(move |_: &()| {
            o1.borrow_mut().push(1);
        });

        let o2 = Rc::clone(&order);
        signal.connect(move |_: &()| {
            o2.borrow_mut().push(2);
        });

        let o3 = Rc::clone(&order);
        signal.connect(move |_: &()| {
            o3.borrow_mut().push(3);
        });

        signal.emit(());
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn disconnect_removes_handler() {
        let signal = Signal::new();
        let count = Rc::new(Cell::new(0u32));
        let c = Rc::clone(&count);
        let id = signal.connect(move |_: &()| {
            c.set(c.get() + 1);
        });

        signal.emit(());
        assert_eq!(count.get(), 1);

        assert!(signal.disconnect(id));
        signal.emit(());
        assert_eq!(count.get(), 1); // not called again
    }

    #[test]
    fn disconnect_returns_false_for_unknown_id() {
        let signal = Signal::<()>::new();
        assert!(!signal.disconnect(ConnectionId(9999)));
    }

    #[test]
    fn connection_count_tracking() {
        let signal = Signal::<()>::new();
        assert_eq!(signal.connection_count(), 0);
        assert!(!signal.is_connected());

        let id1 = signal.connect(|_| {});
        assert_eq!(signal.connection_count(), 1);
        assert!(signal.is_connected());

        let _id2 = signal.connect(|_| {});
        assert_eq!(signal.connection_count(), 2);

        signal.disconnect(id1);
        assert_eq!(signal.connection_count(), 1);
    }

    #[test]
    fn mutable_handler() {
        let signal = Signal::new();
        let accumulated = Rc::new(RefCell::new(Vec::new()));
        let acc = Rc::clone(&accumulated);
        signal.connect_mut(move |val: &i32| {
            acc.borrow_mut().push(*val);
        });

        signal.emit(1);
        signal.emit(2);
        signal.emit(3);
        assert_eq!(*accumulated.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn signal_chaining_forward() {
        let source = Signal::new();
        let target = Signal::new();
        let received = Rc::new(Cell::new(0i32));
        let r = Rc::clone(&received);
        target.connect(move |val: &i32| {
            r.set(*val);
        });

        forward(&source, &target);
        source.emit(99);
        assert_eq!(received.get(), 99);
    }

    #[test]
    fn deferred_emission_on_reentrant_emit() {
        let signal: Signal<i32> = Signal::new();
        let values = Rc::new(RefCell::new(Vec::new()));

        // Handler that re-entrantly emits the same signal when it sees value 1.
        let sig_clone = signal.clone();
        let v = Rc::clone(&values);
        signal.connect(move |val: &i32| {
            v.borrow_mut().push(*val);
            if *val == 1 {
                // Re-entrant emit — should be deferred.
                sig_clone.emit(2);
            }
        });

        signal.emit(1);
        // Handler sees 1 first, then the deferred 2.
        assert_eq!(*values.borrow(), vec![1, 2]);
    }

    #[test]
    fn signal_group_bulk_disconnect() {
        let signal_a = Signal::<()>::new();
        let signal_b = Signal::<()>::new();
        let count = Rc::new(Cell::new(0u32));

        let mut group = SignalGroup::new();

        let c1 = Rc::clone(&count);
        let id1 = signal_a.connect(move |_| c1.set(c1.get() + 1));
        group.track(&signal_a, id1);

        let c2 = Rc::clone(&count);
        let id2 = signal_b.connect(move |_| c2.set(c2.get() + 1));
        group.track(&signal_b, id2);

        signal_a.emit(());
        signal_b.emit(());
        assert_eq!(count.get(), 2);

        group.disconnect_all();

        signal_a.emit(());
        signal_b.emit(());
        assert_eq!(count.get(), 2); // no further calls
    }

    #[test]
    fn signal_group_len() {
        let signal = Signal::<()>::new();
        let mut group = SignalGroup::new();
        assert!(group.is_empty());

        let id = signal.connect(|_| {});
        group.track(&signal, id);
        assert_eq!(group.len(), 1);
        assert!(!group.is_empty());
    }

    #[test]
    fn event_bus_register_emit_subscribe() {
        let bus = EventBus::new();
        bus.register_signal::<i32>("value_changed");

        let received = Rc::new(Cell::new(0i32));
        let r = Rc::clone(&received);
        bus.subscribe::<i32, _>("value_changed", move |val| {
            r.set(*val);
        })
        .unwrap();

        bus.emit("value_changed", 42).unwrap();
        assert_eq!(received.get(), 42);
    }

    #[test]
    fn event_bus_type_mismatch() {
        let bus = EventBus::new();
        bus.register_signal::<i32>("my_signal");

        // Try to emit a String on an i32 signal.
        let result = bus.emit("my_signal", "wrong type".to_string());
        assert!(matches!(result, Err(BusError::TypeMismatch { .. })));
    }

    #[test]
    fn event_bus_not_found() {
        let bus = EventBus::new();
        let result = bus.emit("nonexistent", 42i32);
        assert!(matches!(result, Err(BusError::NotFound(_))));
    }

    #[test]
    fn event_bus_subscribe_type_mismatch() {
        let bus = EventBus::new();
        bus.register_signal::<i32>("my_signal");

        let result = bus.subscribe::<String, _>("my_signal", |_| {});
        assert!(matches!(result, Err(BusError::TypeMismatch { .. })));
    }

    #[test]
    fn event_bus_has_signal() {
        let bus = EventBus::new();
        assert!(!bus.has_signal("test"));
        bus.register_signal::<()>("test");
        assert!(bus.has_signal("test"));
    }

    #[test]
    fn common_signal_types() {
        // Verify common signal types can be used with Signal<T>.
        let clicked_signal = Signal::<Clicked>::new();
        let toggled_signal = Signal::<Toggled>::new();
        let text_signal = Signal::<TextChanged>::new();
        let submit_signal = Signal::<Submitted>::new();
        let activated_signal = Signal::<Activated>::new();
        let selection_signal = Signal::<SelectionChanged>::new();
        let value_signal = Signal::<ValueChanged<f64>>::new();

        let click_count = Rc::new(Cell::new(0u32));
        let cc = Rc::clone(&click_count);
        clicked_signal.connect(move |_| cc.set(cc.get() + 1));
        clicked_signal.emit(Clicked);
        assert_eq!(click_count.get(), 1);

        let toggle_state = Rc::new(Cell::new(false));
        let ts = Rc::clone(&toggle_state);
        toggled_signal.connect(move |t| ts.set(t.0));
        toggled_signal.emit(Toggled(true));
        assert!(toggle_state.get());

        let text_val = Rc::new(RefCell::new(String::new()));
        let tv = Rc::clone(&text_val);
        text_signal.connect(move |t| *tv.borrow_mut() = t.text.clone());
        text_signal.emit(TextChanged {
            text: "hello".to_string(),
        });
        assert_eq!(*text_val.borrow(), "hello");

        let submit_val = Rc::new(RefCell::new(String::new()));
        let sv = Rc::clone(&submit_val);
        submit_signal.connect(move |s| *sv.borrow_mut() = s.0.clone());
        submit_signal.emit(Submitted("done".to_string()));
        assert_eq!(*submit_val.borrow(), "done");

        let active_idx = Rc::new(Cell::new(0usize));
        let ai = Rc::clone(&active_idx);
        activated_signal.connect(move |a| ai.set(a.0));
        activated_signal.emit(Activated(5));
        assert_eq!(active_idx.get(), 5);

        let selected = Rc::new(RefCell::new(Vec::new()));
        let sel = Rc::clone(&selected);
        selection_signal.connect(move |s| *sel.borrow_mut() = s.selected.clone());
        selection_signal.emit(SelectionChanged {
            selected: vec![1, 3, 5],
        });
        assert_eq!(*selected.borrow(), vec![1, 3, 5]);

        let float_val = Rc::new(Cell::new(0.0f64));
        let fv = Rc::clone(&float_val);
        value_signal.connect(move |v| fv.set(v.value));
        value_signal.emit(ValueChanged { value: 3.25 });
        assert!((float_val.get() - 3.25).abs() < f64::EPSILON);
    }
}
