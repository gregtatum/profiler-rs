use std::collections::LinkedList;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// The intention with this linked list is to provide a mechanism to safely be able to
/// access a LinkedList that can be interrupted at any time with a signal. We'll see if
/// if it compiles in Rust or not.
pub struct SignalSafeLinkedList<T> {
    is_mutating: AtomicBool,
    linked_list: LinkedList<T>,
}

impl<T> SignalSafeLinkedList<T> {
    pub fn new() -> SignalSafeLinkedList<T> {
        SignalSafeLinkedList {
            is_mutating: AtomicBool::new(false),
            linked_list: LinkedList::new(),
        }
    }

    pub fn get(&self) -> Option<&LinkedList<T>> {
        if self.is_mutating.load(Ordering::SeqCst) {
            None
        } else {
            Some(&self.linked_list)
        }
    }

    pub fn mutate<F>(&mut self, callback: F)
    where
        F: FnOnce(Option<&mut LinkedList<T>>),
    {
        if self.is_mutating.load(Ordering::SeqCst) {
            callback(None);
        } else {
            self.is_mutating.store(true, Ordering::SeqCst);
            callback(Some(&mut self.linked_list));
            self.is_mutating.store(false, Ordering::SeqCst);
        };
    }
}
