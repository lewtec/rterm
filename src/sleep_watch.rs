#[cfg(target_os = "linux")]
use std::io::{BufRead, BufReader};
use std::process::Child;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

pub struct SleepWatch {
    rx: Receiver<()>,
    stop: Arc<AtomicBool>,
    child: Option<Child>,
    thread: Option<JoinHandle<()>>,
    #[cfg(target_os = "macos")]
    runloop: Arc<std::sync::atomic::AtomicPtr<std::ffi::c_void>>,
}

impl SleepWatch {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg(target_os = "linux")]
        let watch = linux::spawn(tx, rx, stop);
        #[cfg(target_os = "macos")]
        let watch = macos::spawn(tx, rx, stop);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let watch = {
            drop(tx);
            Self {
                rx,
                stop,
                child: None,
                thread: None,
            }
        };
        watch
    }

    pub fn poll(&self) -> bool {
        let mut hit = false;
        while self.rx.try_recv().is_ok() {
            hit = true;
        }
        hit
    }
}

impl Drop for SleepWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        #[cfg(target_os = "macos")]
        {
            let ptr = self.runloop.load(Ordering::SeqCst);
            if !ptr.is_null() {
                unsafe { macos::CFRunLoopStop(ptr) };
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub fn spawn(tx: mpsc::Sender<()>, rx: Receiver<()>, stop: Arc<AtomicBool>) -> SleepWatch {
        let mut command = Command::new("dbus-monitor");
        command
            .args([
                "--system",
                "type='signal',interface='org.freedesktop.login1.Manager',member='PrepareForSleep'",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut child) = command.spawn() else {
            return SleepWatch {
                rx,
                stop,
                child: None,
                thread: None,
            };
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            return SleepWatch {
                rx,
                stop,
                child: None,
                thread: None,
            };
        };
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut prepare = false;
            for line in reader.lines() {
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(line) = line else { break };
                if line.contains("member=PrepareForSleep") {
                    prepare = true;
                }
                if prepare && line.contains("boolean true") {
                    let _ = tx.send(());
                    prepare = false;
                } else if prepare && line.contains("boolean false") {
                    prepare = false;
                }
            }
        });
        SleepWatch {
            rx,
            stop,
            child: Some(child),
            thread: Some(thread),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::c_void;
    use std::sync::atomic::AtomicPtr;
    use std::sync::atomic::AtomicU32;

    const CAN_SLEEP: u32 = 0xE000_0270;
    const WILL_SLEEP: u32 = 0xE000_0280;
    const WILL_POWER: u32 = 0xE000_0250;

    struct Hook {
        tx: mpsc::Sender<()>,
        port: AtomicU32,
    }

    pub fn spawn(tx: mpsc::Sender<()>, rx: Receiver<()>, stop: Arc<AtomicBool>) -> SleepWatch {
        let runloop = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let thread_loop = Arc::clone(&runloop);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let hook = Box::new(Hook {
                tx,
                port: AtomicU32::new(0),
            });
            let hook_ptr = &*hook as *const Hook as *mut c_void;
            let mut notify = std::ptr::null_mut();
            let mut notifier = 0u32;
            let port =
                unsafe { IORegisterForSystemPower(hook_ptr, &mut notify, callback, &mut notifier) };
            if port == 0 || notify.is_null() {
                return;
            }
            hook.port.store(port, Ordering::SeqCst);
            unsafe {
                let source = IONotificationPortGetRunLoopSource(notify);
                let mode = kCFRunLoopDefaultMode;
                let current = CFRunLoopGetCurrent();
                thread_loop.store(current, Ordering::SeqCst);
                CFRunLoopAddSource(current, source, mode);
                if !thread_stop.load(Ordering::Relaxed) {
                    CFRunLoopRun();
                }
                IODeregisterForSystemPower(&mut notifier);
                IONotificationPortDestroy(notify);
            }
        });
        SleepWatch {
            rx,
            stop,
            child: None,
            thread: Some(thread),
            runloop,
        }
    }

    extern "C" fn callback(
        refcon: *mut c_void,
        _service: u32,
        message_type: u32,
        argument: *mut c_void,
    ) {
        if refcon.is_null() {
            return;
        }
        let hook = unsafe { &*(refcon as *const Hook) };
        if message_type == CAN_SLEEP || message_type == WILL_SLEEP || message_type == WILL_POWER {
            unsafe {
                IOAllowPowerChange(hook.port.load(Ordering::SeqCst), argument as isize);
            }
        }
        if message_type == WILL_SLEEP || message_type == WILL_POWER {
            let _ = hook.tx.send(());
        }
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            the_port_ref: *mut *mut c_void,
            callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void),
            notifier: *mut u32,
        ) -> u32;
        fn IOAllowPowerChange(kernel_port: u32, notification_id: isize) -> i32;
        fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
        fn IONotificationPortDestroy(notify: *mut c_void);
        fn IONotificationPortGetRunLoopSource(notify: *mut c_void) -> *mut c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
        pub fn CFRunLoopStop(rl: *mut c_void);
        fn CFRunLoopGetCurrent() -> *mut c_void;
        static kCFRunLoopDefaultMode: *const c_void;
    }
}
