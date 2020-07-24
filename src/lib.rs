/// https://www.man7.org/linux/man-pages/man2/fcntl.2.html

use libc::{
    __errno_location,
    fcntl as libc_fcntl,
};
use std::{
    convert::TryInto,
    os::unix::io::AsRawFd,
};

// re-exports
pub use libc::{
    c_int,
    c_short,
    flock,
};


/// Allowed types for the ``arg` parameter for the `fcntl` syscall.
pub enum FcntlArg {
    Flock(flock),
}


/// Allowed commands (`cmd` parameter) for the `fcntl` syscall.
#[derive(Debug)]
pub enum FcntlCmd {
    /// F_SETLK,
    SetLock,
    /// F_SETLKW
    SetLockWait,
    /// F_GETLK
    GetLock,
}


/// Error type which functions of this crate will return.
#[derive(Debug, PartialEq)]
pub enum FcntlError {
    /// The requested FcntlCmd is not yet handled by our implementation
    CommandNotImplemented,
    /// The syscall returned the respective error (which may be `None`, if the errno lookup fails)
    Errno(Option<c_int>),
    /// An `crate`-internal error occured. If you get this error variant, please report this as a bug!
    Internal,
    /// The enum variant of `arg` does not match the expected variant for the requested `cmd`. No operation was
    /// performed.
    InvalidArgForCmd,
}


/// Defines which types of lock can be set onto files.
#[derive(Debug)]
pub enum FcntlLockType {
    /// Wrapper for `F_RDLCK`
    Read,
    /// Wrapper for `F_WRLCK`
    Write,
}


/// This trait is used to defined functions wich directly operate on `flock`.
pub trait FlockOperations {
    /// Sets `l_type` to the given value. This is mostly intended for internal use. Where possible, it is recommended to
    /// use other methods which alter `l_type`.
    fn with_l_type(self, l_type: c_short) -> Self;

    /// Sets the lock type (`l_type`) to the appropriate value for the given `FcntlLockType`, using the builder pattern.
    fn with_locktype(self, locktype: FcntlLockType) -> Self;
}


/// Calls `fcntl` with the given `cmd` and `arg`. On success, the structure passed to the `arg` parameter is returned
/// as returned by the kernel. In case of an error (syscall returned an error, invalid `arg` provided, `cmd` not
/// supported, etc.) an appropriate `Err` is returned.
///
/// Currently supported `cmd`s:
/// - `FcntlCmd::GetLock`
/// - `FcntlCmd::SetLock`
pub fn fcntl<'a, RF>(fd: &'a RF, cmd: FcntlCmd, arg: FcntlArg) -> Result<FcntlArg, FcntlError>
where RF: AsRawFd
{
    let fd = fd.as_raw_fd();
    // different commands require different types of `arg`
    match cmd {
        FcntlCmd::GetLock | FcntlCmd::SetLock /*| FcntlCmd::SetLockWait*/ => {
            match arg {
                FcntlArg::Flock(flock) => {
                    let mut flock = flock;
                    let rv = unsafe { libc_fcntl(fd, cmd.into(), &mut flock) };
                    if rv == 0 {
                        Ok(FcntlArg::Flock(flock))
                    } else {
                        let errno_ptr = unsafe { __errno_location() };
                        let errno = if errno_ptr.is_null() {
                            None
                        } else {
                            // *should* be safe here as we checked against NULL pointer..
                            Some(unsafe {*errno_ptr})
                        };
                        Err(FcntlError::Errno(errno))
                    }
                }
                _ => Err(FcntlError::InvalidArgForCmd),
            }
        }
        FcntlCmd::SetLockWait => Err(FcntlError::CommandNotImplemented)
    }
}


/// Checks whether the given file is locked.
///
/// The caller is responsible that `fd` was opened with the appropriate parameters, as stated by `fcntl 2`:
/// > In order to place a read lock, `fd` must be open for reading.  In order to place a write lock, `fd` must be open
/// for writing.  To place both types of lock, open a file read-write.
pub fn is_file_locked<'a, RF>(fd: &'a RF, flock: Option<flock>) -> Result<bool, FcntlError>
where RF: AsRawFd
{
    let arg = match flock {
        Some(flock) => FcntlArg::Flock(flock),
        None => FcntlArg::Flock(libc::flock {
            l_type: 0,
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        })
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
pub fn lock_file<'a, RF>(fd: &'a RF, flock: Option<flock>, locktype: Option<FcntlLockType>) -> Result<bool, FcntlError>
where RF: AsRawFd
{
    let locktype = locktype.unwrap_or(FcntlLockType::Read);
    let arg = match flock {
        Some(flock) => FcntlArg::Flock(flock.with_locktype(locktype)),
        None => FcntlArg::Flock(libc::flock {
            l_type: locktype.into(),
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        })
    };

    match fcntl(fd, FcntlCmd::SetLock, arg) {
        // Locking was successful
        Ok(FcntlArg::Flock(_result)) => Ok(true),
        // This should not happen, unless we have a bug..
        Ok(_) => Err(FcntlError::Internal),
        // "If a conflicting lock is held by another process, this call returns -1 and sets errno to EACCES or EAGAIN."
        Err(FcntlError::Errno(Some(libc::EACCES))) | Err(FcntlError::Errno(Some(libc::EAGAIN))) => Ok(false),
        // Everything else is also an error
        Err(err) => Err(err),
    }
}


/// Locks the given file (using `FcntlCmd::SetLock`). If `flock` is `None` all parameters of the flock structure (
/// `l_whence`, `l_start`, `l_len`, `l_pid`) will be set to 0. `flock.l_type` will be set to `libc::F_UNLCK` regardless
/// of its original value.
pub fn unlock_file<'a, RF>(fd: &'a RF, flock: Option<flock>) -> Result<bool, FcntlError>
where RF: AsRawFd
{

    let arg = match flock {
        // unrwap is safe here
        Some(flock) => FcntlArg::Flock(flock.with_l_type(libc::F_UNLCK.try_into().unwrap())),
        None => FcntlArg::Flock(libc::flock {
            // unwrap is safe here
            l_type: libc::F_UNLCK.try_into().unwrap(),
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        })
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
    fn with_l_type(mut self, l_type: c_short) -> Self {
        self.l_type = l_type;
        self
    }

    fn with_locktype(mut self, locktype: FcntlLockType) -> Self {
        self.l_type = locktype.into();
        self
    }
}


impl From<FcntlCmd> for c_int {
    fn from(cmd: FcntlCmd) -> c_int {
        match cmd {
            FcntlCmd::GetLock => libc::F_GETLK,
            FcntlCmd::SetLock => libc::F_SETLK,
            FcntlCmd::SetLockWait => libc::F_SETLKW,
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
    use std::fs::{OpenOptions, remove_file};

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
            let file = OpenOptions::new().write(true).create(true).open(LOCK_FILE_NAME).unwrap();
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
            let file = OpenOptions::new().write(true).create(true).open(LOCK_FILE_NAME).unwrap();
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
        assert_eq!(result_is_file_locked_before, Ok(false), "Verify that file was unlocked");
        assert_eq!(result_lock_file_read, Ok(true), "Lock file");
        //assert_eq!(result_is_file_locked_after, Ok(true));
        assert_eq!(result_unlock_after, Ok(true), "Unlock file");
    }
}
