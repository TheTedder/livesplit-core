use crate::timer::Timer;

use log::info;
use read_process_memory::{CopyAddress, ProcessHandle};
use slotmap::{Key, KeyData, SlotMap};
use std::{convert::TryInto, error::Error, str, time::Duration};
use sysinfo::{self, Pid, ProcessExt, System, SystemExt, AsU32};
use wasmtime::{Memory, Store, Trap};


fn trap_from_err(e: impl Error + Send + Sync + 'static) -> Trap {
    Trap::new(anyhow::Error::from(e).to_string())
}

impl<T> Environment<T> {


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

    pub fn timer_state(&self) -> u32 {
        self.timer.timer_state() as u32
    }

    pub fn attach(&mut self, ptr: u32, len: u32) -> Result<u64, Trap> {
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
        Ok(key.data().as_ffi())
    }

    pub fn detach(&mut self, process: u64) -> Result<(), Trap> {
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

    pub fn print_message(&mut self, ptr: u32, len: u32) -> Result<(), Trap> {
        let message = read_str(&mut self.memory, ptr, len)?;
        info!(target: "Auto Splitter", "{}", message);
        Ok(())
    }

    pub fn read_into_buf(
        &mut self,
        process: u64,
        address: u64,
        buf_ptr: u32,
        buf_len: u32,
    ) -> Result<u32, Trap> {
        let key = ProcessKey::from(KeyData::from_ffi(process as u64));

        let process = self
            .processes
            .get(key)
            .ok_or_else(|| Trap::new(format!("Invalid process handle {}.", process)))?;

        let pid: ProcessHandle = process.as_u32().try_into().map_err(|_| Trap::new(format!("invalid PID: {}", process)))?;
        let res = pid.copy_address(
            address as usize,
            get_bytes(&mut self.memory, buf_ptr, buf_len)?,
        );

        Ok(res.is_ok() as u32)
    }

    pub fn set_game_time(&mut self, secs: u64, nanos: u32) -> Result<(), Trap> {
        if nanos >= 1_000_000_000 {
            Err(Trap::new("more than a one second of nanoseconds"))
        } else {
            self.timer.set_game_time(Duration::new(secs, nanos));
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
