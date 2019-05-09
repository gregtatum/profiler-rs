use super::markers::Marker;
use super::time_expiring_buffer::{BufferEntry, TimeExpiringBuffer};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub enum BufferThreadMessage {
    AddMarker(Box<Marker + Send>),
    ClearExpiredMarkers,
}

pub struct BufferThread {
    receiver: mpsc::Receiver<BufferThreadMessage>,
    markers: TimeExpiringBuffer<Box<Marker + Send>>,
    // TODO - This is not correct, it should be the start time of each thread where the marker
    // is coming from.
    thread_start: Instant,
}

impl BufferThread {
    pub fn new(
        receiver: mpsc::Receiver<BufferThreadMessage>,
        entry_lifetime: Duration,
    ) -> BufferThread {
        BufferThread {
            receiver,
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
                Err(_) => {
                    break;
                }
            }
        }
    }

    fn serialize_markers(&self) {
        for BufferEntry { created_at, value } in self.markers.iter() {
            value.serialize(&self.thread_start, created_at);
        }
    }
}
