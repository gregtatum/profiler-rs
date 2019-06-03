use crate::buffer_thread::BufferThreadMessage;
use crate::sampler::{Sample, Sampler};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub enum SamplerThreadMessage {
    RegisterThread(Box<Sampler>),
    PauseSampling,
    StartSampling,
    StopSampling,
}

pub struct SamplerThread {
    receiver: mpsc::Receiver<SamplerThreadMessage>,
    to_buffer_thread: mpsc::Sender<BufferThreadMessage>,
    interval: Duration,
    samplers: Vec<Box<Sampler>>,
}

impl SamplerThread {
    pub fn new(
        receiver: mpsc::Receiver<SamplerThreadMessage>,
        to_buffer_thread: mpsc::Sender<BufferThreadMessage>,
        interval: Duration,
    ) -> SamplerThread {
        SamplerThread {
            receiver,
            to_buffer_thread,
            interval,
            samplers: Vec::new(),
        }
    }

    pub fn start(&mut self) {
        // Start out paused.
        let mut is_paused = true;
        let mut should_sample = false;

        loop {
            let unhandled_message = if is_paused {
                match self.receiver.recv() {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvError) => {
                        // The channel is closed, we don't need this thread anymore.
                        return;
                    }
                }
            } else {
                // We are actively sampling.
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
                        SamplerThreadMessage::RegisterThread(sampler) => {
                            self.samplers.push(sampler)
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

            // Wait for the duration.
            thread::sleep(self.interval);
        }
    }
}
