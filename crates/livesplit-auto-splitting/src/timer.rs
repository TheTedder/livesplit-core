// use std::time::Duration;

pub trait Timer: 'static {
    fn start(&mut self);
    fn split(&mut self);
    fn reset(&mut self);
    // fn set_game_time(&mut self, time: Duration);
    // fn set_variable(&mut self, key: &str, value: &str);
}
