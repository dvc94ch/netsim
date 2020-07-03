use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Runs callback in a new thread with the given timeout. Panics, if `func()` panics.
pub fn run_test<F: FnOnce() + Send + 'static>(seconds: u64, func: F) {
    let _ = env_logger::try_init();

    let (tx, rx) = mpsc::channel();

    let join_handle = thread::spawn(move || {
        func();
        drop(tx);
    });

    match rx.recv_timeout(Duration::from_secs(seconds)) {
        Ok(()) => unreachable!(),
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("test timed out!"),
        Err(mpsc::RecvTimeoutError::Disconnected) => (),
    };

    // FIXME: this sometimes panics, even if the thread of join_handle didn't.
    join_handle.join().unwrap();
}
