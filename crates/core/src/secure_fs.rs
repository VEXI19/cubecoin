//! Linux primitives for private, single-owner secret files.

use std::{
    fs::{self, DirBuilder, File},
    io::{self, Write},
    os::{
        fd::FromRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    },
    path::Path,
};

use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecureFsError {
    #[error("path does not exist")]
    NotFound,
    #[error("path already exists")]
    AlreadyExists,
    #[error("unsafe private directory")]
    UnsafeDirectory,
    #[error("unsafe private file")]
    UnsafeFile,
    #[error("private file exceeds permitted length")]
    TooLarge,
    #[error("I/O error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: io::Error,
    },
}

/// Ensures the leaf directory exists, is owned by the current user, and is exactly `0700`.
pub fn ensure_private_directory(path: &Path) -> Result<(), SecureFsError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_private_directory_metadata(&metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut builder = DirBuilder::new();
            builder.recursive(true).mode(0o700);
            match builder.create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(SecureFsError::Io {
                        action: "create private directory",
                        source,
                    });
                }
            }
            let metadata = fs::symlink_metadata(path).map_err(|source| SecureFsError::Io {
                action: "inspect private directory",
                source,
            })?;
            validate_private_directory_metadata(&metadata)
        }
        Err(source) => Err(SecureFsError::Io {
            action: "inspect private directory",
            source,
        }),
    }
}

/// Opens and validates a private regular file without following a final symlink.
pub fn open_private_file(path: &Path, maximum_length: u64) -> Result<File, SecureFsError> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    let path = std::ffi::CString::new(bytes).map_err(|_| SecureFsError::UnsafeFile)?;
    // SAFETY: `path` is a NUL-terminated CString. The returned descriptor is owned on success.
    let descriptor = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY,
        )
    };
    if descriptor < 0 {
        let source = io::Error::last_os_error();
        return if source.kind() == io::ErrorKind::NotFound {
            Err(SecureFsError::NotFound)
        } else {
            Err(SecureFsError::Io {
                action: "open private file",
                source,
            })
        };
    }
    // SAFETY: `descriptor` was returned by `open` and is transferred to File exactly once.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(|source| SecureFsError::Io {
        action: "inspect private file",
        source,
    })?;
    validate_private_file_metadata(&metadata, maximum_length)?;
    Ok(file)
}

/// Atomically writes a `0600` file and never replaces an existing destination.
pub fn write_private_file_noclobber(path: &Path, contents: &[u8]) -> Result<(), SecureFsError> {
    let parent = path.parent().ok_or(SecureFsError::UnsafeDirectory)?;
    ensure_private_directory(parent)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| SecureFsError::Io {
        action: "create temporary private file",
        source,
    })?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| SecureFsError::Io {
            action: "set private file permissions",
            source,
        })?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| SecureFsError::Io {
            action: "write private file",
            source,
        })?;
    temporary.persist_noclobber(path).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            SecureFsError::AlreadyExists
        } else {
            SecureFsError::Io {
                action: "persist private file",
                source: error.error,
            }
        }
    })?;
    drop(open_private_file(path, contents.len() as u64)?);
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| SecureFsError::Io {
            action: "sync private directory",
            source,
        })?;
    Ok(())
}

pub fn validate_private_directory(path: &Path) -> Result<(), SecureFsError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| SecureFsError::Io {
        action: "inspect private directory",
        source,
    })?;
    validate_private_directory_metadata(&metadata)
}

fn validate_private_directory_metadata(metadata: &fs::Metadata) -> Result<(), SecureFsError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(SecureFsError::UnsafeDirectory);
    }
    Ok(())
}

fn validate_private_file_metadata(
    metadata: &fs::Metadata,
    maximum_length: u64,
) -> Result<(), SecureFsError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(SecureFsError::UnsafeFile);
    }
    if metadata.len() > maximum_length {
        return Err(SecureFsError::TooLarge);
    }
    Ok(())
}

fn current_uid() -> u32 {
    // SAFETY: `geteuid` has no preconditions and no side effects.
    unsafe { libc::geteuid() }
}
