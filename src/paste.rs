use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use uuid::Uuid;

use crate::driver::{self, SSH_EXECUTABLE};
use crate::place::Place;
use crate::uuidv7::uuid_upper;

pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePasteError {
    TooLarge,
    WriteFailed,
    UploadFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    CommandV,
    ControlV,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Ignore,
    Deliver,
}

pub fn plan(has_image: bool, has_text: bool, is_local: bool, gesture: Gesture) -> Plan {
    if !has_image || has_text {
        return Plan::Ignore;
    }
    match gesture {
        Gesture::CommandV => Plan::Deliver,
        Gesture::ControlV => {
            if is_local {
                Plan::Ignore
            } else {
                Plan::Deliver
            }
        }
    }
}

pub fn host_path(id: Uuid) -> String {
    format!("/tmp/rterm-paste-{}.png", uuid_upper(id))
}

pub type UploadHook<'a> = &'a mut dyn FnMut(&[u8], &[String]) -> Result<(), ImagePasteError>;

pub fn deliver(
    png: &[u8],
    place: &Place,
    paste_id: Uuid,
    on_progress: &mut dyn FnMut(usize, usize),
    upload: Option<UploadHook<'_>>,
) -> Result<String, ImagePasteError> {
    if png.len() > MAX_BYTES {
        return Err(ImagePasteError::TooLarge);
    }
    let path = host_path(paste_id);
    on_progress(0, png.len());
    if place.is_local() {
        atomic_write(Path::new(&path), png).map_err(|_| ImagePasteError::WriteFailed)?;
        on_progress(png.len(), png.len());
        return Ok(path);
    }
    if !Path::new(&driver::control_path(place)).exists() {
        return Err(ImagePasteError::UploadFailed);
    }
    let arguments = driver::upload_arguments(place, &path);
    if let Some(upload) = upload {
        upload(png, &arguments)?;
        on_progress(png.len(), png.len());
    } else {
        ssh_cat(png, &arguments, on_progress)?;
    }
    Ok(path)
}

pub fn write_chunks(
    data: &[u8],
    writer: &mut dyn Write,
    on_progress: &mut dyn FnMut(usize, usize),
) -> std::io::Result<()> {
    let mut sent = 0;
    while sent < data.len() {
        let end = (sent + CHUNK_BYTES).min(data.len());
        writer.write_all(&data[sent..end])?;
        sent = end;
        on_progress(sent, data.len());
    }
    Ok(())
}

