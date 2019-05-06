use super::signal_safe_linked_list::SignalSafeLinkedList;
use std::cell::RefCell;
use std::time::Instant;

// Store some data on each thread that it can use in a non-locking manner. This information
// is periodically taken and slurped up by the main thread.
thread_local! {
    pub static PENDING_MARKERS: RefCell<SignalSafeLinkedList<Box<Marker>>> = RefCell::new(SignalSafeLinkedList::new());
}

pub trait Marker {
    fn serialize(&self);
    fn get_created_at(&self) -> Instant;
}

pub struct StaticStringMarker {
    created_at: Instant,
    string: &'static str,
}

impl StaticStringMarker {
    pub fn new(string: &'static str) -> StaticStringMarker {
        return StaticStringMarker {
            created_at: Instant::now(),
            string,
        };
    }
}

impl Marker for StaticStringMarker {
    fn serialize(&self) {
        // TODO
    }

    fn get_created_at(&self) -> Instant {
        return self.created_at;
    }
}

// Add a marker to the thread local storage. This will later be slurped up by the main thread.
// The markers are stored in a signal-safe linked list.
pub fn add_marker(marker: Box<Marker>) {
    PENDING_MARKERS.with(move |signal_safe_markers_refcell| {
        let mut signal_safe_marker = signal_safe_markers_refcell.borrow_mut();
        signal_safe_marker.mutate(move |markers| {
            match markers {
                Some(markers) => markers.push_back(marker),
                None => {
                    panic!(concat!(
                        "The markers SignalSafeLinkedList should never be locked for mutation while",
                        "adding a marker. This means there is probably some kind of correctness bug."
                    ));
                }
            };
        });
    });
}
