use crate::timer::Timer;

use log::info;
use read_process_memory::{CopyAddress, ProcessHandle};
use slotmap::{Key, KeyData, SlotMap};
use std::{convert::TryInto, error::Error, str, time::Duration};
use sysinfo::{self, Pid, ProcessExt, System, SystemExt};
use wasmtime::{Memory, Trap};

slotmap::new_key_type! {
    struct ProcessKey;
}

pub struct Environment<T> {
    pub memory: Option<Memory>,
    processes: SlotMap<ProcessKey, Pid>,
    pub tick_rate: Duration,
    timer: T,
    info: System,
}

impl<T: Timer> Environment<T> {
    pub fn new(timer: T) -> Self {
        Self {
            memory: None,
            processes: SlotMap::with_key(),
            tick_rate: Duration::from_secs(1) / 60,
            timer,
            info: System::new(),
        }
    }
}

fn trap_from_err(e: impl Error + Send + Sync + 'static) -> Trap {
    Trap::new(anyhow::Error::from(e).to_string())
}

fn get_bytes(memory: &mut Option<Memory>, ptr: i32, len: i32) -> Result<&mut [u8], Trap> {
    let memory = unsafe {
        memory
            .as_mut()
            .ok_or_else(|| Trap::new("There is no memory to use"))?
            .data_unchecked_mut()
    };

    let ptr = ptr as u32 as usize;
    let len = len as u32 as usize;

    memory
        .get_mut(ptr..ptr + len)
        .ok_or_else(|| Trap::new("Index out of bounds"))
}

fn read_str(memory: &mut Option<Memory>, ptr: i32, len: i32) -> Result<&str, Trap> {
    let bytes = get_bytes(memory, ptr, len)?;
    str::from_utf8(bytes).map_err(trap_from_err)
}

impl<T: Timer> Environment<T> {
    pub fn start(&mut self) {
        self.timer.start()
    }

    pub fn split(&mut self) {
        self.timer.split()
    }

    pub fn reset(&mut self) {
        self.timer.reset()
    }

    pub fn timer_state(&self) -> i32 {
        self.timer.timer_state() as i32
    }

    pub fn attach(&mut self, ptr: i32, len: i32) -> Result<i64, Trap> {
        let process_name = read_str(&mut self.memory, ptr, len)?;
        self.info.refresh_processes();
        let mut processes = self.info.process_by_name(process_name);
        let key = if let Some(p) = processes.pop() {
            // TODO: handle the case where we got multiple processes with the same name
            info!("Attached to a new process: {}", process_name);
            self.processes.insert(p.pid())
        } else {
            info!("Couldn't find process: {}", process_name);
            ProcessKey::null()
        };
        Ok(key.data().as_ffi() as i64)
    }

    pub fn detach(&mut self, process: i64) -> Result<(), Trap> {
        let key = ProcessKey::from(KeyData::from_ffi(process as u64));

        self.processes
            .remove(key)
            .ok_or_else(|| Trap::new(format!("Invalid process handle {}.", process)))?;

        Ok(())
    }

    pub fn set_tick_rate(&mut self, ticks_per_sec: f64) {
        info!("New Tick Rate: {}", ticks_per_sec);
        self.tick_rate = Duration::from_secs_f64(ticks_per_sec.recip());
    }

    pub fn print_message(&mut self, ptr: i32, len: i32) -> Result<(), Trap> {
        let message = read_str(&mut self.memory, ptr, len)?;
        info!(target: "Auto Splitter", "{}", message);
        Ok(())
    }

    pub fn read_into_buf(
        &mut self,
        process: i64,
        address: i64,
        buf_ptr: i32,
        buf_len: i32,
    ) -> Result<i32, Trap> {
        let key = ProcessKey::from(KeyData::from_ffi(process as u64));

        let process = self
            .processes
            .get(key)
            .ok_or_else(|| Trap::new(format!("Invalid process handle {}.", process)))?;

        // TODO: is unwrapping fine here?
        let pid: ProcessHandle = (*process).try_into().unwrap();
        let res = pid.copy_address(
            address as usize,
            get_bytes(&mut self.memory, buf_ptr, buf_len)?,
        );

        Ok(res.is_ok() as i32)
    }

    pub fn set_variable(
        &mut self,
        key_ptr: i32,
        key_len: i32,
        value_ptr: i32,
        value_len: i32,
    ) -> Result<(), Trap> {
        let key = read_str(&mut self.memory, key_ptr, key_len)?.to_owned();
        let value = read_str(&mut self.memory, value_ptr, value_len)?.to_owned();
        self.timer.set_variable(&key, &value);
        Ok(())
    }

    pub fn set_game_time(&mut self, secs: i64, nanos: i32) -> Result<(), Trap> {
        if nanos >= 1_000_000_000 {
            Err(Trap::new("more than a one second of nanoseconds"))
        } else {
            self.timer
                .set_game_time(Duration::new(secs as u64, nanos as u32));
            Ok(())
        }
    }

    pub fn pause_game_time(&mut self) {
        self.timer.pause_game_time()
    }

    pub fn resume_game_time(&mut self) {
        self.timer.resume_game_time()
    }
}
