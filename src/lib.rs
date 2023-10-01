#![doc(html_root_url = "https://docs.rs/fcntl/0.1.0")]
//! Wrapper around [fcntl (2)](https://www.man7.org/linux/man-pages/man2/fcntl.2.html) and convenience methods to make
//! interacting with it easier. Currently only supports commands related to Advisory record locking.
//
//
// TODO: Instead of exposing `libc::flock` we should implement our own flock which implements
//     - `From<flock>`
//     - `Into<flock>`
//     - Methods from `FlockOperations` without the need for the extra trait
//     - Other common traits (`Eq`, `PartialEq`, `Ord`, `PartialOrd`, `Hash`, `Debug`, `Display`, `Default`)

use libc::fcntl as libc_fcntl;
use std::{
    convert::TryInto,
    error::Error,
    fmt::{self, Display},
    os::unix::io::AsRawFd,
};

// re-exports
pub use libc::{c_int, c_short, flock};

/// Allowed types for the `arg` parameter for the `fcntl` syscall.
#[derive(Copy, Clone)]
pub enum FcntlArg {
    Flock(flock),
}

/// Allowed commands (`cmd` parameter) for the `fcntl` syscall.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FcntlCmd {
    /// F_SETLK,
    SetLock,
    /// F_SETLKW
    SetLockWait,
    /// F_GETLK
    GetLock,
    /// F_OFD_SETLKW
    OpenFileDescriptorSetLockWait,
}

/// Error type which functions of this crate will return.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FcntlError {
    /// The requested FcntlCmd is not yet handled by our implementation
    CommandNotImplemented(FcntlCmd),
    /// The syscall returned the respective error (which may be `None`, if the errno lookup fails)
    Errno(c_int, Option<c_int>),
    /// An `crate`-internal error occured. If you get this error variant, please report this as a bug!
    Internal,
    /// The enum variant of `arg` does not match the expected variant for the requested `cmd`. No operation was
    /// performed.
    InvalidArgForCmd,
}

/// Defines which types of lock can be set onto files.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FcntlLockType {
    /// Wrapper for `F_RDLCK`
    Read,
    /// Wrapper for `F_WRLCK`
    Write,
}

/// This trait is used to define functions wich directly operate on `flock`.
/// ```rust
/// use libc::flock;
/// use fcntl::{FcntlLockType, FlockOperations};
///
/// let flock = flock::default().with_locktype(FcntlLockType::Write);
/// ```
pub trait FlockOperations {
    /// Since we don't have control over `libc::flock` we cannot `impl Default for libc::flock`
    fn default() -> Self;

    /// Sets `l_type` to the given value using the builder pattern. This is mostly intended for internal use. Where
    /// possible, it is recommended to use other methods which alter `l_type` (e.g. `with_locktype`).
    fn with_l_type(self, l_type: c_short) -> Self;

    /// Sets the lock type (`l_type`) to the appropriate value for the given `FcntlLockType`, using the builder pattern.
    fn with_locktype(self, locktype: FcntlLockType) -> Self;
}

/// Calls `fcntl` with the given `cmd` and `arg`. On success, the structure passed to the `arg` parameter is returned
/// as returned by the kernel.
/// **Note**: Where possible convenience wrappers (such as `is_file_locked`, `lock_file`, etc.) should be used as they
/// correctly interpret possible return values of the syscall.
///
/// Currently supported `cmd`s:
/// - `FcntlCmd::GetLock`
/// - `FcntlCmd::SetLock`
///
/// # Errors
///
/// In case of an error (syscall returned an error, invalid `arg` provided, `cmd` not supported, etc.) an appropriate
/// value is returned.
pub fn fcntl<'a, RF>(fd: &'a RF, cmd: FcntlCmd, arg: FcntlArg) -> Result<FcntlArg, FcntlError>
where
    RF: AsRawFd,
{
    let fd = fd.as_raw_fd();
    // different commands require different types of `arg`
    match cmd {
        FcntlCmd::GetLock
        | FcntlCmd::SetLock
        | FcntlCmd::SetLockWait
        | FcntlCmd::OpenFileDescriptorSetLockWait => {
            match arg {
                FcntlArg::Flock(flock) => {
                    let mut flock = flock;
                    let rv = unsafe { libc_fcntl(fd, cmd.into(), &mut flock) };
                    if rv == 0 {
                        Ok(FcntlArg::Flock(flock))
                    } else {
                        #[cfg(not(target_os = "macos"))]
                        let errno_ptr = unsafe { libc::__errno_location() };
                        #[cfg(target_os = "macos")]
                        let errno_ptr = unsafe { libc::__error() };

                        let errno = if errno_ptr.is_null() {
                            None
                        } else {
                            // *should* be safe here as we checked against NULL pointer..
                            Some(unsafe { *errno_ptr })
                        };
                        Err(FcntlError::Errno(rv, errno))
                    }
                }
                _ => Err(FcntlError::InvalidArgForCmd),
            }
        } // FcntlCmd::SetLockWait => Err(FcntlError::CommandNotImplemented(FcntlCmd::SetLockWait)),
    }
}

