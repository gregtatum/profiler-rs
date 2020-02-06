use crate::time_expiring_buffer::BufferEntry;
use crate::time_expiring_buffer::TimeExpiringBuffer;
use serde::ser::{Serialize, Serializer};
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

pub trait Sampler: Send {
    fn suspend_and_sample_thread(&self) -> Result<NativeStack, SuspendAndSampleError>;
    fn thread_id(&self) -> u32;
}

/// This type alias represents a memory offset stored in the register. It is a u8, since it only
/// represents an offset into the stack address space. When stackwalking, this gets converted to
/// a full memory address, which for now we're storing as a u64 regardless
/// of the underlying memory size.
pub type StackMemoryOffset = *const u8;

/// The relevant offsets into the stack that are currently in the register.
pub struct Registers {
    /// Instruction pointer.
    pub instruction_ptr: StackMemoryOffset,
    /// Stack pointer.
    pub stack_ptr: StackMemoryOffset,
    /// Frame pointer.
    pub frame_ptr: StackMemoryOffset,
}

/// This is an opaque type that refers to a memory address in a given process. Under the hood
/// it uses the c_void pointer type, but it only uses it to store the memory address, never
/// to actually access the value. Accessing the memory would be an unsafe operation. Hence we
/// can safely implement the Send trait for this struct.
///
/// See: https://doc.rust-lang.org/nomicon/ffi.html#representing-opaque-structs
#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(C)]
pub struct ProcessMemoryAddress {
    /// Important! Never access what this points to. It should serve as an opaque type.
    value: *mut core::ffi::c_void,
}

impl ProcessMemoryAddress {
    fn new(value: *mut core::ffi::c_void) -> ProcessMemoryAddress {
        ProcessMemoryAddress { value: value }
    }

    fn null() -> ProcessMemoryAddress {
        ProcessMemoryAddress {
            value: std::ptr::null_mut(),
        }
    }

    fn is_null(&self) -> bool {
        self.value.is_null()
    }
}

/// This unsafe should be fine, as we only store the value of the memory address, but do not
/// access it.
unsafe impl Send for ProcessMemoryAddress {}

/// Implement serialization for serde, which casts the value to a u64.
impl Serialize for ProcessMemoryAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.value as u64)
    }
}

/// The NativeStack represents how we store the information about the current stack. The Registers
/// struct contains the offset into the stack memory space, while this struct contains the offsets
/// for the current process's memory space. Hence the offset size will be much larger than u8.
pub struct NativeStack {
    instruction_ptrs: [ProcessMemoryAddress; MAX_NATIVE_FRAMES],
    stack_ptrs: [ProcessMemoryAddress; MAX_NATIVE_FRAMES],
    count: usize,
}

impl NativeStack {
    pub fn new() -> Self {
        NativeStack {
            instruction_ptrs: [ProcessMemoryAddress::null(); MAX_NATIVE_FRAMES],
            stack_ptrs: [ProcessMemoryAddress::null(); MAX_NATIVE_FRAMES],
            count: 0,
        }
    }

    pub fn add_register_to_stack(
        &mut self,
        instruction_ptr: *mut std::ffi::c_void,
        stack_ptr: *mut std::ffi::c_void,
    ) -> Result<(), ()> {
        if !(self.count < MAX_NATIVE_FRAMES) {
            return Err(());
        }
        self.instruction_ptrs[self.count] = ProcessMemoryAddress::new(instruction_ptr);
        self.stack_ptrs[self.count] = ProcessMemoryAddress::new(stack_ptr);
        self.count = self.count + 1;
        Ok(())
    }
}

#[derive(Debug)]
pub struct StackForSerialization {
    // -1 if not present.
    prefix: Option<usize>,
    instruction_ptr: ProcessMemoryAddress,
}

type StackTable = Vec<StackForSerialization>;

