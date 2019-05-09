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
            "type": "Text Doe",
            "name": self.string,
            "startTime": time,
            "endTime": time,
        })
    }
}
