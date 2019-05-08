
use crate::buffer_thread::{BufferThread, BufferThreadMessage};
use crate::markers::Marker;
use std::cell::RefCell;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Lives on the main thread.
pub struct Core {
    buffer_join_handle: thread::JoinHandle<()>,
    buffer_thread_sender: mpsc::Sender<BufferThreadMessage>,
}

impl Core {
    pub fn new(entry_lifetime: Duration) -> Core {
        let (buffer_thread_sender, buffer_thread_receiver) = mpsc::channel();

        let buffer_join_handle =
            thread::Builder::new()
                .name("Profiler Buffer".into())
                .spawn(move || {
                    let mut buffer_thread =
                        BufferThread::new(buffer_thread_receiver, entry_lifetime);
                    buffer_thread.start();
                });

        Core {
            buffer_join_handle: buffer_join_handle.unwrap(),
            buffer_thread_sender,
        }
    }

    pub fn get_thread_registrar(&self) -> ThreadRegistrar {
        ThreadRegistrar {
            buffer_thread_sender: self.buffer_thread_sender.clone(),
        }
    }
}

pub struct ThreadRegistrar {
    pub buffer_thread_sender: mpsc::Sender<BufferThreadMessage>,
}

impl ThreadRegistrar {
    pub fn register(self) {
        // Consume `self` and put the data into TLS.
        let ThreadRegistrar {
            buffer_thread_sender,
        } = self;

        BUFFER_THREAD_SENDER.with(|maybe_sender| {
            *maybe_sender.borrow_mut() = Some(buffer_thread_sender);
        });
    }
}


thread_local! {
    static BUFFER_THREAD_SENDER: RefCell<
        Option<mpsc::Sender<BufferThreadMessage>>
    > = RefCell::new(None)
}

pub fn store_marker_sender(sender: mpsc::Sender<BufferThreadMessage>) {
    BUFFER_THREAD_SENDER.with(|maybe_sender| {
        *maybe_sender.borrow_mut() = Some(sender);
    });
}

pub fn add_marker(marker: Box<Marker + Send>) {
    BUFFER_THREAD_SENDER.with(|sender| match *sender.borrow() {
        Some(ref sender) => {
            sender
                .send(BufferThreadMessage::AddMarker(marker))
                .expect("Unable to send a marker to the buffer thread.");
        }
        None => {}
    });
}

#[cfg(test)]
mod tests {
    use super::super::markers::StaticStringMarker;
    use super::*;

    #[test]
    fn can_create_and_store_markers() {
        let profiler = Core::new(Duration::new(60, 0));

        let thread_registrar1 = profiler.get_thread_registrar();

        let thread_handle1 = thread::spawn(move || {
            thread_registrar1.register();
            add_marker(Box::new(StaticStringMarker::new("Thread 1, Marker 1")));
            add_marker(Box::new(StaticStringMarker::new("Thread 1, Marker 2")));
            add_marker(Box::new(StaticStringMarker::new("Thread 1, Marker 3")));
        });

        let thread_registrar2 = profiler.get_thread_registrar();

        let thread_handle2 = thread::spawn(move || {
            thread_registrar2.register();
            add_marker(Box::new(StaticStringMarker::new("Thread 2, Marker 1")));
            add_marker(Box::new(StaticStringMarker::new("Thread 2, Marker 2")));
            add_marker(Box::new(StaticStringMarker::new("Thread 2, Marker 3")));
        });

        thread_handle1
            .join()
            .expect("Joined the thread handle for the test.");

        thread_handle2
            .join()
            .expect("Joined the thread handle for the test.");
    }
}