/// Checks whether the given file is locked.
///
/// The caller is responsible that `fd` was opened with the appropriate parameters, as stated by
/// [fcntl (2)](https://www.man7.org/linux/man-pages/man2/fcntl.2.html):
/// > In order to place a read lock, `fd` must be open for reading.  In order to place a write lock, `fd` must be open
/// for writing.  To place both types of lock, open a file read-write.
///
/// ```rust
/// use std::fs::OpenOptions;
/// use fcntl::is_file_locked;
/// # let file_name = "README.md";
///
/// let file = OpenOptions::new().read(true).open(file_name).unwrap();
/// match is_file_locked(&file, None) {
///     Ok(true) => println!("File is currently locked"),
///     Ok(false) => println!("File is not locked"),
///     Err(err) => println!("Error: {:?}", err),
/// }
/// ```
pub fn is_file_locked<'a, RF>(fd: &'a RF, flock: Option<flock>) -> Result<bool, FcntlError>
where
    RF: AsRawFd,
{
    let arg = match flock {
        Some(flock) => FcntlArg::Flock(flock),
        None => FcntlArg::Flock(libc::flock::default()),
    };

    match fcntl(fd, FcntlCmd::GetLock, arg) {
        Ok(FcntlArg::Flock(result)) => {
            // We need to convert from c_int into c_short. F_UNLCK is defined with value 2, so this should never panic.
            // To be extra safe, we have a test case for that ;)
            Ok(result.l_type != libc::F_UNLCK.try_into().unwrap())
        }
        Ok(_) => Err(FcntlError::Internal),
        Err(err) => Err(err),
    }
}

/// Locks the given file (using `FcntlCmd::SetLock`). If `flock` is `None` all parameters of the flock structure (
/// `l_whence`, `l_start`, `l_len`, `l_pid`) will be set to 0.  `locktype` controls the `l_type` parameter. When it is
/// `None`, `FcntlLockType::Read` is used. `flock.l_type` will be overwritten in all cases to avoid passing an invalid
/// parameter to the syscall.
///
/// The caller is responsible that `fd` was opened with the appropriate parameters, as stated by `fcntl 2`:
/// > In order to place a read lock, `fd` must be open for reading.  In order to place a write lock, `fd` must be open
/// for writing.  To place both types of lock, open a file read-write.
///
/// ```rust
/// use std::fs::OpenOptions;
/// use fcntl::{FcntlLockType, lock_file};
/// # let file_name = "README.md";
///
/// let file = OpenOptions::new().read(true).write(true).open(file_name).unwrap();
/// match lock_file(&file, None, Some(FcntlLockType::Write)) {
///     Ok(true) => println!("Lock acuired!"),
///     Ok(false) => println!("Could not acquire lock!"),
///     Err(err) => println!("Error: {:?}", err),
/// }
/// ```
pub fn lock_file<'a, RF>(
    fd: &'a RF,
    flock: Option<flock>,
    locktype: Option<FcntlLockType>,
) -> Result<bool, FcntlError>
where
    RF: AsRawFd,
{
    let locktype = locktype.unwrap_or(FcntlLockType::Read);
    let arg = match flock {
        Some(flock) => FcntlArg::Flock(flock.with_locktype(locktype)),
        None => FcntlArg::Flock(libc::flock::default().with_locktype(locktype)),
    };

    match fcntl(fd, FcntlCmd::SetLockWait, arg) {
        // Locking was successful
        Ok(FcntlArg::Flock(_result)) => Ok(true),
        // This should not happen, unless we have a bug..
        Ok(_) => Err(FcntlError::Internal),
        // "If a conflicting lock is held by another process, this call returns -1 and sets errno to EACCES or EAGAIN."
        Err(FcntlError::Errno(_, Some(libc::EACCES)))
        | Err(FcntlError::Errno(_, Some(libc::EAGAIN))) => Ok(false),
        // Everything else is also an error
        Err(err) => Err(err),
    }
}

