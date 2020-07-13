use crate::sampler::StringTable;
use crate::time_expiring_buffer::BufferEntry;
use crate::time_expiring_buffer::TimeExpiringBuffer;
use serde_json::{json, Value};
use std::time::Instant;

pub trait Marker {
    fn serialize(
        &self,
        thread_start: &Instant,
        created_at: &Instant,
        string_table: &mut StringTable,
    ) -> Value;
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
    fn serialize(
        &self,
        thread_start: &Instant,
        created_at: &Instant,
        string_table: &mut StringTable,
    ) -> Value {
        let time = created_at.duration_since(*thread_start);
        json!([
            string_table.get_index(self.string), // name
            time.as_millis() as u64,             // time
            0,                                   // category
        ])
    }
}

pub fn serialize_markers_in_buffer(
    buffer: &TimeExpiringBuffer<Box<dyn Marker + Send>>,
    thread_start: &Instant,
    string_table: &mut StringTable,
) -> Value {
    // https://github.com/firefox-devtools/profiler/blob/04d81d51ed394827bff9c22e540993abeff1db5e/src/types/gecko-profile.js#L24
    let data: Vec<Value> = buffer
        .iter()
        .map(|BufferEntry { created_at, value }| {
            value.serialize(thread_start, created_at, string_table)
        })
        .collect();

    json!({
        "schema": { "name": 0, "time": 1, "category": 2, "data": 3 },
        "data": data
    })
}
