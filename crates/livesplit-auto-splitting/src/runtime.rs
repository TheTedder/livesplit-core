use crate::{
    environment::Environment,
    std_stream::{stderr, stdout},
    timer::Timer,
    InterruptHandle,
};
use std::{cell::RefCell, mem, rc::Rc};
use wasi_cap_std_sync::WasiCtxBuilder;
use wasmtime::{Config, Engine, Export, Instance, Linker, Module, Store, TypedFunc};
use wasmtime_wasi::Wasi;

// TODO: Check if there's any memory leaks due to reference cycles. The
// exports keep the instance alive which keeps the imports alive, which all
// keep the environment alive, which keeps the memory alive, which may keep the
// instance alive -> reference cycle.
pub struct Runtime<T> {
    instance: Instance,
    is_configured: bool,
    env: Rc<RefCell<Environment<T>>>,
    timer_state: TimerState,
    update: Option<TypedFunc<(), ()>>,
    is_loading_val: Option<bool>,
}

impl<T: Timer> Runtime<T> {
    pub fn new(binary: &[u8], timer: T) -> anyhow::Result<Self> {
        let engine = Engine::new(Config::new().interruptable(true))?;
        let store = Store::new(&engine);
        let module = Module::from_binary(&engine, binary)?;
        let env = Rc::new(RefCell::new(Environment::new(timer)));

        let mut linker = Linker::new(&store);

        linker.func("env", "start", {
            let env = env.clone();
            move || env.borrow_mut().start()
        })?;

        linker.func("env", "split", {
            let env = env.clone();
            move || env.borrow_mut().split()
        })?;

        linker.func("env", "reset", {
            let env = env.clone();
            move || env.borrow_mut().reset()
        })?;

        linker.func("env", "attach", {
            let env = env.clone();
            move |ptr, len| env.borrow_mut().attach(ptr, len)
        })?;

        linker.func("env", "detach", {
            let env = env.clone();
            move |process| env.borrow_mut().detach(process)
        })?;

        linker.func("env", "read_into_buf", {
            let env = env.clone();
            move |process, address, buf_ptr, buf_len| {
                env.borrow_mut()
                    .read_into_buf(process, address, buf_ptr, buf_len)
            }
        })?;

        // linker.func("env", "scan_signature", {
        //     let env = env.clone();
        //     move |ptr, len| env.borrow_mut().scan_signature(ptr, len)
        // })?;

        linker.func("env", "print_message", {
            let env = env.clone();
            move |ptr, len| env.borrow_mut().print_message(ptr, len)
        })?;

        linker.func("env", "set_variable", {
            let env = env.clone();
            move |key_ptr, key_len, value_ptr, value_len| {
                env.borrow_mut()
                    .set_variable(key_ptr, key_len, value_ptr, value_len)
            }
        })?;

        linker.func("env", "set_game_time", {
            let env = env.clone();
            move |secs, nanos| env.borrow_mut().set_game_time(secs, nanos)
        })?;

        let wasi_ctx = WasiCtxBuilder::new()
            .stdout(Box::new(stdout()))
            .stderr(Box::new(stderr()))
            .build()
            .unwrap();

        Wasi::new(&store, wasi_ctx)
            .add_to_linker(&mut linker)
            .unwrap();

        let instance = linker.instantiate(&module)?;
        env.borrow_mut().memory = instance.exports().find_map(Export::into_memory);

        let update = instance.get_typed_func("update").ok();

        Ok(Self {
            instance,
            is_configured: false,
            env,
            timer_state: TimerState::NotRunning,
            update,
            is_loading_val: None,
        })
    }

    pub fn interrupt_handle(&self) -> InterruptHandle {
        self.instance
            .store()
            .interrupt_handle()
            .expect("We configured the runtime to produce an interrupt handle")
    }

    pub fn step(&mut self) -> anyhow::Result<()> {
        if !self.is_configured {
            // TODO: _start is kind of correct, but not in the long term. They are
            // intending for us to use a different function for libraries. Look into
            // reactors.
            if let Ok(func) = self.instance.get_typed_func("_start") {
                func.call(())?;
            }

            // TODO: Do we error out if this doesn't exist?
            if let Ok(func) = self.instance.get_typed_func("configure") {
                func.call(())?;
            }
            self.is_configured = true;
        }

        // {
        //     let mut just_connected = false;

        //     let mut env = self.env.borrow_mut();
        //     if env.process.is_none() {
        //         env.process = match Process::with_name(&env.process_name) {
        //             Ok(p) => Some(p),
        //             Err(_) => return Ok(None),
        //         };
        //         log::info!(target: "Auto Splitter", "Hooked");
        //         just_connected = true;
        //     }
        //     if env.update_values(just_connected).is_err() {
        //         log::info!(target: "Auto Splitter", "Unhooked");
        //         env.process = None;
        //         if !just_connected {
        //             if let Some(unhooked) = &self.unhooked {
        //                 unhooked.call(())?;
        //             }
        //         }
        //         return Ok(None);
        //     }
        //     if just_connected {
        //         if let Some(hooked) = &self.hooked {
        //             hooked.call(())?;
        //         }
        //     }
        // }

        self.run_script()
    }

    pub fn set_state(&mut self, state: TimerState) {
        self.timer_state = state;
    }

    fn run_script(&mut self) -> anyhow::Result<()> {
        if let Some(update) = &self.update {
            update.call(())?;
        }

        // match self.timer_state {
        //     TimerState::NotRunning => {
        //         if let Some(should_start) = &self.should_start {
        //             if should_start.call(())? != 0 {
        //                 return Ok(Some(TimerAction::Start));
        //             }
        //         }
        //     }
        //     TimerState::Running => {
        //         if let Some(is_loading) = &self.is_loading {
        //             self.is_loading_val = Some(is_loading.call(())? != 0);
        //         }
        //         if let Some(game_time) = &self.game_time {
        //             self.game_time_val = Some(game_time.call(())?).filter(|v| !v.is_nan());
        //         }

        //         if let Some(should_split) = &self.should_split {
        //             if should_split.call(())? != 0 {
        //                 return Ok(Some(TimerAction::Split));
        //             }
        //         }
        //         if let Some(should_reset) = &self.should_reset {
        //             if should_reset.call(())? != 0 {
        //                 return Ok(Some(TimerAction::Reset));
        //             }
        //         }
        //     }
        //     TimerState::Finished => {
        //         if let Some(should_reset) = &self.should_reset {
        //             if should_reset.call(())? != 0 {
        //                 return Ok(Some(TimerAction::Reset));
        //             }
        //         }
        //     }
        // }

        Ok(())
    }

    pub fn is_loading(&self) -> Option<bool> {
        self.is_loading_val
    }

    pub fn drain_variable_changes(&mut self) -> impl Iterator<Item = (String, String)> {
        // TODO: This is kind of stupid. We lose all the capacity this way.
        mem::take(&mut self.env.borrow_mut().variable_changes).into_iter()
    }
}
