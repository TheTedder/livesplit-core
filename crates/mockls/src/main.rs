use bstr::BStr;
use livesplit_auto_splitting::{Runtime, Timer, TimerState};
use simple_log::log_level::INFO;
use std::{
    ffi::OsStr,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

struct MockTimer<const N: i32> {
    current_split: i32,
    current_state: TimerState,
    game_time_paused: bool,
}

impl<const N: i32> MockTimer<N> {
    pub fn new() -> Self {
        Self {
            current_split: -1,
            current_state: TimerState::NotRunning,
            game_time_paused: false,
        }
    }
}

impl<const N: i32> Timer for MockTimer<N> {
    fn timer_state(&self) -> TimerState {
        self.current_state
    }

    fn start(&mut self) {
        if self.current_split < 0 {
            println!("Timer Started!");
            self.current_split = 0;
            self.current_state = TimerState::Running;
        }
    }

    fn split(&mut self) {
        if self.current_split < N {
            println!("Finished Split {}", self.current_split);
            self.current_split += 1;

            if self.current_split == N {
                self.current_state = TimerState::Finished;
                println!("Time! you missed PB by a second Kappa.");
            }
        }
    }

    fn reset(&mut self) {
        println!("Once again!");
        self.current_split = -1;
        self.current_state = TimerState::NotRunning;
    }

    fn set_game_time(&mut self, time: Duration) {
        println!("Game Time is now: {:?}", time);
    }

    fn pause_game_time(&mut self) {
        self.game_time_paused = true;
        println!("gametime paused")
    }

    fn resume_game_time(&mut self) {
        self.game_time_paused = false;
        println!("gametime unpaused")
    }

    fn is_game_time_paused(&self) -> bool {
        self.game_time_paused
    }
}

fn main() {
    simple_log::console(INFO).unwrap();

    let mut path = PathBuf::from("autosplitter");
    let output = Command::new("cargo")
        .current_dir(&path)
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .unwrap();

    if !output.status.success() {
        let output: &BStr = output.stderr.as_slice().into();
        panic!("{}", output);
    }

    path.push("target");
    path.push("wasm32-unknown-unknown");
    path.push("release");
    let wasm_path = fs::read_dir(path)
        .unwrap()
        .find_map(|e| {
            let path = e.unwrap().path();
            if path.extension() == Some(OsStr::new("wasm")) {
                Some(path)
            } else {
                None
            }
        })
        .unwrap();

    println!("Done building the Auto Splitter");

    let mut runtime = Runtime::new(&fs::read(wasm_path).unwrap(), MockTimer::<20>::new()).unwrap();

    loop {
        runtime.step().unwrap();
        runtime.sleep();
    }

    // loop {
    //     if let Some(result) = runtime.step().unwrap() {
    //         match result {
    //             TimerAction::Start => {
    //                 runtime.set_state(TimerState::Running);
    //                 println!("Timer started!");
    //                 current_split = 0;
    //             }
    //             TimerAction::Split => {
    //                 println!("Finished Split {}", current_split);
    //                 current_split += 1;

    //                 if current_split == splits_count {
    //                     println!("Time! you missed PB by a second Kappa.");
    //                     runtime.set_state(TimerState::Finished);
    //                 }
    //             }
    //             TimerAction::Reset => {
    //                 println!("Once again!");
    //                 current_split = -1;
    //                 runtime.set_state(TimerState::NotRunning);
    //             }
    //         }
    //     }

    //     runtime.sleep();
    // }
}
