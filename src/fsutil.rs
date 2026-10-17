use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::Error;

pub(crate) fn read_to_string(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|err| Error::io(path.display().to_string(), err))
}

pub(crate) fn set_mode(path: &Path, mode: u32) -> Result<(), Error> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| Error::io(path.display().to_string(), err))
}

pub(crate) fn mode_of(path: &Path) -> Result<u32, Error> {
    let mode = fs::metadata(path)
        .map_err(|err| Error::io(path.display().to_string(), err))?
        .permissions()
        .mode();
    Ok(mode & 0o777)
}

pub(crate) fn ensure_dir(path: &Path, mode: u32) -> Result<(), Error> {
    fs::create_dir_all(path).map_err(|err| Error::io(path.display().to_string(), err))?;
    set_mode(path, mode)
}

pub(crate) fn atomic_write(path: &Path, contents: &str, mode_if_new: u32) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|err| Error::io(parent.display().to_string(), err))?;
        }
    }

    let mode = if path.exists() {
        mode_of(path)?
    } else {
        mode_if_new
    };
    let tmp = temp_sibling(path);
    write_temp(&tmp, contents, mode)?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        Error::io(path.display().to_string(), err)
    })?;
    set_mode(path, mode)?;
    Ok(())
}

fn write_temp(tmp: &Path, contents: &str, mode: u32) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(tmp)
        .map_err(|err| Error::io(tmp.display().to_string(), err))?;
    file.write_all(contents.as_bytes())
        .map_err(|err| Error::io(tmp.display().to_string(), err))?;
    file.sync_all()
        .map_err(|err| Error::io(tmp.display().to_string(), err))?;
    set_mode(tmp, mode)
}

fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cmdgen".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{name}.tmp-{}", std::process::id()))
}

pub(crate) fn remove_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::io(path.display().to_string(), err)),
    }
}
