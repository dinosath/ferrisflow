use std::cell::Cell;
use std::rc::Rc;

use dioxus::core::provide_root_context;
use dioxus::prelude::*;

/// The next number of the current virtual DOM (RFC 0075).
#[derive(Clone, Default)]
struct ElementIds(Rc<Cell<usize>>);

/// The next element id number of the current virtual DOM, for a hook to keep.
/// A server renders each request in a new virtual DOM and the browser builds
/// the same tree to hydrate it, so both hand out the same numbers, which a
/// process-wide counter does not.
pub(crate) fn next_element_id() -> usize {
    let ids = try_consume_context::<ElementIds>()
        .unwrap_or_else(|| provide_root_context(ElementIds::default()));
    let id = ids.0.get();
    ids.0.set(id + 1);
    id
}
