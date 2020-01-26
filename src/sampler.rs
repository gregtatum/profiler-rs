use crate::time_expiring_buffer::BufferEntry;
use crate::time_expiring_buffer::TimeExpiringBuffer;
use serde::{Serialize, Serializer};
use serde_json::{json, Value};
use std::time::Instant;

const MAX_NATIVE_FRAMES: usize = 1024;

/// These are the various failure cases for suspending and sampling a thread.
pub enum SuspendAndSampleError {
    CouldNotSuspendThread,
    CouldNotGetRegisters,
    CouldNotResumeThread,
}

pub struct Sample {
    pub native_stack: NativeStack,
    pub thread_id: u32,
}

impl Sample {
    pub fn serialize(&self, profiler_start: &Instant, created_at: &Instant) -> Value {
        let time = created_at.duration_since(*profiler_start);
        json!({})
    }
}

pub trait Sampler: Send {
    fn suspend_and_sample_thread(&self) -> Result<NativeStack, SuspendAndSampleError>;
    fn thread_id(&self) -> u32;
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

pub struct StackForSerialization {
    // -1 if not present.
    prefix: Option<usize>,
    instruction_ptr: u8,
}

type StackTable = Vec<StackForSerialization>;

// This struct handles JSON serialization through an iterator interface.
pub struct SamplesSerializer<'a> {
    profiler_start: &'a Instant,
    buffer: &'a TimeExpiringBuffer<Sample>,
    stack_table: StackTable,
    buffer_entry_to_stack_table: Vec<usize>,
}

impl<'a> SamplesSerializer<'a> {
    pub fn new(
        profiler_start: &'a Instant,
        buffer: &'a TimeExpiringBuffer<Sample>,
    ) -> SamplesSerializer<'a> {
        let mut buffer_entry_to_stack_table = Vec::new();
        let mut stack_table: Vec<StackForSerialization> = vec![];

        // Build the stack table from the buffer entries.
        let mut stack_index = None;
        for BufferEntry { value, created_at } in buffer.iter() {
            let Sample {
                native_stack,
                thread_id,
            } = value;

            // Convert the native stack in the buffer to a StackTable. The StackTable
            // deduplicates the information already in the buffer. The StackTable is
            // well ordered, in that a leaf stack will always be after a root stack.
            for native_stack_instruction_ptr in native_stack.instruction_ptrs.iter() {
                if *native_stack_instruction_ptr == 0 {
                    // A 0 in the native stack signals a nullptr, which means that there
                    // are no more stacks to convert to a stack table.
                    break;
                }

                // Attempt to find the next stack.
                loop {
                    let stack: Option<&StackForSerialization> = match stack_index {
                        Some(stack_index) => stack_table.get(stack_index),
                        None => None,
                    };

                    match stack {
                        Some(stack) => {
                            if *native_stack_instruction_ptr == stack.instruction_ptr {
                                // We've found the correct stack.
                                break;
                            }
                            // Continue checking the next stack in the stack table.
                            stack_index = Some(stack_index.unwrap() + 1);
                        }
                        None => {
                            // This is the end of the stack table, add an entry here.
                            stack_table.push(StackForSerialization {
                                prefix: stack_index,
                                instruction_ptr: *native_stack_instruction_ptr,
                            });
                            // Update the stack index to the inserted entry.
                            stack_index = Some(stack_table.len() - 1);
                            break;
                        }
                    }
                }
                // Continue with the next native stack frame.
            }
            // Finalize this entry before handling the next.
            buffer_entry_to_stack_table.push(stack_index.unwrap());
            // Reset the stack index to 0 for the next entry.
            stack_index = Some(0);
        }

        SamplesSerializer {
            profiler_start,
            buffer,
            buffer_entry_to_stack_table,
            stack_table,
        }
    }
}

impl<'a> Serialize for SamplesSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(self.buffer.iter().map(|BufferEntry { created_at, value }| {
            value.serialize(&self.profiler_start, created_at)
        }))
    }
}
