use crate::{
    timer::Timer,
    InterruptHandle,
};
use std::{borrow::BorrowMut, cell::RefCell, rc::Rc, thread, time::{Duration, Instant}};
use read_process_memory::ProcessHandle;
use slotmap::{KeyData, SlotMap};
use sysinfo::{System, SystemExt};
use anyhow::anyhow;
use wasmtime::{Config, Engine, Export, Instance, Linker, Memory, Module, Store, Trap, TypedFunc};

slotmap::new_key_type! {
    struct ProcessKey;
}

pub struct Context<T: Timer> {
    pub tick_rate: Duration,
    pub processes: SlotMap<ProcessKey, ProcessHandle>,
    pub timer: T,
    pub info: System
}

pub struct Runtime<T: Timer> {
    instance: Instance,
    store: Store<Context<T>>,
    memory: Memory,
    is_configured: bool,
    update: Option<TypedFunc<(), ()>>,
    prev_time: Instant,
}

impl<T: Timer> Runtime<T> {
    pub fn new(binary: &[u8], timer: T) -> anyhow::Result<Self> {
        let engine = Engine::new(Config::new().interruptable(true))?;
        let store = Store::new(&engine, Context{
            tick_rate: Duration::from_secs_f64( 1.0 / 120.0),
            processes: SlotMap::with_key(),
            timer,
            info: System::new(),
        });
        let module = Module::from_binary(&engine, binary)?;
        let env = Rc::new(RefCell::new(Environment::new(timer)));

        let mut linker = Linker::new(&engine);

        linker.func_wrap("env", "start", {
            let env = env.clone();
            move || env.borrow_mut().start()
        })?;

        linker.func_wrap("env", "split", {
            let env = env.clone();
            move || env.borrow_mut().split()
        })?;

        linker.func_wrap("env", "reset", {
            let env = env.clone();
            move || env.borrow_mut().reset()
        })?;

        linker.func_wrap("env", "attach", {
            let env = env.clone();
            move |ptr, len| env.borrow_mut().attach(ptr, len)
        })?;

        linker.func_wrap("env", "detach", {
            let env = env.clone();
            move |process| env.borrow_mut().detach(process)
        })?;

        linker.func_wrap("env", "read_into_buf", {
            let env = env.clone();
            move |process, address, buf_ptr, buf_len| {
                env.borrow_mut()
                    .read_into_buf(process, address, buf_ptr, buf_len)
            }
        })?;

        linker.func_wrap("env", "set_tick_rate", {
            let env = env.clone();
            move |ticks_per_sec| env.borrow_mut().set_tick_rate(ticks_per_sec)
        })?;

        linker.func_wrap("env", "print_message", {
            let env = env.clone();
            move |ptr, len| env.borrow_mut().print_message(ptr, len)
        })?;

        linker.func_wrap("env", "set_game_time", {
            let env = env.clone();
            move |secs, nanos| env.borrow_mut().set_game_time(secs, nanos)
        })?;

        linker.func_wrap("env", "pause_game_time", {
            let env = env.clone();
            move || env.borrow_mut().pause_game_time()
        })?;

        linker.func_wrap("env", "resume_game_time", {
            let env = env.clone();
            move || env.borrow_mut().resume_game_time()
        })?;

        linker.func_wrap("env", "get_timer_state", {
            let env = env.clone();
            move || env.borrow().timer_state()
        })?;

        let instance = linker.instantiate(&mut store, &module)?;
        let memory = instance.exports(&mut store).find_map(Export::into_memory).ok_or(anyhow!("There is no memory to use"))?;
        let update = instance.get_typed_func(&mut store, "update").ok();

        Ok(Self {
            instance,
            store,
            is_configured: false,
            memory,
            update,
            prev_time: Instant::now(),
        })
    }

    pub fn fill_buf(
        &mut self,
        process: u64,
        address: u64,
        buf_ptr: u32,
        buf_len: u32,
    ) -> Result<u32, Trap> {
        let key = ProcessKey::from(KeyData::from_ffi(process as u64));

        let memory = self.memory.data_mut(&mut self.store);

        let ptr = buf_ptr as usize;
        let len = buf_len as usize;


        let process = self
            .processes
            .get(key)
            .ok_or_else(|| Trap::new(format!("Invalid process handle {}.", process)))?;

        let pid: ProcessHandle = process.as_u32().try_into().map_err(|_| Trap::new(format!("invalid PID: {}", process)))?;
        let res = pid.copy_address(
            address as usize,
        memory.get_mut(ptr..ptr + len)
    .ok_or_else(|| Trap::new("Index out of bounds"))
        );

        Ok(res.is_ok() as u32)
    }

    fn get_slice(&self, ptr: u32, len: u32) -> Result<&[u8], Trap> {
        let memory = self.
    }
    
    fn read_str(&mut self, ptr: u32, len: u32) -> Result<&str, Trap> {
        let bytes = self.get_slice(memory, ptr, len)?;
        str::from_utf8(bytes).map_err(trap_from_err)
    }
    
    pub fn interrupt_handle(&self) -> InterruptHandle {
        self.store
            .interrupt_handle()
            .expect("We configured the runtime to produce an interrupt handle")
    }

    pub fn step(&mut self) -> anyhow::Result<()> {
        if !self.is_configured {
            if let Ok(func) = self.instance.get_typed_func("configure") {
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
