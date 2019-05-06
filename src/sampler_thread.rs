use mach;
use std::sync::{Arc, Mutex};

struct ProfilerState {
    //
    generation: uint;
}

struct RegisteredThread {

}


struct SamplerThread {
    profiler_state: Arc<Mutex<ProfilerState>>,
    interval: Duration;
}

impl SamplerThread {
    pub fn new(interval_microseconds: uint) -> SamplerThread {
        SampleThread{
            interval: Duration::new(0, interval_microseconds * 1000)
        }
    }

    pub fn start() {
        {
            // Update the profiler state to show that it is being profiled.
            let mut state = self.profiler_state.lock().unwrap();
            assert!(!state.is_profiling, "Starting the profiler for this process, it should not already by profiling");
            state.is_profiling = true;
        }
        loop {
            {
                // Lock the profiler state
                let state = self.profiler_state.lock().unwrap();
                if (!state.is_profiling) {
                    return;
                }
                self.sample_thread();
            }
            sleep(self.interval);
        }
    }

    fn sample_thread() {

    }
}

fn check_kern_return(kret: mach::kern_return::kern_return_t) -> Result<(), ()> {
    if kret != mach::kern_return::KERN_SUCCESS {
        return Err(());
    }
    Ok(())
}

#[allow(unsafe_code)]
unsafe fn suspend_thread(thread_id: MonitoredThreadId) -> Result<(), ()> {
    check_kern_return(mach::thread_act::thread_suspend(thread_id))
}
