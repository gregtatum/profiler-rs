use crate::time_expiring_buffer::BufferEntry;
use crate::time_expiring_buffer::TimeExpiringBuffer;
use serde::{Serialize, Serializer};
use serde_json::{json, Value};
use std::time::Instant;

pub trait Marker {
    fn serialize(&self, thread_start: &Instant, created_at: &Instant) -> Value;
}

pub struct StaticStringMarker {
    string: &'static str,
}

impl StaticStringMarker {
    pub fn new(string: &'static str) -> StaticStringMarker {
        StaticStringMarker { string }
    }
}

impl Marker for StaticStringMarker {
    // Target this payload:
    // https://github.com/firefox-devtools/profiler/blob/5bbf1879dcbf7b1a88f44619e426a92e00c81958/src/types/markers.js#L392
    fn serialize(&self, thread_start: &Instant, created_at: &Instant) -> Value {
        let time = created_at.duration_since(*thread_start);
        json!({
            "type": "Text",
            "name": self.string,
            "startTime": time.as_millis() as u64,
            "endTime": time.as_millis() as u64,
        })
    }
}

// This struct handles JSON serialization through an iterator interface.
pub struct MarkersSerializer<'a> {
    thread_start: &'a Instant,
    buffer: &'a TimeExpiringBuffer<Box<dyn Marker + Send>>,
}

impl<'a> MarkersSerializer<'a> {
    pub fn new(
        thread_start: &'a Instant,
        buffer: &'a TimeExpiringBuffer<Box<dyn Marker + Send>>,
    ) -> MarkersSerializer<'a> {
        MarkersSerializer {
            thread_start,
            buffer,
        }
    }
}

impl<'a> Serialize for MarkersSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(self.buffer.iter().map(|BufferEntry { created_at, value }| {
            value.serialize(self.thread_start, created_at)
        }))
    }
}
