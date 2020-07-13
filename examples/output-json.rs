/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
mod utils;
extern crate profiler;
use serde_json;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
  // Initialize the profiler core.
  let profiler_core =
    profiler::core::MainThreadCore::new(Duration::new(60, 0), Duration::from_millis(10));

  // Create a shared reference to signal to the threads to shut down.
  let do_shutdown_threads = Arc::new(AtomicBool::new(false));

  let thread_handle = {
    let mut thread_registrar = profiler_core.get_thread_registrar();
    let do_shutdown_threads = do_shutdown_threads.clone();

    thread::spawn(move || {
      thread_registrar.register();
      loop {
        // Compute fibonacci numbers in a loop.
        profiler::core::add_marker(Box::new(profiler::markers::StaticStringMarker::new(
          "Fibonacci",
        )));
        utils::fibonacci(10000);
        if do_shutdown_threads.load(Ordering::Relaxed) {
          break;
        }
      }
    })
  };

  profiler_core.start_sampling();
  for _ in 0..100 {
    profiler_core.wait_for_one_sample();
  }

  // Signal to the threads that it's time to shut down.
  do_shutdown_threads.store(true, Ordering::Relaxed);
  thread_handle.join().expect("Joined the thread handle.");

  println!("Samples collected: {}", profiler_core.get_sample_count());
  println!("Serializing profile");
  let json = profiler_core.serialize();

  println!("Writing serde to file");
  let path = "examples/profile.json";
  serde_json::to_writer(
    &File::create("examples/profile.json").expect("Unable to create a file to output the JSON."),
    &json,
  )
  .expect("Unable to write the JSON to a file.");

  println!("Profile output to: {}", path);
}