// This struct handles JSON serialization.
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
        for BufferEntry {
            value,
            created_at: _,
        } in buffer.iter()
        {
            let Sample {
                native_stack,
                // TODO - The entries need to be read once for each thread.
                thread_id: _,
            } = value;

            let mut last_matching_stack_index = None;

            // TODO - This loop needs to be reveresed. The leaf most functions are at the
            // beginning of the native stack.

            // Convert the native stack in the buffer to a StackTable. The StackTable
            // deduplicates the information already in the buffer. The StackTable is
            // well ordered, in that a leaf stack will always be after a root stack.
            for native_stack_instruction_ptr in native_stack.instruction_ptrs.iter() {
                if native_stack_instruction_ptr.is_null() {
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
                                last_matching_stack_index = stack_index;
                                // We've found the correct stack.
                                break;
                            }
                            // Continue checking the next stack in the stack table.
                            stack_index = Some(stack_index.unwrap() + 1);
                        }
                        None => {
                            // This is the end of the stack table, add an entry here.
                            stack_table.push(StackForSerialization {
                                prefix: last_matching_stack_index,
                                instruction_ptr: *native_stack_instruction_ptr,
                            });
                            // Update the stack index to the inserted entry.
                            stack_index = Some(stack_table.len() - 1);
                            last_matching_stack_index = stack_index;
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

    pub fn serialize_stack_table(&self) -> serde_json::Value {
        // TODO - Perhaps generate these values in the target format to make this faster so
        // that we don't have to transform the data again.
        let prefix: Vec<i32> = self
            .stack_table
            .iter()
            .map(|stack| match stack.prefix {
                Some(value) => value as i32,
                None => -1,
            })
            .collect();

        let mut zeros = Vec::with_capacity(self.stack_table.len());
        zeros.resize(self.stack_table.len(), 0);

        // For now the frame table matches the stack table, so just print out the vectors.
        let frame: Vec<usize> = self
            .stack_table
            .iter()
            .enumerate()
            .map(|(index, _)| index)
            .collect();

        json!({
            "frame": frame,
            "category": zeros,
            "subcategory": zeros,
            "prefix": prefix,
            "length": self.stack_table.len(),
        })
    }

    pub fn serialize_samples(&self) -> serde_json::Value {
        let mut zeros = Vec::with_capacity(self.stack_table.len());
        zeros.resize(self.stack_table.len(), 0);

        // For now the frame table matches the stack table, so just print out the vectors.
        let time: Vec<u64> = self
            .buffer
            .iter()
            .map(|entry| {
                entry
                    .created_at
                    .duration_since(*self.profiler_start)
                    .as_millis() as u64
            })
            .collect();

        json!({
            "stack": self.buffer_entry_to_stack_table,
            "time": time,
            "length": self.buffer_entry_to_stack_table.len(),
        })
    }

    pub fn serialize_frame_table(&self) -> serde_json::Value {
        // TODO - Perhaps generate these values in the target format to make this faster so
        // that we don't have to transform the data again.
        let address: Vec<ProcessMemoryAddress> = self
            .stack_table
            .iter()
            .map(|stack| stack.instruction_ptr)
            .collect();

        let mut zeros = Vec::with_capacity(self.stack_table.len());
        zeros.resize(self.stack_table.len(), 0);

        let mut nulls = Vec::with_capacity(self.stack_table.len());
        nulls.resize(self.stack_table.len(), Value::Null);

        json!({
          "address": address,
          "category": zeros,
          "subcategory": zeros,
          "func": zeros,
          "innerWindowID": nulls,
          "implementation": nulls,
          "line": nulls,
          "column": nulls,
          "optimizations": nulls,
          "length": self.stack_table.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn add_stack_sample(stacks: &Vec<u8>) -> Sample {
        let mut native_stack = NativeStack::new();
        for (index, stack) in stacks.iter().enumerate() {
            native_stack.instruction_ptrs[index] =
                ProcessMemoryAddress::new(*stack as *mut std::ffi::c_void);
        }
        Sample {
            native_stack: native_stack,
            thread_id: 0,
        }
    }

    fn create_buffer_for_tests(
        profiler_start: &Instant,
        test_data: Vec<Vec<u8>>,
    ) -> TimeExpiringBuffer<Sample> {
        let mut buffer = TimeExpiringBuffer::new(Duration::new(60, 0));
        for (index, stacks) in test_data.iter().enumerate() {
            buffer.push_back_at(
                add_stack_sample(&stacks),
                *profiler_start + Duration::from_millis(index as u64),
            );
        }
        buffer
    }

    #[test]
    fn can_create_a_stack_table() {
        // Each entry in the test data array represents a sample. Each sample is added with
        // an stack that is made up of instruction addresses. The instruction addresses were
        // chosen such that the instruction address would be similar to the stack index.
        let profiler_start = Instant::now();
        let buffer = create_buffer_for_tests(
            &profiler_start,
            vec![
                vec![0x10, 0x11, 0x12],
                vec![0x10, 0x11, 0x12, 0x13],
                vec![0x10, 0x11, 0x12, 0x13, 0x14],
                vec![0x10, 0x15, 0x16],
                vec![0x10, 0x15, 0x16, 0x17],
                vec![0x10, 0x11, 0x12],
            ],
        );
        let serializer = SamplesSerializer::new(&profiler_start, &buffer);
        // Convert the stack table into an easy to assert tuple of form (instruction_ptr, prefix).
        let stack_table: Vec<(ProcessMemoryAddress, Option<usize>)> = serializer
            .stack_table
            .iter()
            .map(|stack_for_serialization| {
                (
                    stack_for_serialization.instruction_ptr,
                    stack_for_serialization.prefix,
                )
            })
            .collect();

        // A simple test helper to coerce the value correctly.
        fn from_u8(value: u8) -> ProcessMemoryAddress {
            ProcessMemoryAddress::new(value as *mut std::ffi::c_void)
        }

        // This assertion tests that the instruction pointers are all sequential, as the test
        // data was laid out this way. Finally, the prefixes should match the structure
        // of the test dat.
        assert_eq!(
            stack_table,
            [
                // (instruction_ptr, prefix)
                (from_u8(0x10), None),
                (from_u8(0x11), Some(0)),
                (from_u8(0x12), Some(1)),
                (from_u8(0x13), Some(2)),
                (from_u8(0x14), Some(3)),
                (from_u8(0x15), Some(0)),
                (from_u8(0x16), Some(5)),
                (from_u8(0x17), Some(6)),
            ],
        );

        // Each stack should point to its unique entry. Of special note: the first and last entry
        // should point to the same stack.
        assert_eq!(
            serializer.buffer_entry_to_stack_table,
            vec![2, 3, 4, 6, 7, 2]
        );
    }

    #[test]
    fn can_serialize_a_stack_table() {
        // Each entry in the test data array represents a sample. Each sample is added with
        // an stack that is made up of instruction addresses. The instruction addresses were
        // chosen such that the instruction address would be the same as the stack index.
        let profiler_start = Instant::now();
        let buffer = create_buffer_for_tests(
            &profiler_start,
            vec![
                vec![0x10, 0x11, 0x12],
                vec![0x10, 0x11, 0x12, 0x13],
                vec![0x10, 0x11, 0x12, 0x13, 0x14],
                vec![0x10, 0x15, 0x16],
                vec![0x10, 0x15, 0x16, 0x17],
                vec![0x10, 0x11, 0x12],
            ],
        );
        let serializer = SamplesSerializer::new(&profiler_start, &buffer);

        assert_eq!(
            serializer.serialize_stack_table(),
            json!({
                "frame": [0, 1, 2, 3, 4, 5, 6, 7],
                "prefix": [-1, 0, 1, 2, 3, 0, 5, 6],
                "category": [0, 0, 0, 0, 0, 0, 0, 0],
                "subcategory": [0, 0, 0, 0, 0, 0, 0, 0],
                "length": 8,
            })
        );

        assert_eq!(
            serializer.serialize_frame_table(),
            json!({
                "address": [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17],
                "category": [0, 0, 0, 0, 0, 0, 0, 0],
                "subcategory": [0, 0, 0, 0, 0, 0, 0, 0],
                "func": [0, 0, 0, 0, 0, 0, 0, 0],
                "innerWindowID": [null, null, null, null, null, null, null, null],
                "implementation": [null, null, null, null, null, null, null, null],
                "line": [null, null, null, null, null, null, null, null],
                "column": [null, null, null, null, null, null, null, null],
                "optimizations": [null, null, null, null, null, null, null, null],
                "length": 8,
            })
        );

        assert_eq!(
            serializer.serialize_samples(),
            json!({
                "stack": [2, 3, 4, 6, 7, 2],
                "time": [0, 1, 2, 3, 4, 5],
                "length": 6,
            })
        );
    }
}
