//! > **THIS PLACE IS NOT A PLACE OF HONOR.**
//! >
//! > **NO HIGHLY ESTEEMED DEED IS COMMEMORATED HERE.**
//! >
//! > **NOTHING VALUED IS HERE.**
//!
//! ---------------------------------------------------------------------------
//!
//! this module implements cross platform[^1] file locking.
//! file locking is cursed on unix[^2], so this module is cursed as well.
//!
//! the use-case which this module needs to fulfil is as follows:
//! > multiple `rustdn` instances need to synchronize toolchain updates (caused by, for example, toolchain file updates).
//! > if the toolchain doesn't require an update all instances should acquire a shared lock,
//! > check that the toolchain is up-to-date, use the toolchain, unlock the shared lock.
//! >
//! > if the toolchain requires an update, exactly one instance should acquire an exclusive lock and update the toolchain
//! > (while all others instances are waiting). this is also known as [leader election].
//! >
//! > [leader election]: https://en.wikipedia.org/wiki/Leader_election
//!
//! there are 3 options for file locking:
//! 1. `flock`
//! 2. "open file description locks" (`fcntl(F_OFD_SETLKW)`, ..)
//! 3. "advisory record locking" (`fcntl(F_SETLKW)`, ..)
//!
//! and guess what? they all suck in their unique ways:
//!
//! 1. `flock` does not detect deadlocks and doesn't properly support upgrading locks shared->exclusive, which is required for leader election
//! 2. ofd locks are gnu-only (and thus are not on macos)
//! 3. "advisory record locks" are process-local and can't make exclusive locks on directories[^3]
//!
//! the third option is generally regarded as "completely fucked" because it's semantics are... not great.
//! as already mentioned, they are process-local, meaning that a lock is identified by a `(pid, file)` tuple,
//! which means that you can't synchronize with it in-process (and this complicates testing a huge log!).
//!
//! sadly, it's the only option that even remotely works for our needs.
//!
//! thankfully, for this exact case it's not *that* bad.
//! the reason for that is that we are single-threaded and are only working with a single lock at a time,
//! i.e. we only need to lock one file per `rustdn` execution.
//!
//! with all of that in mind, this module uses `fcntl` via [`rustix`] to implement basic locking with a nice-ish API
//! (if you ignore all the horrors of the semantics).
//!
//! [^1]: i.e. linux, macos, and maybe other unixes. windows is not supported, since nix doesn't support windows.
//! [^2]: i have not checked the state of file locking on windows, since there is no need for that, as per the note above.
//! [^3]: because while you *can* open a directory for reading, you can't open it for writing and exclusive `fcntl` locks require write permissions

use std::{fs::File, ops::Deref, os::fd::AsFd};

use crate::destructure;

// FIXME: this should provide a `cfg(test)` implementation that works only in-process, rather than between processes.
//        (i.e. use `static STATE: Mutex<...>` to manage locks, instead of `fcntl`, so that we can run unit tests)
// FIXME: add `cfg(debug_assertions)` code, which would check that we are not locking the same file multiple times in-process

/// Acquires a shared lock on `file`.
///
/// **N.B.** `file` must be opened for reading.
///
/// This blocks until a shared lock can be acquired.
pub fn lock_shared<F>(file: F) -> rustix::io::Result<Lock<F, Shared>>
where
    F: Deref<Target = File>,
{
    rustix::fs::fcntl_lock(file.as_fd(), rustix::fs::FlockOperation::LockShared)?;

    Ok(Lock { file, mode: Shared })
}

pub struct Shared;
pub struct Exclusive;

/// A lock guard.
///
/// While this type exists, the file is locked.
/// `M` signifies the mode, either [`Shared`] or [`Exclusive`].
///
/// Unlocks the lock on drop.
pub struct Lock<F, M>
where
    F: Deref<Target = File>,
{
    file: F,
    mode: M,
}

impl<F> Lock<F, Shared>
where
    F: Deref<Target = File>,
{
    /// Given a shared lock, try upgrading it to an exclusive one.
    ///
    /// **N.B.**: the underlying `file` must be opened for writing.
    ///
    /// This blocks until all shared locks are released.
    /// If a deadlock occurs because multiple processes are trying to upgrade,
    /// all, but one, get an [`DEADLK`] error.
    ///
    /// [`DEADLK`]: rustix::io::Errno::DEADLK
    ///
    /// On error, the shared lock is released.
    ///
    /// Under the hood this re-locks the file using `fcntl`.
    pub fn upgrade(self) -> rustix::io::Result<Lock<F, Exclusive>> {
        // i'm not sure if it's actually documented or guaranteed anywhere, but from my experiments/experience,
        // if all processes try to upgrade their shared locks to exclusive locks, then all **but one** processes get `EDEADLK`.
        // note however that this, by itself, does not unlock the shared lock they had.
        // so we need to make sure that on fail we unlock the lock we had,
        // to give an opportunity for someone to actually acquire exclusive lock.
        //
        // on the error-path this drops `self`, which unlocks the lock.
        rustix::fs::fcntl_lock(self.file.as_fd(), rustix::fs::FlockOperation::LockExclusive)?;

        // `destructure` does not run the destructor, so this **doesn't** unlock the lock.
        destructure!(Lock { file, mode: _ } = self);
        let mode = Exclusive;

        Ok(Lock { file, mode })
    }
}

// we could have a `impl<F, M> Deref for Lock<F, M>`, but we don't need it,
// because we have a separate empty lock file on which we don't run any operations.
// in a way, we are doing a c-style lock (where lock and data are separate) instead of a
// `Mutex`-like thing, this is somewhat footgunny, but... idk how to make it better.

impl<F, M> Drop for Lock<F, M>
where
    F: Deref<Target = File>,
{
    fn drop(&mut self) {
        _ = rustix::fs::fcntl_lock(self.file.as_fd(), rustix::fs::FlockOperation::Unlock);
    }
}
