/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use crate::buffer_thread::BufferThreadMessage;
use crate::core::CoreThreadMessage;
use crate::sampler::{Sample, Sampler};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// These are all of the messages that can be sent to the SamplerThread.
pub enum SamplerThreadMessage {
    /// Each sampler is responsible for sampling a single thread. There are platform
    /// specific implementations.
    RegisterSampler(Box<dyn Sampler>),
    /// Unregister a sampler with a given thread id.
    UnregisterSampler(u32),
    PauseSampling,
    StartSampling,
    StopSampling,
    // This message is used primarily for testing, so we can know that one sample
    // has happened.
    WaitingForOneSample,
}

/**
 * The SamplerThread is responsible for orchestrating taking samples from different
 * threads. It handles the registration of threads, and the platform-dependent signal
 * calls to stop and stackwalk a thread.
 */
pub struct SamplerThread {
    receiver: mpsc::Receiver<SamplerThreadMessage>,
    to_buffer_thread: mpsc::Sender<BufferThreadMessage>,
    to_core: mpsc::Sender<CoreThreadMessage>,
    interval: Duration,
    samplers: Vec<Box<dyn Sampler>>,
}

impl SamplerThread {
    /// The MainThreadCore is responsible for initializing this SamplerThread, and can
    /// pass messages to it using the SamplerThreadMessage. In addition, the
    /// to_buffer_thread mpsc::Sender is passed in so that the SamplerThread can
    //  communicate to the buffer thread.
    pub fn new(
        receiver: mpsc::Receiver<SamplerThreadMessage>,
        to_buffer_thread: mpsc::Sender<BufferThreadMessage>,
        to_core: mpsc::Sender<CoreThreadMessage>,
        interval: Duration,
    ) -> SamplerThread {
        SamplerThread {
            receiver,
            to_buffer_thread,
            to_core,
            interval,
            samplers: Vec::new(),
        }
    }

    /// The MainThreadCore creates the SamplerThread, and then immediately starts it.
    pub fn start(&mut self) {
        // Start out paused.
        let mut is_paused = true;
        let mut is_waiting_for_one_sample = false;
        let mut should_sample;

        loop {
            let unhandled_message = if is_paused {
                // The profiler is paused. Rather than continue to wake up this thread,
                // just wait until we get a new message to handle.
                match self.receiver.recv() {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvError) => {
                        // The channel is closed, we don't need this thread anymore.
                        return;
                    }
                }
            } else {
                // We are actively sampling, don't block this thread to receive messages.
                // Only handle any messages received while we were sleeping, and continue
                // on to sampling.
                match self.receiver.try_recv() {
                    Ok(message) => Some(message),
                    Err(mpsc::TryRecvError::Empty) => {
                        // There are no more messages to process.
                        None
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // The channel is closed, we don't need this thread anymore.
                        return;
                    }
                }
            };

            // First handle any messages that were sent, and react to them.
            match unhandled_message {
                Some(message) => {
                    should_sample = false;
                    match message {
                        SamplerThreadMessage::RegisterSampler(sampler) => {
                            self.samplers.push(sampler)
                        }
                        SamplerThreadMessage::UnregisterSampler(thread_id) => {
                            // Remove the sampler from the list.
                            self.samplers = self
                                .samplers
                                .drain(..)
                                .filter(|sampler| sampler.thread_id() != thread_id)
                                .collect();
                        }
                        SamplerThreadMessage::StopSampling => {
                            return;
                        }
                        SamplerThreadMessage::PauseSampling => {
                            is_paused = true;
                            continue;
                        }
                        SamplerThreadMessage::StartSampling => {
                            is_paused = false;
                        }
                        SamplerThreadMessage::WaitingForOneSample => {
                            is_waiting_for_one_sample = true;
                        }
                    }
                }
                None => {
                    should_sample = true;
                }
            };

            // Only continue looping when all the messages are drained.
            if !should_sample {
                continue;
            }

            // Now go through all the samplers.
            for sampler in self.samplers.iter() {
                match sampler.suspend_and_sample_thread() {
                    Ok(native_stack) => {
                        self.to_buffer_thread
                            .send(BufferThreadMessage::AddSample(Sample {
                                native_stack,
                                thread_id: sampler.thread_id(),
                            }))
                            .expect("Unable to send a sample to the buffer thread.");
                    }
                    Err(_) => {}
                }
            }

            if is_waiting_for_one_sample {
                is_waiting_for_one_sample = false;
                self.to_core
                    .send(CoreThreadMessage::OneSampleTaken)
                    .expect("Unable to send a message to the core thread from the sampler.");
            }

            // Wait for the duration.
            thread::sleep(self.interval);
        }
    }
}
