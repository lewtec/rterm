use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Run a command and give up after `timeout`. Returns `None` when the process
/// could not start or had to be killed.
pub fn run_bounded(program: &str, arguments: &[String], timeout: Duration) -> Option<Output> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let out_reader = std::thread::spawn(move || read_all(stdout));
    let err_reader = std::thread::spawn(move || read_all(stderr));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = out_reader.join().ok()?;
    let stderr = err_reader.join().ok()?;
    let status = status?;
    Some(Output {
        status,
        stdout,
        stderr,
    })
}

fn read_all(mut pipe: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = pipe.read_to_end(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_output() {
        let output = run_bounded(
            "sh",
            &["-c".into(), "printf 'ok'".into()],
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"ok");
    }

    #[test]
    fn kills_a_hung_process() {
        assert!(
            run_bounded(
                "sh",
                &["-c".into(), "sleep 5".into()],
                Duration::from_millis(150)
            )
            .is_none()
        );
    }
}
