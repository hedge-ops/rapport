//! Production filesystem backed by the operating system.
//!
//! This module owns UTF-8 path conversion and real file mutations.

use crate::{FileSystem, Utf8Path, Utf8PathBuf};
use std::io::{self, Write as _};

#[derive(Debug, Default, Clone, Copy)]
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool {
        path.as_ref().is_dir()
    }

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool {
        path.as_ref().is_file()
    }

    fn canonicalize(&self, path: impl AsRef<Utf8Path>) -> io::Result<Utf8PathBuf> {
        let path = std::fs::canonicalize(path.as_ref())?;
        Utf8PathBuf::from_path_buf(path).map_err(|path| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("canonical path is not UTF-8: {}", path.display()),
            )
        })
    }

    fn git_file_mode(&self, path: impl AsRef<Utf8Path>) -> io::Result<u32> {
        let metadata = std::fs::metadata(path.as_ref())?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a regular file", path.as_ref()),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if metadata.permissions().mode() & 0o111 != 0 {
                return Ok(0o100_755);
            }
        }
        Ok(0o100_644)
    }

    fn read_to_string(&self, path: impl AsRef<Utf8Path>) -> io::Result<String> {
        std::fs::read_to_string(path.as_ref())
    }

    fn read_bytes(&self, path: impl AsRef<Utf8Path>) -> io::Result<Vec<u8>> {
        std::fs::read(path.as_ref())
    }

    fn read_dir(&self, path: impl AsRef<Utf8Path>) -> io::Result<Vec<Utf8PathBuf>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path.as_ref())? {
            let entry = entry?;
            let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not a UTF-8 path", path.display()),
                )
            })?;
            entries.push(path);
        }
        entries.sort();
        Ok(entries)
    }

    fn create_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
        std::fs::create_dir_all(path.as_ref())
    }

    fn write_string(
        &mut self,
        path: impl AsRef<Utf8Path>,
        contents: impl AsRef<str>,
    ) -> io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path.as_ref(), contents.as_ref())
    }

    fn append_line(&mut self, path: impl AsRef<Utf8Path>, line: impl AsRef<str>) -> io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        writeln!(file, "{}", line.as_ref())
    }

    fn remove_file(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
        std::fs::remove_file(path.as_ref())
    }

    fn rename(&mut self, from: impl AsRef<Utf8Path>, to: impl AsRef<Utf8Path>) -> io::Result<()> {
        std::fs::rename(from.as_ref(), to.as_ref())
    }

    fn remove_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
        std::fs::remove_dir_all(path.as_ref())
    }
}
