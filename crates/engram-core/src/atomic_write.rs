//! Crash-safe replacement for small local metadata files.
//!
//! The write is atomic for readers once replacement succeeds: they observe the
//! old complete file or the new complete file. `sync_all` improves durability,
//! but sudden-power-loss guarantees still depend on the OS/filesystem.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const TEMP_MARKER: &str = ".engram-atomic-tmp-";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub fn replace(path: &Path, payload: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("metadata destination has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temp = create_temp(parent, path)?;
    let result = write_and_replace(&temp, path, payload);
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn create_temp(parent: &Path, destination: &Path) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .context("metadata destination has no filename")?;
    for _ in 0..128 {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{name}{TEMP_MARKER}{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                if let Err(error) = preserve_permissions(destination, &file) {
                    let _ = fs::remove_file(&candidate);
                    return Err(error);
                }
                return Ok(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("could not allocate a unique metadata temporary file")
}

fn write_and_replace(temp: &Path, destination: &Path, payload: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).open(temp)?;
    file.write_all(payload)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    replace_existing(temp, destination)?;
    sync_parent(destination.parent().unwrap_or_else(|| Path::new(".")))
}

#[cfg(unix)]
fn replace_existing(temp: &Path, destination: &Path) -> Result<()> {
    fs::rename(temp, destination)
        .with_context(|| format!("replacing {}", destination.display()))?;
    Ok(())
}

#[cfg(windows)]
fn replace_existing(temp: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn replace_existing(temp: &Path, destination: &Path) -> Result<()> {
    fs::rename(temp, destination)
        .with_context(|| format!("replacing {}", destination.display()))?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<()> {
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn sync_parent(_parent: &Path) -> Result<()> {
    // Windows replacement uses MOVEFILE_WRITE_THROUGH. Directory handles do
    // not provide a portable equivalent of Unix directory fsync.
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_parent(_parent: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn preserve_permissions(destination: &Path, temp: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(destination) {
        temp.set_permissions(fs::Permissions::from_mode(metadata.permissions().mode()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn preserve_permissions(_destination: &Path, _temp: &File) -> Result<()> {
    Ok(())
}
