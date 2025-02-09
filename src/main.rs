use chrono::Local;
use error::{Generify, MyResult, Standardize};
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{WaitPidFlag, WaitStatus};
use nix::unistd::{fork, Pid};
use nix::{sys::wait::waitpid, unistd::ForkResult};
use std::thread;
use std::time::SystemTime;
use std::{thread::sleep, time::Duration};
mod clipboard;
mod error;
mod log;
mod sync;

fn main() {
    let mut c = 0;
    loop {
        match unsafe { fork() }.expect("Failed to fork") {
            ForkResult::Parent { child } => {
                if c == 0 {
                    log::info!("started clipboard sync manager");
                }
                kill_after(child, 600);
                let status = waitpid(Some(child), None);
                log::info!("child process completed with: {:?}", status);
                sleep(Duration::from_secs(1));
                c += 1;
            }
            ForkResult::Child => run(),
        }
    }
}

pub fn kill_after(pid: Pid, seconds: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(seconds));
        if let Ok(WaitStatus::StillAlive) = waitpid(Some(pid), Some(WaitPidFlag::WNOWAIT)) {
            log::info!("killing subprocess {pid}");
            signal::kill(pid, Signal::SIGTERM).unwrap();
        }
    });
}

fn run() {
    log::info!("starting clipboard sync");
    loop_with_error_pain_management(
        sync::get_clipboards().unwrap(), //
        |cb| sync::keep_synced(cb),
        |_| sync::get_clipboards().unwrap(),
    )
    .unwrap();
}

/// Execute an action with a sophistocated retry mechanism
/// If the action fails:
/// - 1. run a recovery step to manipulate the input
/// - 2. attempt to execute the action again
/// If the action fails too frequently, exit
fn loop_with_error_pain_management<
    Input,
    Return,
    Action: Fn(&Input) -> MyResult<Return>,
    Recovery: Fn(Input) -> Input,
>(
    // data passed into action and reset by recovery
    initial_input: Input,
    // action to attempt on every iteration
    action: Action,
    // action to attempt on every error - errors here are not yet handled. you can panic if necessary
    recovery: Recovery,
) -> MyResult<Return> {
    let mut input = initial_input;
    let mut errorcount: u64 = 0;
    let mut first_error: SystemTime = SystemTime::UNIX_EPOCH;
    let mut last_error: SystemTime = SystemTime::UNIX_EPOCH;
    loop {
        match action(&input) {
            Ok(ret) => return Ok(ret),
            Err(err) => {
                log::error!("action exited with error: {:?}", err);
                let now = SystemTime::now();
                input = recovery(input);
                if errorcount == 0 {
                    first_error = now;
                } else {
                    if SystemTime::now().duration_since(last_error).unwrap()
                        > Duration::from_secs(10)
                    {
                        errorcount = 0;
                    } else {
                        let error_session_seconds = SystemTime::now()
                            .duration_since(first_error)
                            .unwrap()
                            .as_secs();
                        let error_session_rate_scaled = errorcount
                            .checked_mul(100)
                            .unwrap_or(u64::MAX)
                            .checked_div(error_session_seconds)
                            .unwrap_or(u64::MAX);
                        let error_pain = error_session_rate_scaled
                            .checked_add(error_session_seconds)
                            .unwrap_or(u64::MAX);
                        if error_pain > 100 {
                            Err(format!("too many errors, exiting"))
                                .standardize()
                                .generify()?;
                        }
                    }
                }
                last_error = now;
                errorcount += 1;
                sleep(Duration::from_millis(1000));
            }
        }
        log::info!("retrying");
    }
}