fn ssh_cat(
    data: &[u8],
    arguments: &[String],
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<(), ImagePasteError> {
    let program = if Path::new(SSH_EXECUTABLE).is_file() {
        SSH_EXECUTABLE
    } else {
        "ssh"
    };
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ImagePasteError::UploadFailed)?;
    let mut stdin = child.stdin.take().ok_or(ImagePasteError::UploadFailed)?;
    write_chunks(data, &mut stdin, on_progress).map_err(|_| ImagePasteError::UploadFailed)?;
    drop(stdin);
    let status = child.wait().map_err(|_| ImagePasteError::UploadFailed)?;
    if status.success() {
        Ok(())
    } else {
        Err(ImagePasteError::UploadFailed)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("png.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::uuidv7::{self, version_of};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn plan_ignores_text_even_when_image_present() {
        assert_eq!(plan(true, true, false, Gesture::CommandV), Plan::Ignore);
        assert_eq!(plan(true, true, false, Gesture::ControlV), Plan::Ignore);
    }

    #[test]
    fn plan_delivers_image_only_command_v_on_both_places() {
        assert_eq!(plan(true, false, true, Gesture::CommandV), Plan::Deliver);
        assert_eq!(plan(true, false, false, Gesture::CommandV), Plan::Deliver);
    }

    #[test]
    fn plan_control_v_only_on_remote() {
        assert_eq!(plan(true, false, true, Gesture::ControlV), Plan::Ignore);
        assert_eq!(plan(true, false, false, Gesture::ControlV), Plan::Deliver);
    }

    #[test]
    fn host_path_uses_paste_id() {
        let id = Uuid::new_v4();
        assert_eq!(
            host_path(id),
            format!("/tmp/rterm-paste-{}.png", uuid_upper(id))
        );
    }

    #[test]
    fn default_paste_id_is_version_7() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let id = uuidv7::generate_at(now);
        assert_eq!(version_of(id), 7);
        assert_eq!(id.as_bytes()[8] >> 6, 0b10);
        let bytes = id.as_bytes();
        let millis = (u64::from(bytes[0]) << 40)
            | (u64::from(bytes[1]) << 32)
            | (u64::from(bytes[2]) << 24)
            | (u64::from(bytes[3]) << 16)
            | (u64::from(bytes[4]) << 8)
            | u64::from(bytes[5]);
        assert_eq!(millis, 1_700_000_000_000);
    }

    #[test]
    fn local_deliver_writes_png() {
        let id = Uuid::new_v4();
        let png = b"png-bytes";
        let place = Place::new("ada", "", Backend::Herdr, None);
        let mut progress = Vec::new();
        let result = deliver(
            png,
            &place,
            id,
            &mut |sent, total| progress.push((sent, total)),
            None,
        );
        let path = result.unwrap();
        assert_eq!(path, host_path(id));
        assert_eq!(std::fs::read(&path).unwrap(), png);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            progress.iter().map(|item| item.0).collect::<Vec<_>>(),
            [0, png.len()]
        );
        assert_eq!(
            progress.iter().map(|item| item.1).collect::<Vec<_>>(),
            [png.len(), png.len()]
        );
    }

    #[test]
    fn rejects_oversized_png() {
        let png = vec![0u8; MAX_BYTES + 1];
        let place = Place::new("ada", "", Backend::Herdr, None);
        assert_eq!(
            deliver(&png, &place, Uuid::new_v4(), &mut |_, _| {}, None),
            Err(ImagePasteError::TooLarge)
        );
    }

    #[test]
    fn remote_deliver_fails_without_control_socket() {
        let place = Place::new("ada", "remote.example", Backend::Herdr, None);
        let png = b"png";
        let mut called = false;
        let result = deliver(
            png,
            &place,
            Uuid::new_v4(),
            &mut |_, _| {},
            Some(&mut |_, _| {
                called = true;
                Ok(())
            }),
        );
        assert_eq!(result, Err(ImagePasteError::UploadFailed));
        assert!(!called);
    }

    #[test]
    fn remote_deliver_uploads_when_socket_exists() {
        let place = Place::new("ada", "remote.example", Backend::Herdr, None);
        let socket = driver::control_path(&place);
        driver::prepare_control_socket_directory();
        std::fs::write(&socket, []).unwrap();
        let id = Uuid::new_v4();
        let png = b"png";
        let mut uploaded = None;
        let result = deliver(
            png,
            &place,
            id,
            &mut |_, _| {},
            Some(&mut |data, arguments| {
                uploaded = Some((data.to_vec(), arguments.to_vec()));
                Ok(())
            }),
        );
        let _ = std::fs::remove_file(&socket);
        assert_eq!(result.unwrap(), host_path(id));
        let (data, arguments) = uploaded.unwrap();
        assert_eq!(data, png);
        assert_eq!(arguments, driver::upload_arguments(&place, &host_path(id)));
    }

    #[test]
    fn write_chunks_reports_each_chunk() {
        let data = vec![0x61u8; CHUNK_BYTES + 10];
        let path = std::env::temp_dir().join(format!("rterm-chunk-{}", Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).unwrap();
        let mut progress = Vec::new();
        write_chunks(&data, &mut file, &mut |sent, total| {
            progress.push((sent, total))
        })
        .unwrap();
        drop(file);
        assert_eq!(
            progress.iter().map(|item| item.0).collect::<Vec<_>>(),
            [CHUNK_BYTES, data.len()]
        );
        assert_eq!(
            progress.iter().map(|item| item.1).collect::<Vec<_>>(),
            [data.len(), data.len()]
        );
        assert_eq!(std::fs::read(&path).unwrap(), data);
        let _ = std::fs::remove_file(path);
    }
}
