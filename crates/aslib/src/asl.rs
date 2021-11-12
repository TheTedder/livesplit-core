use bytemuck::Pod;
use core::{
    mem::{self, MaybeUninit},
    slice,
};

mod sys {
    use super::Address;

    pub const INVALID_PROCESS_HANDLE: i64 = 0x1_FFFF_FFFF;

    extern "C" {
        pub fn start();
        pub fn split();
        pub fn reset();
        pub fn attach(name_ptr: *const u8, name_len: usize) -> i64;
        pub fn detach(process: i64);
        pub fn set_tick_rate(ticks_per_second: f64);
        pub fn print_message(text_ptr: *const u8, text_len: usize);
        pub fn read_into_buf(
            process: i64,
            address: Address,
            buf_ptr: *mut u8,
            buf_len: usize,
        ) -> bool;
        pub fn get_timer_state() -> i32;
        pub fn set_game_time(secs: f64);
        pub fn pause_game_time();
        pub fn resume_game_time();
    }
}

#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Process(i64);

impl Drop for Process {
    fn drop(&mut self) {
        unsafe { sys::detach(self.0) }
    }
}

impl Process {
    pub fn attach(name: &str) -> Option<Self> {
        let id = unsafe { sys::attach(name.as_ptr(), name.len()) };
        if id != sys::INVALID_PROCESS_HANDLE {
            Some(Self(id))
        } else {
            None
        }
    }

    pub fn read_into_buf(&self, address: Address, buf: &mut [u8]) -> Result<(), ()> {
        unsafe {
            if sys::read_into_buf(self.0, address, buf.as_mut_ptr(), buf.len()) {
                Ok(())
            } else {
                Err(())
            }
        }
    }

    pub fn read<T: Pod>(&self, address: Address) -> Result<T, ()> {
        unsafe {
            let mut value = MaybeUninit::<T>::uninit();
            self.read_into_buf(
                address,
                slice::from_raw_parts_mut(value.as_mut_ptr().cast(), mem::size_of::<T>()),
            )?;
            Ok(value.assume_init())
        }
    }

    pub fn read_into_slice<T: Pod>(&self, address: Address, slice: &mut [T]) -> Result<(), ()> {
        self.read_into_buf(address, bytemuck::cast_slice_mut(slice))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct Address(pub u64);

pub fn start() {
    unsafe { sys::start() }
}

pub fn split() {
    unsafe { sys::split() }
}

pub fn reset() {
    unsafe { sys::reset() }
}

pub fn set_tick_rate(ticks_per_second: f64) {
    unsafe { sys::set_tick_rate(ticks_per_second) }
}

pub fn print_message(msg: &str) {
    unsafe { sys::print_message(msg.as_ptr(), msg.len()) }
}

pub fn set_game_time(secs: f64) {
    unsafe { sys::set_game_time(secs) }
}

pub fn pause_game_time() {
    unsafe { sys::pause_game_time() }
}

pub fn resume_game_time() {
    unsafe { sys::resume_game_time() }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimerState {
    NotRunning,
    Running,
    Finished,
}

pub fn timer_state() -> TimerState {
    unsafe {
        match sys::get_timer_state() {
            0 => TimerState::NotRunning,
            1 => TimerState::Running,
            2 => TimerState::Finished,
            _ => core::hint::unreachable_unchecked(),
        }
    }
}
