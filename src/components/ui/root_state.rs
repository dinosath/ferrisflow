use dioxus::prelude::*;

/// State a compound component's root owns (RFC 0077): the app's value while
/// it sets one, otherwise the root's own, which starts at the default. Parts
/// read it through the root's context and change it through `set`.
pub(crate) struct Controllable<T: 'static> {
    controlled: Memo<Option<T>>,
    own: Signal<T>,
    on_change: CopyValue<Option<Callback<T>>>,
}

impl<T: 'static> Clone for Controllable<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Controllable<T> {}

impl<T: Clone + PartialEq + 'static> Controllable<T> {
    /// The current value, subscribing the reader to its changes. It reads the
    /// app's value and the root's own directly: a memo over the `controlled`
    /// memo would only turn dirty once `controlled` recomputed, so an event
    /// handler reading it right after the app changed its value got the old
    /// one.
    pub(crate) fn get(&self) -> T {
        self.controlled.cloned().unwrap_or_else(|| self.own.cloned())
    }

    /// Takes a change the user made: the root keeps it unless the app controls
    /// the value, and the app hears of it either way.
    pub(crate) fn set(&self, next: T) {
        if self.controlled.peek().is_none() {
            let mut own = self.own;
            own.set(next.clone());
        }
        if let Some(on_change) = *self.on_change.peek() {
            on_change.call(next);
        }
    }
}

/// `controlled` reads the app's value, `None` while the app leaves the state
/// to the root; `on_change` is the app's change callback, kept current across
/// renders.
pub(crate) fn use_controllable<T: Clone + PartialEq + 'static>(
    controlled: impl Fn() -> Option<T> + 'static, default: impl FnOnce() -> T,
    on_change: Option<Callback<T>>,
) -> Controllable<T> {
    let own = use_signal(default);
    let controlled = use_memo(controlled);
    let mut handler = use_hook(|| CopyValue::new(on_change));
    handler.set(on_change);
    Controllable { controlled, own, on_change: handler }
}

/// The context of the root a part belongs to. A part outside its root
/// panics here, naming both, instead of rendering without its state; Dioxus
/// logs the panic and renders nothing for the part.
pub(crate) fn use_root_context<T: Clone + 'static>(part: &str, root: &str) -> T {
    root_context_or_panic(try_use_context::<T>(), part, root)
}

fn root_context_or_panic<T>(context: Option<T>, part: &str, root: &str) -> T {
    context.unwrap_or_else(|| panic!("`{part}` must be inside a `{root}` (RFC 0077)"))
}
