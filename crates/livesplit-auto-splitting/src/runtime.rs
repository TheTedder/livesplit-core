use crate::{InterruptHandle, timer::Timer};
use anyhow::anyhow;
use log::info;
use read_process_memory::{CopyAddress, ProcessHandle};
use slotmap::{Key, KeyData, SlotMap};
use std::{convert::TryInto, error::Error, panic::catch_unwind, thread, time::{Duration, Instant}};
use sysinfo::{AsU32, ProcessExt, System, SystemExt};
use wasmtime::{Caller, Config, Engine, Extern, Instance, Linker, Module, Store, Trap, TypedFunc};

slotmap::new_key_type! {
    struct ProcessKey;
}

fn trap_from_err(e: impl Error + Send + Sync + 'static) -> Trap {
    Trap::new(anyhow::Error::from(e).to_string())
}

pub struct Context<T: Timer> {
    tick_rate: Duration,
    processes: SlotMap<ProcessKey, ProcessHandle>,
    timer: T,
    info: System,
}

pub struct Runtime<T: Timer> {
    instance: Instance,
    store: Store<Context<T>>,
    is_configured: bool,
    update: Option<TypedFunc<(), ()>>,
    prev_time: Instant,
}

impl<T: Timer> Runtime<T> {
    pub fn new(binary: &[u8], timer: T) -> anyhow::Result<Self> {
        let engine = Engine::new(Config::new().interruptable(true))?;
        let mut store = Store::new(
            &engine,
            Context {
                tick_rate: Duration::from_secs_f64(1.0 / 120.0),
                processes: SlotMap::with_key(),
                timer,
                info: System::new(),
            },
        );
        let module = Module::new(&engine, binary)?;

        let mut linker = Linker::new(&engine);

        linker.func_wrap("env", "start", |mut caller: Caller<'_, Context<T>>| {
            caller.data_mut().timer.start();
        })?;

