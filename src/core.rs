use crate::buffer_thread::{BufferThread, BufferThreadMessage};
use crate::markers::Marker;
use crate::sampler_mac::MacOsSampler;
use crate::sampler_thread::{SamplerThread, SamplerThreadMessage};
use serde_json;
use std::cell::RefCell;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

/// Send a message from the buffer thread to the main thread core.
pub enum SerializationMessage {
    // Respond with the serialized profile.
    Serialize(serde_json::Value),
    // See how many samples are in the profile buffers. This is useful for testing.
    SampleCount(usize),
}

/// The MainThreadCore is the orchestrator that coordinates the registration between multiple
/// threads.
pub struct MainThreadCore {
    start_time: Instant,
    buffer_join_handle: thread::JoinHandle<()>,
    sampler_join_handle: thread::JoinHandle<()>,
    buffer_thread_sender: mpsc::Sender<BufferThreadMessage>,
    serialization_receiver: mpsc::Receiver<SerializationMessage>,
    sampler_thread_sender: mpsc::Sender<SamplerThreadMessage>,
}

impl MainThreadCore {
    pub fn new(entry_lifetime: Duration, sampling_interval: Duration) -> MainThreadCore {
        let (buffer_thread_sender, buffer_thread_receiver) = mpsc::channel();
        let (sampler_thread_sender, sampler_thread_receiver) = mpsc::channel();
        let (serialization_sender, serialization_receiver) = mpsc::channel();

        let buffer_join_handle =
            thread::Builder::new()
                .name("Profiler Buffer".into())
                .spawn(move || {
                    let mut buffer_thread = BufferThread::new(
                        buffer_thread_receiver,
                        entry_lifetime,
                        serialization_sender,
                    );
                    buffer_thread.start();
                });

        let sampler_join_handle = {
            let buffer_thread_sender2 = buffer_thread_sender.clone();
            thread::Builder::new()
                .name("Profiler Sampler".into())
                .spawn(move || {
                    let mut sampler_thread = SamplerThread::new(
                        sampler_thread_receiver,
                        buffer_thread_sender2,
                        sampling_interval,
                    );
                    sampler_thread.start();
                })
        };

        MainThreadCore {
            start_time: Instant::now(),
            buffer_join_handle: buffer_join_handle.unwrap(),
            sampler_join_handle: sampler_join_handle.unwrap(),
            buffer_thread_sender,
            sampler_thread_sender,
            serialization_receiver,
        }
    }

    pub fn get_thread_registrar(&self) -> ThreadRegistrar {
        ThreadRegistrar {
            buffer_thread_sender: Some(self.buffer_thread_sender.clone()),
            sampler_thread_sender: Some(self.sampler_thread_sender.clone()),
        }
    }

    pub fn get_serialization_response(&self) -> SerializationMessage {
        self.serialization_receiver
            .recv()
            .expect("Unable to receive the message from the serialization receiver.")
    }

    pub fn get_sample_count(&self) -> usize {
        self.buffer_thread_sender
            .send(BufferThreadMessage::GetSampleCount)
            .expect("Unable to send a message to the buffer thread to serialize markers");

        match self.get_serialization_response() {
            SerializationMessage::SampleCount(size) => size,
            _ => panic!("Expected to receive a message Serialize message."),
        }
    }

    pub fn serialize(&self) -> serde_json::Value {
        self.buffer_thread_sender
            .send(BufferThreadMessage::SerializeBuffer(self.start_time))
            .expect("Unable to send a message to the buffer thread to serialize markers");

        match self.get_serialization_response() {
            SerializationMessage::Serialize(profile) => profile,
            _ => panic!("Expected to receive a message Serialize message."),
        }
    }

    pub fn start_sampling(&self) {
        self.sampler_thread_sender
            .send(SamplerThreadMessage::StartSampling)
            .expect("Unable to start the profiler");
    }
}

/// The ThreadRegistrar is responsible for the registration of a thread. It is created via
/// the MainThreadCore on the main thread, and then passed into individual threads.
///
/// ```
///    use profiler::core::MainThreadCore;
///    use std::time::Duration;
///    use std::thread;
///
///    let profiler_core = MainThreadCore::new(Duration::new(60, 0), Duration::from_millis(10));
///    let mut thread_registrar = profiler_core.get_thread_registrar();
///
///    let thread_handle = thread::spawn(move || {
///        thread_registrar.register();
///        // At the end of the scope, the thread is automatically unregistered.
///    });
/// ```
pub struct ThreadRegistrar {
    pub buffer_thread_sender: Option<mpsc::Sender<BufferThreadMessage>>,
    pub sampler_thread_sender: Option<mpsc::Sender<SamplerThreadMessage>>,
}

impl ThreadRegistrar {
    pub fn register(&mut self) {
        let ThreadRegistrar {
            buffer_thread_sender,
            sampler_thread_sender,
        } = self;

        let err = "The ThreadRegistrar was already used.";
        let buffer_thread_sender = buffer_thread_sender.take().expect(err);
        let sampler_thread_sender = sampler_thread_sender.take().expect(err);

        sampler_thread_sender
            .send(SamplerThreadMessage::RegisterSampler(MacOsSampler::new()))
            .expect("Expected to send a message to the sampler thread.");

        BUFFER_THREAD_SENDER.with(|maybe_sender| {
            *maybe_sender.borrow_mut() = Some(buffer_thread_sender);
        });

        SAMPLER_THREAD_SENDER.with(|maybe_sender| {
            *maybe_sender.borrow_mut() = Some(sampler_thread_sender);
        });
    }

