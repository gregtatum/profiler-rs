use crate::time_expiring_buffer::TimeExpiringBuffer;
use serde::{Serialize, Serializer};
use std::time::Instant;

const MAX_NATIVE_FRAMES: usize = 1024;

pub struct Sample {
    pub native_stack: NativeStack,
    pub thread_id: u64,
}

impl Sample {
    fn serialize(&self, thread_start: &Instant, created_at: &Instant) -> Value {
        // TODO
        json!({})
    }
}

pub trait Sampler: Send {
    fn suspend_and_sample_thread(&self) -> Result<NativeStack, ()>;
    fn thread_id(&self) -> u64;
}

// Several types in this file are currently not used in a Linux or Windows build.
#[allow(dead_code)]
pub type Address = *const u8;

/// The registers used for stack unwinding
#[allow(dead_code)]
pub struct Registers {
    /// Instruction pointer.
    pub instruction_ptr: Address,
    /// Stack pointer.
    pub stack_ptr: Address,
    /// Frame pointer.
    pub frame_ptr: Address,
}

pub struct NativeStack {
    instruction_ptrs: [u8; MAX_NATIVE_FRAMES],
    stack_ptrs: [u8; MAX_NATIVE_FRAMES],
    count: usize,
}

impl NativeStack {
    pub fn new() -> Self {
        NativeStack {
            instruction_ptrs: [0; MAX_NATIVE_FRAMES],
            stack_ptrs: [0; MAX_NATIVE_FRAMES],
            count: 0,
        }
    }

    pub fn process_register(
        &mut self,
        instruction_ptr: *mut std::ffi::c_void,
        stack_ptr: *mut std::ffi::c_void,
    ) -> Result<(), ()> {
        if !(self.count < MAX_NATIVE_FRAMES) {
            return Err(());
        }
        self.instruction_ptrs[self.count] = instruction_ptr as u8;
        self.stack_ptrs[self.count] = stack_ptr as u8;
        self.count = self.count + 1;
        Ok(())
    }
}

// This struct handles JSON serialization through an iterator interface.
pub struct SamplesSerializer<'a> {
    thread_start: &'a Instant,
    buffer: &'a TimeExpiringBuffer<Sample>,
}

impl<'a> SamplesSerializer<'a> {
    pub fn new(
        thread_start: &'a Instant,
        buffer: &'a TimeExpiringBuffer<Sample>,
    ) -> SamplesSerializer<'a> {
        SamplesSerializer {
            thread_start,
            buffer,
        }
    }
}

impl<'a> Serialize for SamplesSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(self.buffer.iter().map(
            |Sample {
                 native_stack,
                 thread_id,
             }| value.serialize(self.thread_start, created_at),
        ))
    }
}