        linker.func_wrap("env", "split", |mut caller: Caller<'_, Context<T>>| {
            caller.data_mut().timer.split();
        })?;

        linker.func_wrap("env", "reset", |mut caller: Caller<'_, Context<T>>| {
            caller.data_mut().timer.reset();
        })?;

        linker.func_wrap("env", "attach", |mut caller: Caller<'_, Context<T>>, ptr, len| -> Result<u64, Trap> {
            let mem = Self::get_memory(&mut caller)?;
            let process_name = Self::read_str(mem, ptr, len)?;
            let data = caller.data_mut();
            let info = &mut data.info;
            info.refresh_processes();
            let mut processes = info.process_by_name(process_name.as_str());

            let key = if let Some(p) = processes.pop() {
                // TODO: handle the case where we got multiple processes with the same name
                info!("Attached to a new process: {}", process_name);
                let pid = p.pid();
                match pid.as_u32().try_into() {
                    Ok(handle) => data.processes.insert(handle),
                    Err(_) => {
                        info!("Couldn't attach to process with pid {}", pid);
                        ProcessKey::null()
                    },
                }
            } else {
                info!("Couldn't find process: {}", process_name);
                ProcessKey::null()
            };
            Ok(key.data().as_ffi())
        })?;

        linker.func_wrap("env", "detach", |mut caller: Caller<'_, Context<T>>, process: u64 | -> Result<(), Trap> {
            let key = ProcessKey::from(KeyData::from_ffi(process));

            caller.data_mut().processes
                .remove(key)
                .ok_or_else(|| Trap::new(format!("Invalid process handle {}.", process))).and(Ok(()))
        })?;

        linker.func_wrap("env", "read_into_buf",|
            mut caller: Caller<'_, Context<T>>,
            process: u64,
            address: u64,
            buf_ptr: u32,
            buf_len: u32,
        | -> Result<(), Trap> {
            let key = ProcessKey::from(KeyData::from_ffi(process));
            
            let (memory, data) = Self::get_memory_mut(&mut caller)?;
            let start = buf_ptr as usize;
            let end = start + buf_len as usize;
            
            let handle = data.processes
                .get(key)
                .ok_or_else(|| Trap::new(format!("Invalid process handle {}.", process)))?;

            handle.copy_address(
                address as usize,
                memory
                    .get_mut(start..end)
                    .ok_or_else(|| Trap::new("Index out of bounds"))?,
            ).map_err(trap_from_err)
        })?;

        linker.func_wrap("env", "set_tick_rate", |mut caller: Caller<'_, Context<T>>, ticks_per_sec: f64| {
            caller.data_mut().tick_rate = Duration::from_secs_f64(ticks_per_sec.recip())
        })?;

        linker.func_wrap("env", "print_message", |mut caller: Caller<'_, Context<T>>, ptr: u32, len: u32| -> Result<(), Trap> {
            let mem = Self::get_memory(&mut caller)?;
            let message = Self::read_str(mem, ptr, len)?;
            info!(target: "Auto Splitter", "{}", message);
            Ok(())
        })?;

        linker.func_wrap("env", "set_game_time", |mut caller: Caller<'_, Context<T>>, secs: f64| -> Result<(), Trap> {
            let dur: Duration = catch_unwind(|| {
                Duration::from_secs_f64(secs)
            }).or(Err(Trap::new(format!("Could not instantiate a Duration with the following float value: {}", secs))))?;

            caller.data_mut().timer.set_game_time(dur);
            Ok(())
        })?;

        linker.func_wrap("env", "pause_game_time", |mut caller: Caller<'_, Context<T>> | {
            caller.data_mut().timer.pause_game_time();
        })?;

        linker.func_wrap("env", "resume_game_time", |mut caller: Caller<'_, Context<T>> | {
            caller.data_mut().timer.resume_game_time();
        })?;

        linker.func_wrap("env", "get_timer_state", |caller: Caller<'_, Context<T>> | -> u32 {
            caller.data().timer.timer_state() as u32
        })?;

        let instance = linker.instantiate(&mut store, &module)?;
        let update = instance.get_typed_func(&mut store, "update").ok();

        Ok(Self {
            instance,
            store,
            is_configured: false,
            update,
            prev_time: Instant::now(),
        })
    }
   
    fn get_memory<'a>(caller: &'a mut Caller<'_, Context<T>>) -> Result<&'a [u8], Trap> {
        match caller.get_export("memory") {
            Some(Extern::Memory(mem)) => Ok(mem.data(caller)),
            _ => Err(Trap::new("failed to find host memory"))
        }
    }

    fn get_memory_mut<'a>(caller: &'a mut Caller<'_, Context<T>>) -> Result<(&'a mut [u8], &'a mut Context<T>), Trap> {
        match caller.get_export("memory") {
            Some(Extern::Memory(mem)) => Ok(mem.data_and_store_mut(caller)),
            _ => Err(Trap::new("failed to find host memory"))
        }
    }

    fn read_str<'a>(mem: &[u8], ptr: u32, len: u32) -> Result<String, Trap> {
        let start = ptr as usize;
        let end = (ptr + len) as usize;
        let bytes = mem.get(start..end).ok_or(Trap::new("Index out of bounds"))?;
        String::from_utf8(bytes.into()).map_err(trap_from_err)
    }

    pub fn interrupt_handle(&self) -> InterruptHandle {
        self.store
            .interrupt_handle()
            .expect("We configured the runtime to produce an interrupt handle")
    }

    pub fn step(&mut self) -> anyhow::Result<()> {
        if !self.is_configured {
            if let Ok(func) = self.instance.get_typed_func(&mut self.store,"configure") {
                func.call(&mut self.store, ())?;
            } else {
                return Err(anyhow!("didn't expose a 'configure' function"));
            }
            self.is_configured = true;
        }
        self.run_script()
    }

    fn run_script(&mut self) -> anyhow::Result<()> {
        if let Some(update) = &self.update {
            update.call(&mut self.store, ())?;
        }
        Ok(())
    }

    pub fn sleep(&mut self) {
        let target = self.store.data().tick_rate;
        let delta = self.prev_time.elapsed();
        if delta < target {
            thread::sleep(target - delta);
        }
        self.prev_time = Instant::now();
    }
}