    fn unregister_thread(&mut self) {
        // Grab the sampler thread from thread local storage.
        SAMPLER_THREAD_SENDER.with(|maybe_sender| {
            {
                // Send a message to the sampler thread to remove it.
                let sender = &*maybe_sender.borrow();

                sender
                    .as_ref()
                    .expect("The SAMPLER_THREAD_SENDER did not exist.")
                    .send(SamplerThreadMessage::UnregisterSampler(
                        MacOsSampler::request_thread_id(),
                    ))
                    .expect("Expected to send a message to the sampler thread.");
            }
            // Finally set the sender to None, which is a noop if it's already None.
            *maybe_sender.borrow_mut() = None;
        });
    }
}

impl Drop for ThreadRegistrar {
    fn drop(&mut self) {
        self.unregister_thread();
    }
}

// Each thread local value here is used on the instrumented thread to coordinate with
// recording values into the buffer.
thread_local! {
    static BUFFER_THREAD_SENDER: RefCell<
        Option<mpsc::Sender<BufferThreadMessage>>
    > = RefCell::new(None);

    static SAMPLER_THREAD_SENDER: RefCell<
        Option<mpsc::Sender<SamplerThreadMessage>>
    > = RefCell::new(None);
}

pub fn add_marker(marker: Box<dyn Marker + Send>) {
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
    use serde_json::{json, Value};

    // Markers have real time numbers in them. This doesn't work for asserting data structures
    // in tests. Strip them out.
    fn set_time_values_to_zero(values: &mut Vec<Value>) {
        for value in values.iter_mut() {
            for value in value.as_object_mut().unwrap().values_mut() {
                if value.is_number() {
                    *value = json!(0);
                }
            }
        }
    }

    #[test]
    fn can_create_and_store_markers() {
        let profiler_core = MainThreadCore::new(Duration::new(60, 0), Duration::from_millis(10));
        let mut thread_registrar1 = profiler_core.get_thread_registrar();

        let thread_handle1 = thread::spawn(move || {
            thread_registrar1.register();
            thread::sleep(Duration::from_millis(100));
            add_marker(Box::new(StaticStringMarker::new("Thread 1, Marker 1")));
            add_marker(Box::new(StaticStringMarker::new("Thread 1, Marker 2")));
            add_marker(Box::new(StaticStringMarker::new("Thread 1, Marker 3")));
        });

        let mut thread_registrar2 = profiler_core.get_thread_registrar();

        let thread_handle2 = thread::spawn(move || {
            thread_registrar2.register();
            thread::sleep(Duration::from_millis(200));
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

        let mut markers: Vec<Value> = profiler_core
            .serialize()
            .get_mut("markers")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .to_owned();

        set_time_values_to_zero(&mut markers);
        let time_value = 0;

        assert_eq!(
            json!(markers),
            serde_json::json!([
                {"endTime": time_value, "name": "Thread 1, Marker 1", "startTime": time_value, "type": "Text"},
                {"endTime": time_value, "name": "Thread 1, Marker 2", "startTime": time_value, "type": "Text"},
                {"endTime": time_value, "name": "Thread 1, Marker 3", "startTime": time_value, "type": "Text"},
                {"endTime": time_value, "name": "Thread 2, Marker 1", "startTime": time_value, "type": "Text"},
                {"endTime": time_value, "name": "Thread 2, Marker 2", "startTime": time_value, "type": "Text"},
                {"endTime": time_value, "name": "Thread 2, Marker 3", "startTime": time_value, "type": "Text"}
            ]),
        );
    }

    #[test]
    fn can_register_samples() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        // Initialize the profiler core.
        let profiler_core = MainThreadCore::new(Duration::new(60, 0), Duration::from_millis(10));

        // Create a shared reference to signal to the threads to shut down.
        let do_shutdown_threads = Arc::new(AtomicBool::new(false));

        let thread_handle = {
            let mut thread_registrar = profiler_core.get_thread_registrar();
            let do_shutdown_threads = do_shutdown_threads.clone();

            thread::spawn(move || {
                thread_registrar.register();
                // Just spin in place seeing if it's time to exit.
                loop {
                    if do_shutdown_threads.load(Ordering::Relaxed) {
                        break;
                    }
                }
            })
        };

        profiler_core.start_sampling();

        loop {
            if profiler_core.get_sample_count() > 0 {
                break;
            }
        }

        // Signal to the threads that it's time to shut down.
        do_shutdown_threads.store(true, Ordering::Relaxed);

        thread_handle
            .join()
            .expect("Joined the thread handle for the test.");

        // TODO - Write a better assertion for this.

        // println!("Serialization: {:#?}", profiler_core.serialize());
        // assert_eq!(
        //     profiler_core.serialize(),
        //     json!({
        //         "frameTable": {
        //             "address":[],
        //             "category":[],
        //             "column":[],
        //             "func":[],
        //             "implementation":[],
        //             "innerWindowID":[],
        //             "length":0,"line":[],
        //             "optimizations":[],
        //             "subcategory":[]
        //         },
        //         "markers":[],
        //         "samples":{
        //             "stack":[],
        //             "time":[],
        //             "length":0,
        //         },
        //         "stackTable": {
        //             "category":[],
        //             "frame":[],
        //             "prefix":[],
        //             "subcategory":[],
        //             "length":0,
        //         }
        //     })
        // )
    }
}
