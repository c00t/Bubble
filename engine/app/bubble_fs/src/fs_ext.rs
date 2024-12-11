pub use crate::path_ext::*;
use backon::{BlockingRetryable, Retryable};
use bubble_core::tracing::{debug, error, info, trace, warn};
use fs2::FileExt;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Create a symlink at `dst` pointing to `src`, replacing any existing symlink.
///
/// On Windows, this uses the `junction` crate to create a junction point.
/// Note because junctions are used, the source must be a directory.
#[cfg(windows)]
pub fn replace_symlink(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    // If the source is a file, we can't create a junction
    if src.as_ref().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Cannot create a junction for {}: is not a directory",
                src.as_ref().display()
            ),
        ));
    }

    // Remove the existing symlink, if any.
    match junction::delete(dunce::simplified(dst.as_ref())) {
        Ok(()) => match fs_err::remove_dir_all(dst.as_ref()) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    };

    // Replace it with a new symlink.
    junction::create(
        dunce::simplified(src.as_ref()),
        dunce::simplified(dst.as_ref()),
    )
}

/// Create a symlink at `dst` pointing to `src`, replacing any existing symlink if necessary.
#[cfg(unix)]
pub fn replace_symlink(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    // Attempt to create the symlink directly.
    match std::os::unix::fs::symlink(src.as_ref(), dst.as_ref()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            // Create a symlink, using a temporary file to ensure atomicity.
            let temp_dir = tempfile::tempdir_in(dst.as_ref().parent().unwrap())?;
            let temp_file = temp_dir.path().join("link");
            std::os::unix::fs::symlink(src, &temp_file)?;

            // Move the symlink into the target location.
            fs_err::rename(&temp_file, dst.as_ref())?;

            Ok(())
        }
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
pub fn remove_symlink(path: impl AsRef<Path>) -> std::io::Result<()> {
    fs_err::remove_file(path.as_ref())
}

/// Create a symlink at `dst` pointing to `src` on Unix or copy `src` to `dst` on Windows
///
/// This does not replace an existing symlink or file at `dst`.
///
/// This does not fallback to copying on Unix.
///
/// This function should only be used for files. If targeting a directory, use [`replace_symlink`]
/// instead; it will use a junction on Windows, which is more performant.
pub fn symlink_or_copy_file(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        fs_err::copy(src.as_ref(), dst.as_ref())?;
    }
    #[cfg(unix)]
    {
        fs_err::os::unix::fs::symlink(src.as_ref(), dst.as_ref())?;
    }

    Ok(())
}

#[cfg(windows)]
pub fn remove_symlink(path: impl AsRef<Path>) -> std::io::Result<()> {
    match junction::delete(dunce::simplified(path.as_ref())) {
        Ok(()) => match fs_err::remove_dir_all(path.as_ref()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Return a [`NamedTempFile`] in the specified directory.
pub fn tempfile_in(path: &Path) -> std::io::Result<NamedTempFile> {
    tempfile::Builder::new().tempfile_in(path)
}

/// Write `data` to `path` atomically using a temporary file and atomic rename.
#[cfg(feature = "bubble-tasks")]
pub async fn write_atomic(
    path: impl AsRef<Path>,
    data: impl bubble_tasks::buf::IoBuf,
) -> std::io::Result<()> {
    let temp_file = tempfile_in(
        path.as_ref()
            .parent()
            .expect("Write path must have a parent"),
    )?;
    bubble_tasks::fs::write(&temp_file, data).await.0?;
    // Note: `persist` is a instant replace syscall behind, i don't think we should async the syscall
    // using spawn_blocking in bubble-tasks.
    temp_file.persist(&path).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Failed to persist temporary file to {}: {}",
                path.user_display(),
                err.error
            ),
        )
    })?;
    Ok(())
}

/// Write `data` to `path` atomically using a temporary file and atomic rename.
pub fn write_atomic_sync(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> std::io::Result<()> {
    let temp_file = tempfile_in(
        path.as_ref()
            .parent()
            .expect("Write path must have a parent"),
    )?;
    fs_err::write(&temp_file, &data)?;
    temp_file.persist(&path).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Failed to persist temporary file to {}: {}",
                path.user_display(),
                err.error
            ),
        )
    })?;
    Ok(())
}

