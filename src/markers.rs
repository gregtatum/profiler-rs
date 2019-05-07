use std::time::Instant;

pub trait Marker {
    fn serialize(&self);
}

pub struct StaticStringMarker {
    created_at: Instant,
    string: &'static str,
}

impl StaticStringMarker {
    pub fn new(string: &'static str) -> StaticStringMarker {
        return StaticStringMarker {
            created_at: Instant::now(),
            string,
        };
    }
}

impl Marker for StaticStringMarker {
    fn serialize(&self) {
        // TODO
    }
}
