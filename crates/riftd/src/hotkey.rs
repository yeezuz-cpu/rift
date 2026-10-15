use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey, VK_F9};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

pub fn spawn(flag: Arc<AtomicBool>) {
    std::thread::spawn(move || unsafe {
        if RegisterHotKey(None, 1, MOD_NOREPEAT, VK_F9.0 as u32).is_err() {
            eprintln!("could not register F9, is another app using it?");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                flag.store(true, Ordering::Relaxed);
            }
        }
    });
}