/// Copy `from` to `to` atomically using a temporary file and atomic rename.
pub fn copy_atomic_sync(from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<()> {
    let temp_file = tempfile_in(to.as_ref().parent().expect("Write path must have a parent"))?;
    fs_err::copy(from.as_ref(), &temp_file)?;
    temp_file.persist(&to).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Failed to persist temporary file to {}: {}",
                to.user_display(),
                err.error
            ),
        )
    })?;
    Ok(())
}

fn backon_file_move() -> backon::ExponentialBuilder {
    // backoff::ExponentialBackoffBuilder::default()
    //     .with_initial_interval(std::time::Duration::from_millis(10))
    //     .with_max_elapsed_time(Some(std::time::Duration::from_secs(10)))
    //     .build()
    backon::ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_millis(10))
        .with_jitter()
        .with_max_times(10)
}

/// Error is the error value in an operation's
/// result.
///
/// Based on the two possible values, the operation
/// may be retried.
#[derive(Debug)]
pub enum BackonError<E> {
    /// Permanent means that it's impossible to execute the operation
    /// successfully. This error is immediately returned from `retry()`.
    Permanent(E),

    /// Transient means that the error is temporary. If the `retry_after` is `None`
    /// the operation should be retried according to the backoff policy, else after
    /// the specified duration. Useful for handling ratelimits like a HTTP 429 response.
    Transient(E),
}

impl<E> BackonError<E> {
    // Creates an permanent error.
    pub fn permanent(err: E) -> Self {
        BackonError::Permanent(err)
    }

    // Creates an transient error which is retried according to the backoff
    // policy.
    pub fn transient(err: E) -> Self {
        BackonError::Transient(err)
    }

    pub fn is_transient(&self) -> bool {
        matches!(self, BackonError::Transient(_))
    }

    pub fn is_permanent(&self) -> bool {
        matches!(self, BackonError::Permanent(_))
    }

    pub fn into_inner(self) -> E {
        match self {
            BackonError::Permanent(err) => err,
            BackonError::Transient(err) => err,
        }
    }
}

impl<E> core::fmt::Display for BackonError<E>
where
    E: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> Result<(), core::fmt::Error> {
        match *self {
            BackonError::Permanent(ref err) | BackonError::Transient(ref err) => err.fmt(f),
        }
    }
}

impl<E> core::error::Error for BackonError<E>
where
    E: core::error::Error,
{
    fn description(&self) -> &str {
        match *self {
            BackonError::Permanent(_) => "permanent error",
            BackonError::Transient(_) => "transient error",
        }
    }

    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match *self {
            BackonError::Permanent(ref err) | BackonError::Transient(ref err) => err.source(),
        }
    }

    fn cause(&self) -> Option<&dyn core::error::Error> {
        self.source()
    }
}

/// Rename a file, retrying (on Windows) if it fails due to transient operating system errors.
#[cfg(feature = "bubble-tasks")]
pub async fn rename_with_retry(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> Result<(), std::io::Error> {
    if cfg!(windows) {
        // On Windows, antivirus software can lock files temporarily, making them inaccessible.
        // This is most common for DLLs, and the common suggestion is to retry the operation with
        // some backoff.
        //
        // See: <https://github.com/astral-sh/uv/issues/1491> & <https://github.com/astral-sh/uv/issues/9531>
        let from = from.as_ref();
        let to = to.as_ref();

        let backoff = backon_file_move();
        (|| async move {
            match fs_err::rename(from, to) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                    warn!(
                        "Retrying rename from {} to {} due to transient error: {}",
                        from.display(),
                        to.display(),
                        err
                    );
                    Err(BackonError::transient(err))
                }
                Err(err) => Err(BackonError::permanent(err)),
            }
        })
        .retry(backoff)
        .sleep(bubble_tasks::BubbleSleeper::new())
        .when(|e| e.is_transient())
        .await
        .map_err(|e| e.into_inner())
    } else {
        // fs_err::tokio::rename(from, to).await
        bubble_tasks::fs::rename(from, to).await
    }
}

