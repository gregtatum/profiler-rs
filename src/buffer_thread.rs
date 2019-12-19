use super::markers::{Marker, MarkersSerializer};
use super::time_expiring_buffer::TimeExpiringBuffer;
use crate::sampler::Sample;
use serde_json;
use serde_json::json;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// This enum collects all of the different types of messages that we can send to the buffer
/// thread. The buffer thread is the end point of a multiple producer, single consumer (mpsc)
/// channel, and this enum is how we communicate with it.
pub enum BufferThreadMessage {
    AddMarker(Box<dyn Marker + Send>),
    AddSample(Sample),
    ClearExpiredMarkers,
    SerializeMarkers,
}

/// The BufferThread represents a thread for handling messages that need to be stored in the
/// profiler buffer. It contains storage for
pub struct BufferThread {
    receiver: mpsc::Receiver<BufferThreadMessage>,
    serialization_sender: mpsc::Sender<serde_json::Value>,
    markers: TimeExpiringBuffer<Box<dyn Marker + Send>>,
    samples: TimeExpiringBuffer<Sample>,
    thread_start: Instant,
}

impl BufferThread {
    pub fn new(
        receiver: mpsc::Receiver<BufferThreadMessage>,
        entry_lifetime: Duration,
        serialization_sender: mpsc::Sender<serde_json::Value>,
    ) -> BufferThread {
        BufferThread {
            receiver,
            serialization_sender,
            thread_start: Instant::now(),
            markers: TimeExpiringBuffer::new(entry_lifetime),
            samples: TimeExpiringBuffer::new(entry_lifetime),
        }
    }

    /// This method runs the loop to handle messages.
    pub fn start(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(BufferThreadMessage::AddMarker(marker)) => {
                    self.markers.push_back(marker);
                }
                Ok(BufferThreadMessage::AddSample(sample)) => {
                    self.samples.push_back(sample);
                }
                Ok(BufferThreadMessage::ClearExpiredMarkers) => {
                    self.markers.remove_expired();
                }
                Ok(BufferThreadMessage::SerializeMarkers) => {
                    self.serialization_sender
                        .send(self.serialize_markers())
                        .unwrap();
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    fn serialize_markers(&self) -> serde_json::Value {
        json!({
            "markers": MarkersSerializer::new(&self.thread_start, &self.markers)
        })
    }
}
