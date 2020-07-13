/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// This file is adapted from the servo project.

use crate::sampler::{NativeStack, Registers, Sampler, StackMemoryOffset, SuspendAndSampleError};
use libc;
use mach;
use std::mem;
use std::panic;
use std::process;

type MonitoredThreadId = mach::mach_types::thread_act_t;

/// This is the platform specific implementation of a sampler.
pub struct MacOsSampler {
    thread_id: MonitoredThreadId,
}

impl MacOsSampler {
    pub fn new() -> Box<dyn Sampler> {
        let thread_id = MacOsSampler::request_thread_id();
        Box::new(MacOsSampler { thread_id })
    }

    pub fn request_thread_id() -> MonitoredThreadId {
        unsafe { mach::mach_init::mach_thread_self() }
    }
}

impl Sampler for MacOsSampler {
    fn suspend_and_sample_thread(&self) -> Result<NativeStack, SuspendAndSampleError> {
        // -----------------------------------------------------
        // WARNING: The "critical section" begins here.

        // In the critical section:
        //  * Do not do any dynamic memory allocation.
        //  * Do not try to acquire any lock.
        //  * Or access any other unshareable resource.

        // First, don't let panic do its thing.
        // TODO - I'm not sure why this is here, as I dropped it in from the Servo code.
        // Perhaps a panic would break the flow of logic, or more likely, it would try
        // to create a heap allocation or take a lock, causing a deadlock.
        let current_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {
            // Avoiding any allocation or locking as part of standard panicking.
            process::abort();
        }));

        // Now do all the magic to collect the native stack.
        let native_stack = unsafe {
            // Attempt to suspend the thread.
            if let Err(()) = suspend_thread(self.thread_id) {
                panic::set_hook(current_hook);
                return Err(SuspendAndSampleError::CouldNotSuspendThread);
            };

            // Attempt to get the registers, and perform a stackwalk. Suspension
            // could be fallible, while critical_frame_pointer_stack_walk() always returns
            // a result.
            let native_stack = match get_registers(self.thread_id) {
                Ok(registers) => Ok(critical_frame_pointer_stack_walk(registers)),
                Err(()) => Err(SuspendAndSampleError::CouldNotGetRegisters),
            };

            // Attempt to resume the thread. If we can't, it's bad enough that we
            // should abort the process.
            if let Err(()) = resume_thread(self.thread_id) {
                eprintln!("Unable to resume a sampled thread from the profiler.");
                process::abort();
            }

            native_stack
        };

        // Restore panic now that we are done with the unsafe block.
        panic::set_hook(current_hook);

        // End of "critical section".
        // -----------------------------------------------------

        native_stack
    }

    fn thread_id(&self) -> u32 {
        self.thread_id
    }
}

/// Convert a kernel response code into a Rust friendly value.
fn check_kern_return(kern_return: mach::kern_return::kern_return_t) -> Result<(), ()> {
    if kern_return != mach::kern_return::KERN_SUCCESS {
        return Err(());
    }
    Ok(())
}

/// Send a signal to suspend the thread at the given ID.
unsafe fn suspend_thread(thread_id: MonitoredThreadId) -> Result<(), ()> {
    check_kern_return(mach::thread_act::thread_suspend(thread_id))
}

/// Call the kernel to get the current registers.
unsafe fn get_registers(thread_id: MonitoredThreadId) -> Result<Registers, ()> {
    // These hold the results of the system calls.
    let mut state = mach::structs::x86_thread_state64_t::new();
    let mut state_count = mach::structs::x86_thread_state64_t::count();

    let kern_return = mach::thread_act::thread_get_state(
        thread_id,
        mach::thread_status::x86_THREAD_STATE64,
        (&mut state) as *mut _ as *mut _,
        &mut state_count,
    );

    check_kern_return(kern_return)?;

    // We got the registers, convert them into Rust-friendly values.
    Ok(Registers {
        instruction_ptr: state.__rip as StackMemoryOffset,
        stack_ptr: state.__rsp as StackMemoryOffset,
        frame_ptr: state.__rbp as StackMemoryOffset,
    })
}

