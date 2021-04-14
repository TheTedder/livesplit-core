use std::time::Duration;

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimerState {
    NotRunning = 0,
    Running = 1,
    Finished = 2,
}

pub trait Timer: 'static {
    fn timer_state(&self) -> TimerState;
    fn start(&mut self);
    fn split(&mut self);
    fn reset(&mut self);
    //fn get_game_time(&self) -> Duration;
    fn set_game_time(&mut self, time: Duration);
    fn pause_game_time(&mut self);
    fn resume_game_time(&mut self);
    fn is_game_time_paused(&self) -> bool;
    // fn set_variable(&mut self, key: &str, value: &str);
}
