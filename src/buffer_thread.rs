use super::markers::Marker;
use super::time_expiring_buffer::TimeExpiringBuffer;
use std::sync::mpsc;
use std::time::Duration;

pub enum BufferThreadMessage {
    AddMarker(Box<Marker + Send>),
    ClearExpiredMarkers,
}

pub struct BufferThread {
    receiver: mpsc::Receiver<BufferThreadMessage>,
    markers: TimeExpiringBuffer<Box<Marker + Send>>,
}

impl BufferThread {
    pub fn new(
        receiver: mpsc::Receiver<BufferThreadMessage>,
        entry_lifetime: Duration,
    ) -> BufferThread {
        BufferThread {
            receiver,
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
}
