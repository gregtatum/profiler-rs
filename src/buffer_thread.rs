use super::markers::{Marker, MarkersSerializer};
use super::time_expiring_buffer::{TimeExpiringBuffer};
use serde_json;
use serde_json::json;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub enum BufferThreadMessage {
    AddMarker(Box<Marker + Send>),
    ClearExpiredMarkers,
    SerializeMarkers,
}

pub struct BufferThread {
    receiver: mpsc::Receiver<BufferThreadMessage>,
    serialization_sender: mpsc::Sender<serde_json::Value>,
    markers: TimeExpiringBuffer<Box<Marker + Send>>,
    // TODO - This is not correct, it should be the start time of each thread where the marker
    // is coming from.
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
            // TODO - This is not correct.
            thread_start: Instant::now(),
            markers: TimeExpiringBuffer::new(entry_lifetime),
        }
    }

    pub fn start(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(BufferThreadMessage::AddMarker(marker)) => {
                    self.markers.push_back(marker);
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
