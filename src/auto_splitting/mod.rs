//! Live split one supports autosplitters written in a variety of languages by
//! interpreting wasm modules with [wasmtime](https://github.com/bytecodealliance/wasmtime).
//! A wasm blob which provides the right set of functions can be loaded into
//! livesplit's runtime and directly control the timer. (TODO: add docs for
//! the interface and/or link to example autosplitter)

use {
    crate::{timing::SharedTimer, TimeSpan, TimerPhase},
    crossbeam_channel::{bounded, unbounded, Sender},
    livesplit_auto_splitting::{
        Runtime as ScriptRuntime, Timer as AutoSplitTimer, TimerState,
    },
    std::{
        thread::{self, JoinHandle},
        time::Duration,
    },
};

/// Ways in which the autosplitter runtime can fail
#[derive(Debug, Copy, Clone, snafu::Snafu)]
pub enum Error {
    /// The autosplitter runtime's thread died
    ThreadStopped,
    /// The wasm module for the autosplitter failed to load
    LoadFailed,
}

type Result<T> = std::result::Result<T, Error>;

/// The autosplitter runtime is a thread running an event loop. It takes a
/// shared timer which the wasm autosplitter running inside can update. The only
/// communication possible with the runtime is to load or unload an
/// autosplitter. In the future we may expose an interface to communicate with
/// autosplitters for uses such as configuration.
pub struct Runtime {
    sender: Sender<Request>,
    join_handle: Option<JoinHandle<Result<()>>>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.sender.send(Request::End).ok();
        self.join_handle.take().unwrap().join().ok();
    }
}

impl Runtime {
    /// Starts the runtime. Doesn't actually load an autosplitter until you ask
    /// it to.
    pub fn new(timer: SharedTimer) -> Self {
        let (sender, receiver) = unbounded();
        let join_handle = thread::spawn(move || -> Result<()> {
            'back_to_not_having_a_runtime: loop {
                let mut runtime = loop {
                    match receiver.recv().map_err(|_| Error::ThreadStopped)? {
                        Request::LoadScript(script, ret) => {
                            match ScriptRuntime::new(&script, AST(timer.clone())) {
                                Ok(r) => {
                                    ret.send(Ok(())).ok();
                                    break r;
                                }
                                Err(_) => ret.send(Err(Error::LoadFailed)).unwrap(),
                            };
                        }
                        Request::UnloadScript(ret) => {
                            log::warn!(target: "Auto Splitter", "Attempted to unload already unloaded script");
                            ret.send(()).ok();
                        }
                        Request::End => return Ok(()),
                    };
                };
                log::info!(target: "Auto Splitter", "Loaded script");
                loop {
                    // TODO: Handle the different kinds of errors here, such as
                    // needing to end
                    if let Ok(request) = receiver.try_recv() {
                        match request {
                            Request::LoadScript(script, ret) => {
                                match ScriptRuntime::new(&script, AST(timer.clone())) {
                                    Ok(r) => {
                                        ret.send(Ok(())).ok();
                                        runtime = r;
                                    }
                                    Err(_) => {
                                        ret.send(Err(Error::LoadFailed)).ok();
                                    }
                                }
                                log::info!(target: "Auto Splitter", "Reloaded script");
                            }
                            Request::UnloadScript(ret) => {
                                log::info!(target: "Auto Splitter", "Unloaded script");
                                ret.send(()).ok();
                                continue 'back_to_not_having_a_runtime;
                            }
                            Request::End => return Ok(()),
                        }
                    }
                    if let Err(e) = runtime.step() {
                        log::error!(target: "Auto Splitter", "Unloaded due to failure: {}", e);
                        continue 'back_to_not_having_a_runtime;
                    };
                    runtime.sleep();
                }
            }
        });

        Self {
            sender,
            join_handle: Some(join_handle),
        }
    }

    /// Attempt to load a wasm blob containing an autosplitter module. This call
    /// will block until the autosplitter has either loaded successfully or
    /// failed.
    pub fn load_script(&self, script: Vec<u8>) -> Result<()> {
        // TODO: replace with `futures:channel::oneshot`
        let (sender, receiver) = bounded(1);
        self.sender
            .send(Request::LoadScript(script, sender))
            .map_err(|_| Error::ThreadStopped)?;
        receiver
            .recv()
            .map_err(|_| Error::ThreadStopped)
            .and_then(std::convert::identity)
    }

    /// Unload the current autosplitter. This will _not_ return an error if
    /// there isn't an autosplitter loaded.
    pub fn unload_script(&self) -> Result<()> {
        // TODO: replace with `futures:channel::oneshot`
        let (sender, receiver) = bounded(1);
        self.sender
            .send(Request::UnloadScript(sender))
            .map_err(|_| Error::ThreadStopped)?;
        receiver.recv().map_err(|_| Error::ThreadStopped)
    }
}

enum Request {
    LoadScript(Vec<u8>, Sender<Result<()>>),
    UnloadScript(Sender<()>),
    End,
}

// This newtype is required because we SharedTimer is an Arc, so we can't impl
// the autosplit Timer trait on it
struct AST(SharedTimer);

impl AutoSplitTimer for AST {
    fn timer_state(&self) -> TimerState {
        match self.0.read().current_phase() {
            TimerPhase::NotRunning => TimerState::NotRunning,
            TimerPhase::Running | TimerPhase::Paused => TimerState::Running,
            TimerPhase::Ended => TimerState::Finished,
        }
    }

    fn start(&mut self) {
        self.0.write().start()
    }

    fn split(&mut self) {
        self.0.write().split()
    }

    fn reset(&mut self) {
        self.0.write().reset(true)
    }

    fn set_game_time(&mut self, time: Duration) {
        // TODO: use TimeSpan::from()
        // self.0.write().set_game_time(time.into());
        self.0
            .write()
            .set_game_time(TimeSpan::from_milliseconds(time.as_millis() as f64));
    }

    fn pause_game_time(&mut self) {
        self.0.write().pause_game_time()
    }

    fn resume_game_time(&mut self) {
        self.0.write().resume_game_time()
    }

    fn is_game_time_paused(&self) -> bool {
        self.0.read().is_game_time_paused()
    }
}