/// Rename a file, retrying (on Windows) if it fails due to transient operating system errors, in a synchronous context.
pub fn rename_with_retry_sync(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> Result<(), std::io::Error> {
    if cfg!(windows) {
        // On Windows, antivirus software can lock files temporarily, making them inaccessible.
        // This is most common for DLLs, and the common suggestion is to retry the operation with
        // some backoff.
        //
        // See: <https://github.com/astral-sh/uv/issues/1491> & <https://github.com/astral-sh/uv/issues/9531>
        let from = from.as_ref();
        let to = to.as_ref();

        let backoff = backon_file_move();
        (|| match fs_err::rename(from, to) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                warn!(
                    "Retrying rename from {} to {} due to transient error: {}",
                    from.display(),
                    to.display(),
                    err
                );
                Err(BackonError::transient(err))
            }
            Err(err) => Err(BackonError::permanent(err)),
        })
        .retry(backoff)
        .sleep(backon::DefaultBlockingSleeper::default())
        .when(|e| e.is_transient())
        .call()
        .map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Failed to rename {} to {}: {}",
                    from.display(),
                    to.display(),
                    err
                ),
            )
        })
    } else {
        fs_err::rename(from, to)
    }
}

/// Persist a `NamedTempFile`, retrying (on Windows) if it fails due to transient operating system errors, in a synchronous context.
pub fn persist_with_retry_sync(
    from: NamedTempFile,
    to: impl AsRef<Path>,
) -> Result<(), std::io::Error> {
    if cfg!(windows) {
        // On Windows, antivirus software can lock files temporarily, making them inaccessible.
        // This is most common for DLLs, and the common suggestion is to retry the operation with
        // some backoff.
        //
        // See: <https://github.com/astral-sh/uv/issues/1491> & <https://github.com/astral-sh/uv/issues/9531>
        let to = to.as_ref();

        // the `NamedTempFile` `persist` method consumes `self`, and returns it back inside the Error in case of `PersistError`
        // https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html#method.persist
        // So we will update the `from` optional value in safe and borrow-checker friendly way every retry
        // Allows us to use the NamedTempFile inside a FnMut closure used for backoff::retry
        let mut from = Some(from);

        let backoff = backon_file_move();
        let persisted = (move || {
            if let Some(file) = from.take() {
                file.persist(to).map_err(|err| {
                    let error_message = err.to_string();
                    warn!(
                        "Retrying to persist temporary file to {}: {}",
                        to.display(),
                        error_message
                    );

                    // Set back the NamedTempFile returned back by the Error
                    from = Some(err.file);

                    BackonError::transient(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!(
                            "Failed to persist temporary file to {}: {}",
                            to.display(),
                            error_message
                        ),
                    ))
                })
            } else {
                Err(BackonError::permanent(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Failed to retrieve temporary file while trying to persist to {}",
                        to.display()
                    ),
                )))
            }
        })
        .retry(backoff)
        .sleep(backon::DefaultBlockingSleeper::default())
        .when(|e| e.is_transient())
        .call();

        match persisted {
            Ok(_) => Ok(()),
            Err(err) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{err:?}"),
            )),
        }
    } else {
        fs_err::rename(from, to)
    }
}

/// Iterate over the subdirectories of a directory.
///
/// If the directory does not exist, returns an empty iterator.
pub fn directories(path: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
    path.as_ref()
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err) => {
                warn!("Failed to read entry: {}", err);
                None
            }
        })
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .map(|entry| entry.path())
}

/// Iterate over the symlinks in a directory.
///
/// If the directory does not exist, returns an empty iterator.
pub fn symlinks(path: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
    path.as_ref()
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err) => {
                warn!("Failed to read entry: {}", err);
                None
            }
        })
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|file_type| file_type.is_symlink())
        })
        .map(|entry| entry.path())
}

/// Iterate over the files in a directory.
///
/// If the directory does not exist, returns an empty iterator.
pub fn files(path: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
    path.as_ref()
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err) => {
                warn!("Failed to read entry: {}", err);
                None
            }
        })
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
        .map(|entry| entry.path())
}

