use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct BufferEntry<T> {
    pub created_at: Instant,
    pub value: T,
}

/// The duration buffer stores profiler data inside of a VecDeque. This works so that
/// we can always add on information to the buffer, but older data gets cycled out.
/// The buffer will only increase in size based on the max size of information it
/// has seen.
pub struct TimeExpiringBuffer<T> {
    pub buffer: VecDeque<BufferEntry<T>>,
    pub entry_lifetime: Duration,
}

impl<T> TimeExpiringBuffer<T> {
    pub fn new(entry_lifetime: Duration) -> TimeExpiringBuffer<T> {
        TimeExpiringBuffer {
            buffer: VecDeque::new(),
            entry_lifetime,
        }
    }

    pub fn push_back(&mut self, value: T) {
        self.buffer.push_back(BufferEntry {
            created_at: Instant::now(),
            value,
        });
    }

    pub fn remove_expired(&mut self) {
        let now = Instant::now();
        loop {
            match self.buffer.front() {
                Some(item) => {
                    if now.duration_since(item.created_at) > self.entry_lifetime {
                        self.buffer.pop_front();
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<BufferEntry<T>> {
        self.buffer.iter()
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
        let mut time_expiring_buffer = TimeExpiringBuffer::new(one_ms);
        time_expiring_buffer.push_back(Box::new(StaticStringMarker::new("Marker 1")));
        assert_eq!(
            time_expiring_buffer.buffer.len(),
            1,
            "All of the markers are in the buffer."
        );

        // Sleep, this is a bit risky and could have intermittents if things are slow.
        sleep(Duration::new(0, 120000));

        time_expiring_buffer.push_back(Box::new(StaticStringMarker::new("Marker 2")));
        time_expiring_buffer.push_back(Box::new(StaticStringMarker::new("Marker 3")));
        time_expiring_buffer.remove_expired();

        assert_eq!(
            time_expiring_buffer.buffer.len(),
            2,
            "Two of the expired markers were expunged."
        );
    }
}
