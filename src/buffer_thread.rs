use super::core::{SerializationMessage, ThreadInfo};
use super::markers::{serialize_markers_in_buffer, Marker};
use super::time_expiring_buffer::TimeExpiringBuffer;
use crate::sampler::{Sample, SamplesSerializer, StringTable};
use serde_json;
use serde_json::json;
use std::process;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct CoreInfoForSerialization {
    pub start_time: Instant,
    pub sampling_interval: u64,
    pub thread_infos: Vec<ThreadInfo>,
}

/// This enum collects all of the different types of messages that we can send to the buffer
/// thread. The buffer thread is the end point of a multiple producer, single consumer (mpsc)
/// channel, and this enum is how we communicate with it.
pub enum BufferThreadMessage {
    AddMarker(u32, Box<dyn Marker + Send>),
    AddSample(u32, Sample),
    ClearExpiredMarkers,
    SerializeBuffer(CoreInfoForSerialization),
    GetSampleCount,
    // This message is used primarily for testing, so we can know that one sample
    // has happened.
    WaitingForOneSample,
}

/// The BufferThread represents a thread for handling messages that need to be stored in the
/// profiler buffer. It contains storage for
pub struct BufferThread {
    receiver: mpsc::Receiver<BufferThreadMessage>,
    serialization_sender: mpsc::Sender<SerializationMessage>,
    markers: TimeExpiringBuffer<Box<dyn Marker + Send>>,
    samples: TimeExpiringBuffer<Sample>,
}

impl BufferThread {
    pub fn new(
        receiver: mpsc::Receiver<BufferThreadMessage>,
        entry_lifetime: Duration,
        serialization_sender: mpsc::Sender<SerializationMessage>,
    ) -> BufferThread {
        BufferThread {
            receiver,
            serialization_sender,
            markers: TimeExpiringBuffer::new(entry_lifetime),
            samples: TimeExpiringBuffer::new(entry_lifetime),
        }
    }

    /// This method runs the loop to handle messages.
    pub fn start(&mut self) {
        let mut is_waiting_for_one_sample = false;
        loop {
            match self.receiver.recv() {
                Ok(BufferThreadMessage::AddMarker(tid, marker)) => {
                    self.markers.push_back(tid, marker);
                }
                Ok(BufferThreadMessage::AddSample(tid, sample)) => {
                    self.samples.push_back(tid, sample);
                    if is_waiting_for_one_sample {
                        is_waiting_for_one_sample = false;
                        self.serialization_sender
                            .send(SerializationMessage::OneSampleReceived)
                            .unwrap();
                    }
                }
                Ok(BufferThreadMessage::ClearExpiredMarkers) => {
                    self.markers.remove_expired();
                }
                Ok(BufferThreadMessage::SerializeBuffer(core_info)) => {
                    self.serialization_sender
                        .send(SerializationMessage::Serialize(
                            self.serialize_buffer(&core_info),
                        ))
                        .unwrap();
                }
                Ok(BufferThreadMessage::GetSampleCount) => {
                    self.serialization_sender
                        .send(SerializationMessage::SampleCount(self.samples.len()))
                        .unwrap();
                }
                Ok(BufferThreadMessage::WaitingForOneSample) => {
                    is_waiting_for_one_sample = true;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    fn serialize_buffer(&self, core_info: &CoreInfoForSerialization) -> serde_json::Value {
        let pid = process::id();
        json!({
            // https://github.com/firefox-devtools/profiler/blob/04d81d51ed394827bff9c22e540993abeff1db5e/src/types/gecko-profile.js#L244
            "meta": {
                // https://github.com/firefox-devtools/profiler/blob/04d81d51ed394827bff9c22e540993abeff1db5e/src/app-logic/constants.js#L8
                "version": 19,
                "startTime": 0,
                "shutdownTime": serde_json::Value::Null,
                // https://github.com/firefox-devtools/profiler/blob/04d81d51ed394827bff9c22e540993abeff1db5e/src/profile-logic/data-structures.js
                "categories": [
                    { "name": "Other", "color": "grey", "subcategories": ["Other"] },
                    { "name": "Idle", "color": "transparent", "subcategories": ["Other"] },
                    { "name": "Layout", "color": "purple", "subcategories": ["Other"] },
                    { "name": "JavaScript", "color": "yellow", "subcategories": ["Other"] },
                    { "name": "GC / CC", "color": "orange", "subcategories": ["Other"] },
                    { "name": "Network", "color": "lightblue", "subcategories": ["Other"] },
                    { "name": "Graphics", "color": "green", "subcategories": ["Other"] },
                    { "name": "DOM", "color": "blue", "subcategories": ["Other"] },
                ],
                "interval": core_info.sampling_interval,
                "product": "Rust Profiler",
            },
            "libs": [],
            "pausedRanges": [],
            "processes": [],
            // TODO - Actually output threads.
            // https://github.com/firefox-devtools/profiler/blob/04d81d51ed394827bff9c22e540993abeff1db5e/src/types/gecko-profile.js#L170
            "threads": core_info.thread_infos.iter()
                .map(|thread_info| {
                    let samples_serializer = SamplesSerializer::new(
                        &core_info.start_time,
                        &self.samples,
                        thread_info.id
                    );
                    let mut string_table = StringTable::new();

                    json!({
                        // TODO - Fill out these properties.
                        "registerTime": 0,
                        "unregisterTime": serde_json::Value::Null,
                        // These should be complete:
                        "name": thread_info.name,
                        "processType": "default",
                        "tid": thread_info.id,
                        "pid": pid,
                        "markers": serialize_markers_in_buffer(
                            &self.markers, &core_info.start_time, &mut string_table, thread_info.id),
                        "samples": samples_serializer.serialize_samples(thread_info.id),
                        "stackTable": samples_serializer.serialize_stack_table(),
                        "frameTable": samples_serializer.serialize_frame_table(&mut string_table),
                        "stringTable": string_table.serialize(),
                    })
                })
                .collect::<serde_json::Value>()
        })
    }
}
