use super::markers::Marker;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The duration buffer stores profiler data inside of a VecDeque. This works so that
/// we can always add on information to the buffer, but older data gets cycled out.
/// The buffer will only increase in size based on the max size of information it
/// has seen.
pub struct TimeExpiringBuffer {
    pub markers: VecDeque<Box<Marker>>,
    pub entry_lifetime: Duration,
}

impl TimeExpiringBuffer {
    pub fn new(entry_lifetime: Duration) -> TimeExpiringBuffer {
        TimeExpiringBuffer {
            markers: VecDeque::new(),
            entry_lifetime,
        }
    }

    pub fn add_marker(&mut self, marker: Box<Marker>) {
        self.markers.push_back(marker);
    }

    pub fn remove_expired_markers(&mut self) {
        let now = Instant::now();
        loop {
            match self.markers.front() {
                Some(marker) => {
                    if now.duration_since(marker.get_created_at()) > self.entry_lifetime {
                        self.markers.pop_front();
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::markers::StaticStringMarker;
    use super::*;
    use std::thread::sleep;

    #[test]
    fn expired_markers_are_removed() {
        let one_ms = Duration::new(0, 100000);
        let mut buffer = TimeExpiringBuffer::new(one_ms);
        buffer.add_marker(Box::new(StaticStringMarker::new("Marker 1")));
        assert_eq!(
            buffer.markers.len(),
            1,
            "All of the markers are in the buffer."
        );

        // Sleep, this is a bit risky and could have intermittents if things are slow.
        sleep(Duration::new(0, 120000));

        buffer.add_marker(Box::new(StaticStringMarker::new("Marker 2")));
        buffer.add_marker(Box::new(StaticStringMarker::new("Marker 3")));
        buffer.remove_expired_markers();

        assert_eq!(
            buffer.markers.len(),
            2,
            "Two of the expired markers were expunged."
        );
    }
}
