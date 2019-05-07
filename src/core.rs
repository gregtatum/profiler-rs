use crate::buffer_thread::{BufferThread, BufferThreadMessage};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Lives on the main thread.
pub struct Core {
    sampler_join_handle: thread::JoinHandle<()>,
    sampler_thread_sender: mpsc::Sender<BufferThreadMessage>,
}

impl Core {
    pub fn new(entry_lifetime: Duration) -> Core {
        let (sampler_thread_sender, sampler_thread_receiver) = mpsc::channel();

        let sampler_join_handle = thread::Builder::new()
            .name("Profiler Sampler".into())
            .spawn(move || {
                let mut sampler_thread = BufferThread::new(sampler_thread_receiver, entry_lifetime);
                sampler_thread.start();
            });

        Core {
            sampler_join_handle: sampler_join_handle.unwrap(),
            sampler_thread_sender,
        }
    }
}
