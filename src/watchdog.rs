use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct Shared {
    interval: Duration,
    deadline: Mutex<Option<Instant>>,
    stop: Mutex<bool>,
    cvar: Condvar,
    on_fire: Mutex<Box<dyn Fn() + Send>>,
}

pub struct Watchdog {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    pub fn new(interval: Duration, on_fire: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            interval,
            deadline: Mutex::new(None),
            stop: Mutex::new(false),
            cvar: Condvar::new(),
            on_fire: Mutex::new(Box::new(on_fire)),
        });
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::spawn(move || run(thread_shared));
        Self {
            shared,
            thread: Some(thread),
        }
    }

    pub fn pet(&self) {
        if let Ok(mut deadline) = self.shared.deadline.lock() {
            *deadline = Some(Instant::now() + self.shared.interval);
        }
        self.shared.cvar.notify_one();
    }

    pub fn cancel(&self) {
        if let Ok(mut deadline) = self.shared.deadline.lock() {
            *deadline = None;
        }
        self.shared.cvar.notify_one();
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Ok(mut stop) = self.shared.stop.lock() {
            *stop = true;
        }
        self.shared.cvar.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: Arc<Shared>) {
    let mut deadline = shared
        .deadline
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    loop {
        if *shared.stop.lock().unwrap_or_else(|err| err.into_inner()) {
            break;
        }
        let Some(when) = *deadline else {
            deadline = shared
                .cvar
                .wait(deadline)
                .unwrap_or_else(|err| err.into_inner());
            continue;
        };
        let now = Instant::now();
        if now >= when {
            *deadline = None;
            drop(deadline);
            if let Ok(callback) = shared.on_fire.lock() {
                callback();
            }
            deadline = shared
                .deadline
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            continue;
        }
        let (guard, _) = shared
            .cvar
            .wait_timeout(deadline, when.saturating_duration_since(now))
            .unwrap_or_else(|err| err.into_inner());
        deadline = guard;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    #[test]
    fn fires_after_interval() {
        let (tx, rx) = mpsc::channel();
        let dog = Watchdog::new(Duration::from_millis(50), move || {
            let _ = tx.send(());
        });
        dog.pet();
        rx.recv_timeout(Duration::from_millis(400)).unwrap();
    }

    #[test]
    fn pet_delays_fire() {
        let (tx, rx) = mpsc::channel();
        let dog = Watchdog::new(Duration::from_millis(80), move || {
            let _ = tx.send(());
        });
        dog.pet();
        std::thread::sleep(Duration::from_millis(40));
        dog.pet();
        rx.recv_timeout(Duration::from_millis(500)).unwrap();
    }

    #[test]
    fn cancel_prevents_fire() {
        let count = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&count);
        let dog = Watchdog::new(Duration::from_millis(50), move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        dog.pet();
        dog.cancel();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
