use std::io::{Read, Seek, SeekFrom, Write};
use std::num::NonZeroU16;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use rterm::attach::CONNECT_TIMEOUT_SECONDS;
use rterm::driver;
use rterm::kitty::Keyboard;
use rterm::place::Place;
use rterm::watchdog::Watchdog;

enum Event {
    Bytes(Vec<u8>),
    Trace(String),
    Timeout,
}

pub struct Session {
    generation: u32,
    rows: u16,
    cols: u16,
    parser: vt100::Parser,
    keyboard: Keyboard,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    log_stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    watchdog: Arc<Watchdog>,
    announced: bool,
    saw_exit: bool,
}

pub enum Pump {
    Output,
    Trace(String),
    Timeout,
    Exit(Option<i32>),
}

impl Session {
    pub fn start(place: &Place, generation: u32) -> Result<Self, String> {
        let rows = NonZeroU16::new(24).expect("rows");
        let cols = NonZeroU16::new(80).expect("cols");
        let system = portable_pty::native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| err.to_string())?;
        let log = if place.is_local() {
            None
        } else {
            Some(driver::verbose_log_path(place.id, generation))
        };
        if let Some(path) = &log {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::File::create(path);
        }
        let log_arg = log.as_ref().map(|path| path.to_string_lossy().into_owned());
        let mut launch = driver::launch(place, log_arg.as_deref());
        if !place.is_local() {
            launch.executable = driver::ssh_program();
        }
        let mut cmd = CommandBuilder::new(launch.executable);
        cmd.args(launch.arguments);
        if let Some(dir) = launch.current_directory {
            cmd.cwd(dir);
        }
        cmd.env("TERM", "xterm-256color");
        // herdr refuses to attach when it inherits a parent pane. rterm is
        // often started inside herdr, so the child must be a fresh client.
        clear_herdr_identity(&mut cmd);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|err| err.to_string())?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| err.to_string())?;
        let writer = pair.master.take_writer().map_err(|err| err.to_string())?;
        let (tx, rx) = mpsc::channel();
        let timeout_tx = tx.clone();
        let watchdog = Arc::new(Watchdog::new(
            Duration::from_secs(CONNECT_TIMEOUT_SECONDS),
            move || {
                let _ = timeout_tx.send(Event::Timeout);
            },
        ));
        watchdog.pet();
        let stop = Arc::new(AtomicBool::new(false));
        let log_stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        let read_stop = Arc::clone(&stop);
        let read_tx = tx.clone();
        threads.push(std::thread::spawn(move || {
            read_pty(reader, read_tx, read_stop)
        }));
        if let Some(path) = log {
            let log_stop = Arc::clone(&log_stop);
            let hard_stop = Arc::clone(&stop);
            let log_tx = tx;
            let log_dog = Arc::clone(&watchdog);
            threads.push(std::thread::spawn(move || {
                tail_log(&path, log_tx, log_dog, log_stop, hard_stop);
            }));
        }
        Ok(Self {
            generation,
            rows: 24,
            cols: 80,
            parser: vt100::Parser::new(rows, cols, 1_000),
            keyboard: Keyboard::new(),
            writer: Some(writer),
            master: Some(pair.master),
            child: Some(child),
            events: rx,
            stop,
            log_stop,
            threads,
            watchdog,
            announced: false,
            saw_exit: false,
        })
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn kitty_flags(&self) -> u16 {
        self.keyboard.flags()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }

    pub fn fit(&mut self, width: f32, height: f32, char_w: f32, char_h: f32) {
        if char_w <= 0.0 || char_h <= 0.0 {
            return;
        }
        let cols = ((width / char_w).floor() as u16).max(1);
        let rows = ((height / char_h).floor() as u16).max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        if let Some(master) = self.master.as_ref() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let (Some(rows), Some(cols)) = (NonZeroU16::new(rows), NonZeroU16::new(cols)) {
            self.parser.set_size(rows, cols);
        }
    }

    pub fn pump(&mut self) -> Vec<Pump> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Bytes(bytes) => {
                    if !bytes.is_empty() {
                        let first = !self.announced;
                        let (rest, reply) = self.keyboard.feed(&bytes);
                        if !reply.is_empty() {
                            self.write(&reply);
                        }
                        self.parser.process(&rest);
                        if first {
                            self.announced = true;
                            self.watchdog.cancel();
                            self.log_stop.store(true, Ordering::Relaxed);
                        }
                        out.push(Pump::Output);
                    }
                }
                Event::Trace(line) => out.push(Pump::Trace(line)),
                Event::Timeout => out.push(Pump::Timeout),
            }
        }
        if !self.saw_exit
            && let Some(child) = self.child.as_mut()
        {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.saw_exit = true;
                    out.push(Pump::Exit(exit_code(&status)));
                }
                Ok(None) => {}
                Err(_) => {
                    self.saw_exit = true;
                    out.push(Pump::Exit(Some(1)));
                }
            }
        }
        out
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.watchdog.cancel();
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        self.writer.take();
        self.master.take();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn read_pty(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Event>, stop: Arc<AtomicBool>) {
    let mut buf = [0u8; 8192];
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.send(Event::Bytes(buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        }
    }
}

fn tail_log(
    path: &std::path::Path,
    tx: mpsc::Sender<Event>,
    watchdog: Arc<Watchdog>,
    log_stop: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    let mut offset = 0u64;
    let mut leftover = String::new();
    while !stop.load(Ordering::Relaxed) && !log_stop.load(Ordering::Relaxed) {
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
        };
        let len = file.metadata().map(|meta| meta.len()).unwrap_or(offset);
        if len < offset {
            offset = 0;
        }
        if len > offset && file.seek(SeekFrom::Start(offset)).is_ok() {
            let mut buf = vec![0u8; (len - offset) as usize];
            if file.read_exact(&mut buf).is_ok() {
                offset = len;
                leftover.push_str(&String::from_utf8_lossy(&buf));
                while let Some(index) = leftover.find('\n') {
                    let line = leftover[..index].trim_end_matches('\r').to_string();
                    leftover.drain(..=index);
                    watchdog.pet();
                    if tx.send(Event::Trace(line)).is_err() {
                        return;
                    }
                }
            }
        }
        drop(file);
        std::thread::sleep(Duration::from_millis(30));
    }
    let _ = std::fs::remove_file(path);
}

fn clear_herdr_identity(cmd: &mut portable_pty::CommandBuilder) {
    let keys: Vec<String> = std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| key.starts_with("HERDR_"))
        .collect();
    for key in keys {
        cmd.env_remove(key);
    }
}

pub fn exit_code(status: &portable_pty::ExitStatus) -> Option<i32> {
    if status.signal().is_some() {
        None
    } else {
        Some(status.exit_code() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_does_not_inherit_herdr_identity() {
        let key = "HERDR_RTERM_TEST_IDENTITY";
        unsafe { std::env::set_var(key, "1") };
        let mut cmd = portable_pty::CommandBuilder::new("true");
        clear_herdr_identity(&mut cmd);
        assert!(cmd.get_env(key).is_none());
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn exit_status_mapping() {
        assert_eq!(
            exit_code(&portable_pty::ExitStatus::with_exit_code(0)),
            Some(0)
        );
        assert_eq!(
            exit_code(&portable_pty::ExitStatus::with_exit_code(7)),
            Some(7)
        );
        assert_eq!(
            exit_code(&portable_pty::ExitStatus::with_signal("TERM")),
            None
        );
    }
}
