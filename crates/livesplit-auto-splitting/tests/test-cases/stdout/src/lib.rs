#![no_std]

use core::panic::PanicInfo;
use aslib::print_message;

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[no_mangle]
pub extern "C" fn configure() {
    print_message("Printing from the auto splitter");
}