/// Returns `true` if a path is a temporary file or directory.
pub fn is_temporary(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .map_or(false, |name| name.starts_with(".tmp"))
}

/// A file lock that is automatically released when dropped.
#[derive(Debug)]
pub struct LockedFile(fs_err::File);

impl LockedFile {
    /// Inner implementation for [`LockedFile::acquire_blocking`] and [`LockedFile::acquire`].
    fn lock_file_blocking(file: fs_err::File, resource: &str) -> Result<Self, std::io::Error> {
        trace!(
            "Checking lock for `{resource}` at `{}`",
            file.path().user_display()
        );
        match file.file().try_lock_exclusive() {
            Ok(()) => {
                debug!("Acquired lock for `{resource}`");
                Ok(Self(file))
            }
            Err(err) => {
                // Log error code and enum kind to help debugging more exotic failures.
                if err.kind() != std::io::ErrorKind::WouldBlock {
                    debug!("Try lock error: {err:?}");
                }
                info!(
                    "Waiting to acquire lock for `{resource}` at `{}`",
                    file.path().user_display(),
                );
                file.file().lock_exclusive().map_err(|err| {
                    // Not an fs_err method, we need to build our own path context
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!(
                            "Could not acquire lock for `{resource}` at `{}`: {}",
                            file.path().user_display(),
                            err
                        ),
                    )
                })?;

                debug!("Acquired lock for `{resource}`");
                Ok(Self(file))
            }
        }
    }

    /// The same as [`LockedFile::acquire`], but for synchronous contexts. Do not use from an async
    /// context, as this can block the runtime while waiting for another process to release the
    /// lock.
    pub fn acquire_blocking(
        path: impl AsRef<Path>,
        resource: impl Display,
    ) -> Result<Self, std::io::Error> {
        let file = fs_err::File::create(path.as_ref())?;
        let resource = resource.to_string();
        Self::lock_file_blocking(file, &resource)
    }

    /// Acquire a cross-process lock for a resource using a file at the provided path.
    #[cfg(feature = "bubble-tasks")]
    pub async fn acquire(
        path: impl AsRef<Path>,
        resource: impl Display,
    ) -> Result<Self, std::io::Error> {
        let file = fs_err::File::create(path.as_ref())?;
        let resource = resource.to_string();
        // tokio::task::spawn_blocking(move || Self::lock_file_blocking(file, &resource)).await?
        bubble_tasks::runtime::spawn_blocking(move || Self::lock_file_blocking(file, &resource))
            .await
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, format!("{err:?}")))?
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        if let Err(err) = self.0.file().unlock() {
            error!(
                "Failed to unlock {}; program may be stuck: {}",
                self.0.path().display(),
                err
            );
        } else {
            debug!("Released lock at `{}`", self.0.path().display());
        }
    }
}

/// An asynchronous reader that reports progress as bytes are read.
#[cfg(feature = "bubble-tasks")]
pub struct ProgressReader<Reader: bubble_tasks::io::AsyncRead + Unpin, Callback: Fn(usize) + Unpin>
{
    reader: Reader,
    callback: Callback,
}

#[cfg(feature = "bubble-tasks")]
impl<Reader: bubble_tasks::io::AsyncRead + Unpin, Callback: Fn(usize) + Unpin>
    ProgressReader<Reader, Callback>
{
    /// Create a new [`ProgressReader`] that wraps another reader.
    pub fn new(reader: Reader, callback: Callback) -> Self {
        Self { reader, callback }
    }
}

#[cfg(feature = "bubble-tasks")]
impl<Reader: bubble_tasks::io::AsyncRead + Unpin, Callback: Fn(usize) + Unpin>
    bubble_tasks::io::AsyncRead for ProgressReader<Reader, Callback>
{
    async fn read<B: bubble_tasks::buf::IoBufMut>(
        &mut self,
        buf: B,
    ) -> bubble_tasks::BufResult<usize, B> {
        self.reader.read(buf).await.map(|n, buf| {
            (self.callback)(n);
            (n, buf)
        })
    }
}

/// Recursively copy a directory and its contents.
pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs_err::create_dir_all(&dst)?;
    for entry in fs_err::read_dir(src.as_ref())? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs_err::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