/// Releases the lock on the given file (using `FcntlCmd::SetLock`). If `flock` is `None` all parameters of the flock
/// structure (`l_whence`, `l_start`, `l_len`, `l_pid`) will be set to 0. `flock.l_type` will be set to `libc::F_UNLCK`
/// regardless of its original value.
///
/// ```rust
/// use std::fs::OpenOptions;
/// use fcntl::unlock_file;
/// # let file_name = "README.md";
///
/// let file = OpenOptions::new().read(true).open(file_name).unwrap();
/// match unlock_file(&file, None) {
///     Ok(true) => println!("Lock successfully released"),
///     Ok(false) => println!("Falied to release lock"),
///     Err(err) => println!("Error: {:?}", err),
/// }
/// ```
pub fn unlock_file<'a, RF>(fd: &'a RF, flock: Option<flock>) -> Result<bool, FcntlError>
where
    RF: AsRawFd,
{
    let arg = match flock {
        // unrwap is safe here
        Some(flock) => FcntlArg::Flock(flock.with_l_type(libc::F_UNLCK.try_into().unwrap())),
        // unwrap is safe here
        None => {
            FcntlArg::Flock(libc::flock::default().with_l_type(libc::F_UNLCK.try_into().unwrap()))
        }
    };

    match fcntl(fd, FcntlCmd::SetLock, arg) {
        // Unlocking was successful
        Ok(FcntlArg::Flock(_result)) => Ok(true),
        // This should not happen, unless we have a bug..
        Ok(_) => Err(FcntlError::Internal),
        // "If a conflicting lock is held by another process, this call returns -1 and sets errno to EACCES or EAGAIN."
        //Err(FcntlError::Errno(Some(libc::EACCES))) | Err(FcntlError::Errno(Some(libc::EAGAIN))) => Ok(false),
        // Everything else is also an error
        Err(err) => Err(err),
    }
}

impl FlockOperations for flock {
    /// Sets all fields to 0.
    fn default() -> Self {
        flock {
            l_type: 0,
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        }
    }

    fn with_l_type(mut self, l_type: c_short) -> Self {
        self.l_type = l_type;
        self
    }

    fn with_locktype(mut self, locktype: FcntlLockType) -> Self {
        self.l_type = locktype.into();
        self
    }
}

impl Display for FcntlError {
    fn fmt(&self, ff: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandNotImplemented(cmd) => {
                write!(ff, "{:?} is not implemented for this operation", cmd)
            }
            Self::Errno(rv, Some(errno)) => write!(
                ff,
                "syscall {rv} returned unknown or unexpected error: {}",
                errno
            ),
            Self::Errno(rv, None) => {
                write!(
                    ff,
                    "syscall returned {rv} error but we could not retrieve errno"
                )
            }
            Self::Internal => write!(
                ff,
                "we encountered an internal error. Please report this as a bug (fcntl)!"
            ),
            Self::InvalidArgForCmd => write!(
                ff,
                "the provided arg parameter is invalid for the requested cmd"
            ),
        }
    }
}

impl Error for FcntlError {}

impl From<FcntlCmd> for c_int {
    fn from(cmd: FcntlCmd) -> c_int {
        match cmd {
            FcntlCmd::GetLock => libc::F_GETLK,
            FcntlCmd::SetLock => libc::F_SETLK,
            FcntlCmd::SetLockWait => libc::F_SETLKW,
            FcntlCmd::OpenFileDescriptorSetLockWait => 38,
        }
    }
}

impl From<FcntlLockType> for c_short {
    fn from(locktype: FcntlLockType) -> c_short {
        match locktype {
            // These should never panic as their values are hardcoded in the kernel with low enough values
            FcntlLockType::Read => libc::F_RDLCK.try_into().unwrap(),
            FcntlLockType::Write => libc::F_WRLCK.try_into().unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{remove_file, OpenOptions};

    const LOCK_FILE_NAME: &str = "./test-work-dir/lock-test";

    #[test]
    fn check_cmd_conversion() {
        let pairs = vec![
            (FcntlCmd::GetLock, libc::F_GETLK),
            (FcntlCmd::SetLock, libc::F_SETLK),
            (FcntlCmd::SetLockWait, libc::F_SETLKW),
        ];

        for (cmd, check) in pairs.into_iter() {
            assert_eq!(libc::c_int::from(cmd), check);
        }
    }

    #[test]
    fn ensure_conversions_dont_panic() {
        let _: libc::c_short = libc::F_UNLCK.try_into().unwrap();
    }

    #[test]
    fn check_file_locking_simple() {
        // nested so that `file` goes out of scope before we delete the file again
        let result = {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(LOCK_FILE_NAME)
                .unwrap();
            is_file_locked(&file, None)
        };

        // cleanup
        let _ = remove_file(LOCK_FILE_NAME);

        // final assertion
        assert_eq!(result, Ok(false));
    }

    #[test]
    fn lock_file_simple() {
        // nested so that `file` goes out of scope before we delete the file again
        // create the file
        {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(LOCK_FILE_NAME)
                .unwrap();
        }
        let file = OpenOptions::new().read(true).open(LOCK_FILE_NAME).unwrap();

        let result_is_file_locked_before = is_file_locked(&file, None);
        let result_lock_file_read = lock_file(&file, None, None);
        // We would need to test this with a separate process...
        //let result_is_file_locked_after = is_file_locked(&file, None);
        let result_unlock_after = unlock_file(&file, None);

        // cleanup
        let _ = remove_file(LOCK_FILE_NAME);

        // final assertions
        assert_eq!(
            result_is_file_locked_before,
            Ok(false),
            "Verify that file was unlocked"
        );
        assert_eq!(result_lock_file_read, Ok(true), "Lock file");
        //assert_eq!(result_is_file_locked_after, Ok(true));
        assert_eq!(result_unlock_after, Ok(true), "Unlock file");
    }
}
