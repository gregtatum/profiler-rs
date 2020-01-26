/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// This file is adapted from the servo project.

use crate::sampler::{Address, NativeStack, Registers, Sampler, SuspendAndSampleError};
use libc;
use mach;
use std::panic;
use std::process;

type MonitoredThreadId = mach::mach_types::thread_act_t;

/// This is the platform specific implementation of a sampler.
pub struct MacOsSampler {
    thread_id: MonitoredThreadId,
}

impl MacOsSampler {
    #[allow(unsafe_code)]
    pub fn new() -> Box<dyn Sampler> {
        let thread_id = MacOsSampler::request_thread_id();
        Box::new(MacOsSampler { thread_id })
    }

    pub fn request_thread_id() -> MonitoredThreadId {
        unsafe { mach::mach_init::mach_thread_self() }
    }
}

impl Sampler for MacOsSampler {
    #[allow(unsafe_code)]
    fn suspend_and_sample_thread(&self) -> Result<NativeStack, SuspendAndSampleError> {
        // -----------------------------------------------------
        // WARNING: The "critical section" begins here.

        // In the critical section:
        //  * Do not do any dynamic memory allocation.
        //  * Do not try to acquire any lock.
        //  * Or access any other unshareable resource.

        // First, don't let panic do its thing.
        // TODO - I'm not sure why this is here, as I dropped it in from the Servo code.
        // Perhaps they break the flow of the logic or perhaps they allocate?
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
            // could be fallible, while frame_pointer_stack_walk() always returns
            // a result.
            let native_stack = match get_registers(self.thread_id) {
                Ok(regs) => Ok(frame_pointer_stack_walk(regs)),
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
fn check_kern_return(kret: mach::kern_return::kern_return_t) -> Result<(), ()> {
    if kret != mach::kern_return::KERN_SUCCESS {
        return Err(());
    }
    Ok(())
}

/// Send a signal to suspend the thread at the given ID.
#[allow(unsafe_code)]
unsafe fn suspend_thread(thread_id: MonitoredThreadId) -> Result<(), ()> {
    check_kern_return(mach::thread_act::thread_suspend(thread_id))
}

/// Call the kernel to get the current registers.
#[allow(unsafe_code)]
unsafe fn get_registers(thread_id: MonitoredThreadId) -> Result<Registers, ()> {
    // This holds the result of the
    let mut state = mach::structs::x86_thread_state64_t::new();
    let mut state_count = mach::structs::x86_thread_state64_t::count();

    let kret = mach::thread_act::thread_get_state(
        thread_id,
        mach::thread_status::x86_THREAD_STATE64,
        (&mut state) as *mut _ as *mut _,
        &mut state_count,
    );
    check_kern_return(kret)?;
    Ok(Registers {
        instruction_ptr: state.__rip as Address,
        stack_ptr: state.__rsp as Address,
        frame_ptr: state.__rbp as Address,
    })
}

/// This function wraps the system call to resume into a Rust-friendly type.
#[allow(unsafe_code)]
unsafe fn resume_thread(thread_id: MonitoredThreadId) -> Result<(), ()> {
    check_kern_return(mach::thread_act::thread_resume(thread_id))
}

#[allow(unsafe_code)]
unsafe fn frame_pointer_stack_walk(regs: Registers) -> NativeStack {
    // Note: this function will only work with code build with:
    // --dev,
    // or --with-frame-pointer.

    let pthread_t = libc::pthread_self();
    let stackaddr = libc::pthread_get_stackaddr_np(pthread_t);
    let stacksize = libc::pthread_get_stacksize_np(pthread_t);
    let mut native_stack = NativeStack::new();
    let pc = regs.instruction_ptr as *mut std::ffi::c_void;
    let stack = regs.stack_ptr as *mut std::ffi::c_void;
    let _ = native_stack.process_register(pc, stack);
    let mut current = regs.frame_ptr as *mut *mut std::ffi::c_void;
    while !current.is_null() {
        if (current as usize) < stackaddr as usize {
            // Reached the end of the stack.
            break;
        }
        if current as usize >= stackaddr.add(stacksize * 8) as usize {
            // Reached the beginning of the stack.
            // Assumining 64 bit mac(see the stacksize * 8).
            break;
        }
        let next = *current as *mut *mut std::ffi::c_void;
        let pc = current.add(1);
        let stack = current.add(2);
        if let Err(()) = native_stack.process_register(*pc, *stack) {
            break;
        }
        if (next <= current) || (next as usize & 3 != 0) {
            break;
        }
        current = next;
    }
    native_stack
}
