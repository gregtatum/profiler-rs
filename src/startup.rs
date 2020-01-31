use ipc_channel::ipc;
use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};
use std::env;
use std::process::Command;

/// This module is in charge of deciding what mode to start in. Conceptually, the ProfilerService
/// holds the buffers used for storing profiler data, while the App sends data over to the
/// ProfilerService. For now, there is only one App, and multiple ProfilerServices. In the future
/// this will change to one ProfilerService and many Apps connected to it. However, the ipc-channel
/// does not support multiple producers at this time.

///
pub enum StartupMode {
    ProfilerService,
    App,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ToProfilerService {
    Hello(u32),
}

pub fn launch() {
    // Set the environment variable to hard code logging to be turned on.
    std::env::set_var("RUST_LOG", "trace");
    pretty_env_logger::init();

    match deduce_startup_mode() {
        StartupMode::ProfilerService => launch_profiler_service(),
        StartupMode::App => launch_app(),
    }
}

pub fn get_token_from_args() -> Option<String> {
    // Go through the args and try to find the string that follows --profiler-service.
    let mut is_token_string = false;
    for argument in env::args() {
        if is_token_string {
            return Some(argument);
        }
        if argument == "--profiler-service" {
            is_token_string = true;
        }
    }
    None
}

pub fn launch_profiler_service() {
    info!("Starting the profiler service.");

    let (unprivileged_content_sender, unprivileged_content_receiver) =
        ipc::channel::<ToProfilerService>().unwrap();

    let token = get_token_from_args().expect("No --profiler-service with an argument was passed");

    let connection_bootstrap: ipc::IpcSender<IpcSender<ToProfilerService>> =
        IpcSender::connect(token).unwrap();

    connection_bootstrap
        .send(unprivileged_content_sender)
        .unwrap();

    let hello = unprivileged_content_receiver
        .recv()
        .expect("Expected to receive a hello.");

    info!("Received a hello {:?}", hello);
}

pub fn launch_app() {
    use ipc_channel::ipc::IpcOneShotServer;
    info!("Starting the app.");

    // Spawn the profiler service.
    let path = std::env::current_exe().expect("Unable to get the path to the current executable");

    // The one-shot will boot-strap the IPC channel.
    let (server, token) = IpcOneShotServer::<IpcSender<ToProfilerService>>::new()
        .expect("Failed to create IPC one-shot server.");

    // Spin up the profiler service.
    let mut profiler_service = Command::new(path)
        .arg("--profiler-service")
        .arg(token)
        .spawn()
        .expect("Unable to spawn the profiler service.");

    // Output some log information now that the processes are alive.
    info!("App PID: {}, ", std::process::id());
    info!("Profiler Service PID: {}, ", profiler_service.id());

    let (_receiver, sender) = server.accept().expect("Server failed to accept.");
    sender
        .send(ToProfilerService::Hello(7))
        .expect("Expected to send a hello the profiler service.");

    // Right now this app doesn't do anything, so wait until it exits.
    let exit_code = profiler_service
        .wait()
        .expect("Failed to wait on profiler service.");

    info!("The exit code of the profiler service was {}", exit_code);
}

pub fn deduce_startup_mode() -> StartupMode {
    for argument in env::args() {
        match argument.as_ref() {
            "--profiler-service" => return StartupMode::ProfilerService,
            _ => {}
        }
    }

    return StartupMode::App;
}