/// This function wraps the system call to resume into a Rust-friendly type.
unsafe fn resume_thread(thread_id: MonitoredThreadId) -> Result<(), ()> {
    check_kern_return(mach::thread_act::thread_resume(thread_id))
}

/// /!\ WARNING /!\
/// Do not heap allocate during this function! This function is called in the critical section
/// of when a thread is suspended. This function assumes that the build is using frame
/// pointers, e.g. `RUSTFLAGS="-Cforce-frame-pointers=yes" cargo build`
unsafe fn critical_frame_pointer_stack_walk(registers: Registers) -> NativeStack {
    // Perform system calls to get the necessary information about the stack.
    let pthread_t = libc::pthread_self();
    let stacksize = libc::pthread_get_stacksize_np(pthread_t);
    let stack_address_min = libc::pthread_get_stackaddr_np(pthread_t);

    // This code assumes 8 byte sized pointers.
    const_assert_eq!(mem::size_of::<*const core::ffi::c_void>(), 8);
    let stack_address_max = stack_address_min.add(stacksize * 8);

    let mut native_stack = NativeStack::new();

    // Frame pointers point to the next frame pointer. Note the two *mut values.
    type RecursivePointer = *mut *mut std::ffi::c_void;

    let _ = native_stack.add_register_to_stack(
        registers.instruction_ptr as *mut std::ffi::c_void,
        registers.stack_ptr as *mut std::ffi::c_void,
    );

    // Example memory layout of the stack:
    //
    //                   start of stack
    //                   -----------------------------------------
    //  root-most frame: | 0x990 | ...............................
    //                   | ..... |
    //       |           | ..... |
    //     stack         | 0x550 | ... other frame information ...
    //     grows         | 0x548 | ... other frame information ...
    //      down         | 0x540 | stack pointer ???
    //       |           | 0x538 | instruction pointer "0x3858745"
    //       v           | 0x530 | frame pointer "0x628" (points to the next frame ^)
    //                   | ..... |
    // leaf-most frame:  | 0x430 | ... other frame information ...
    //                   | 0x428 | ... other frame information ...
    //                   | 0x420 | stack pointer ???
    //                   | 0x418 | instruction pointer "0x4246693"
    //                   | 0x410 | frame pointer "0x530" (points to the next frame ^)
    //                   -----------------------------------------
    //                   end of the stack

    // Walk the stack.
    let mut current_fp = registers.frame_ptr as RecursivePointer;
    while !current_fp.is_null() {
        if (current_fp as usize) < stack_address_min as usize {
            // Reached the end of the stack.
            break;
        }

        if current_fp as usize >= stack_address_max as usize {
            // Reached the beginning of the stack.
            break;
        }

        // Continue recursing into the frame pointers.
        let next_fp = *current_fp as RecursivePointer;

        // Offset the pointer by 1 and 2 respectively, looking up the next values inside
        // the frame's memory.
        let instruction_ptr = current_fp.add(1);
        let stack_ptr = current_fp.add(2);

        if let Err(()) = native_stack.add_register_to_stack(*instruction_ptr, *stack_ptr) {
            // The NativeStack struct is full. Quit stackwalking.
            break;
        }

        // Sanity check the stack walking.
        if
        // The next frame pointer must be higher than the current, or else we walked
        // into some unknown territory. See the memory diagram above for an explanation.
        (next_fp <= current_fp) ||
            // The next frame pointer is not 4-byte aligned. This means that stack walking failed
            // and is not where it should be.
            // TODO - I believe in 64 bit systems this should be 8-byte aligned and use 0b111.
            (next_fp as usize & 0b11 != 0)
        {
            // TODO - It would be nice to know if we failed here because of something crazy,
            // or if just because we reached the end of the stack. We can't panic, because
            // the hook is not set. This error could be bubbled up and handled outside
            // of the critical section. I'd prefer to be stricter here.
            break;
        }

        // Continue onto the next frame pointer.
        current_fp = next_fp;
    }
    native_stack
}
