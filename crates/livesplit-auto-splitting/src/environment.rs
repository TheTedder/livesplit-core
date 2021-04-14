use crate::{process::Process, timer::Timer};

use slotmap::{Key, KeyData, SlotMap};
use std::{collections::HashMap, error::Error, str, time::Duration};
use wasmtime::{Memory, Trap};

slotmap::new_key_type! {
    struct ProcessKey;
}

pub struct Environment<T> {
    pub memory: Option<Memory>,
    processes: SlotMap<ProcessKey, Process>,
    pub tick_rate: Duration,
    // pub process: Option<Process>,
    pub variable_changes: HashMap<String, String>,
    timer: T,
}

impl<T: Timer> Environment<T> {
    pub fn new(timer: T) -> Self {
        Self {
            memory: None,
            processes: SlotMap::with_key(),
            variable_changes: HashMap::new(),
            tick_rate: Duration::from_secs(1) / 60,
            timer,
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

    pub fn attach(&mut self, ptr: i32, len: i32) -> Result<i64, Trap> {
        let process_name = read_str(&mut self.memory, ptr, len)?;
        let key = if let Ok(p) = Process::with_name(process_name) {
            self.processes.insert(p)
        } else {
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

    // pub fn get_val<T>(
    //     &self,
    //     pointer_path_id: i32,
    //     current: i32,
    //     convert: impl FnOnce(&PointerValue) -> Option<T>,
    // ) -> Result<T, Trap> {
    //     let pointer_path = self
    //         .pointer_paths
    //         .get(pointer_path_id as u32 as usize)
    //         .ok_or_else(|| Trap::new("Specified invalid pointer path"))?;

    //     let value = if current != 0 {
    //         &pointer_path.current
    //     } else {
    //         &pointer_path.old
    //     };

    //     convert(value).ok_or_else(|| Trap::new("The types did not match"))
    // }

    // pub fn scan_signature(&mut self, ptr: i32, len: i32) -> Result<i64, Trap> {
    //     // TODO: Don't trap
    //     if let Some(process) = &self.process {
    //         let signature = read_str(&mut self.memory, ptr, len)?;
    //         let address = process.scan_signature(signature).map_err(trap_from_err)?;
    //         return Ok(address.unwrap_or(0) as i64);
    //     }

    //     Ok(0)
    // }

    pub fn set_tick_rate(&mut self, ticks_per_sec: f64) {
        log::info!("New Tick Rate: {}", ticks_per_sec);
        self.tick_rate = Duration::from_secs_f64(1.0 / ticks_per_sec);
    }

    pub fn print_message(&mut self, ptr: i32, len: i32) -> Result<(), Trap> {
        let message = read_str(&mut self.memory, ptr, len)?;
        log::info!(target: "Auto Splitter", "{}", message);
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

        let res = process.read_buf(
            address as u64,
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
        self.variable_changes.insert(key, value);
        Ok(())
    }

    pub fn set_game_time(&mut self, secs: i64, nanos: i64) -> Result<(), Trap> {
        self.timer
            .set_game_time(Duration::new(secs as u64, nanos as u32));
        Ok(())
    }

    // pub fn update_values(&mut self, just_connected: bool) -> anyhow::Result<()> {
    //     let process = self
    //         .process
    //         .as_mut()
    //         .expect("The process should be connected at this point");

    //     for pointer_path in &mut self.pointer_paths {
    //         let mut address = if !pointer_path.module_name.is_empty() {
    //             process.module_address(&pointer_path.module_name)?
    //         } else {
    //             0
    //         };
    //         let mut offsets = pointer_path.offsets.iter().cloned().peekable();
    //         if process.is_64bit() {
    //             while let Some(offset) = offsets.next() {
    //                 address = (address as Offset).wrapping_add(offset) as u64;
    //                 if offsets.peek().is_some() {
    //                     address = process.read(address)?;
    //                 }
    //             }
    //         } else {
    //             while let Some(offset) = offsets.next() {
    //                 address = (address as i32).wrapping_add(offset as i32) as u64;
    //                 if offsets.peek().is_some() {
    //                     address = process.read::<u32>(address)? as u64;
    //                 }
    //             }
    //         }
    //         match &mut pointer_path.old {
    //             PointerValue::U8(v) => *v = process.read(address)?,
    //             PointerValue::U16(v) => *v = process.read(address)?,
    //             PointerValue::U32(v) => *v = process.read(address)?,
    //             PointerValue::U64(v) => *v = process.read(address)?,
    //             PointerValue::I8(v) => *v = process.read(address)?,
    //             PointerValue::I16(v) => *v = process.read(address)?,
    //             PointerValue::I32(v) => *v = process.read(address)?,
    //             PointerValue::I64(v) => *v = process.read(address)?,
    //             PointerValue::F32(v) => *v = process.read(address)?,
    //             PointerValue::F64(v) => *v = process.read(address)?,
    //             PointerValue::String(_) => todo!(),
    //         }
    //     }

    //     if just_connected {
    //         for pointer_path in &mut self.pointer_paths {
    //             pointer_path.current.clone_from(&pointer_path.old);
    //         }
    //     } else {
    //         for pointer_path in &mut self.pointer_paths {
    //             mem::swap(&mut pointer_path.current, &mut pointer_path.old);
    //         }
    //     }

    //     Ok(())
    // }
}
