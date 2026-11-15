//! The `wasi:filesystem` host: what every encoding judges once a call walks
//! from a descriptor, so preview1 adapts to it today and a guest built for
//! preview2 inherits it. A call on an open file needs none of it and asks the
//! Open File itself.

use std::sync::Arc;

use kobako_runtime::cred::Cred;
use kobako_runtime::filesystem::Iattr;
use kobako_runtime::security::WalkGuard;
use kobako_runtime::vfs::{self, File};
use kobako_runtime::Errno;

/// How many times a call is made before a guest is answered EAGAIN, as cap-std
/// makes `openat2` four times: any move in the process answers a `..` walking
/// past it so, and nothing bounds how often one lands.
const MAX_ATTEMPTS: usize = 4;

/// What `call` answers once no move landed during its walk, made again at most
/// `MAX_ATTEMPTS` times in all, as cap-std retries `openat2` on EAGAIN.
pub(crate) fn retry_eagain<T>(mut call: impl FnMut() -> Result<T, Errno>) -> Result<T, Errno> {
    let mut answer = call();
    for _ in 1..MAX_ATTEMPTS {
        if !matches!(answer, Err(Errno::WouldBlock)) {
            break;
        }
        answer = call();
    }
    answer
}

/// Where the descriptor `file` finds stands, for a path to be walked from,
/// once `guard` answered for a directory there. The descriptor is found when
/// the VFS has taken the name, as Linux's `path_init` takes it. What is no
/// directory the walk itself refuses, and a move the guard meets is one a
/// walk meets, so `retry_eagain` makes the call again.
pub(crate) fn start_at<'a>(
    guard: &'a WalkGuard,
    file: impl Fn() -> Result<&'a File, Errno> + 'a,
) -> impl vfs::Start + 'a {
    move || {
        let file = file()?;
        if file.is_directory() {
            guard.check(file.path())?;
        }
        Ok(file.path().clone())
    }
}

/// Set `file`'s times, once `guard` answered for where it stands.
pub(crate) fn set_times(
    cred: &Arc<Cred>,
    guard: &WalkGuard,
    file: &File,
    times: &Iattr,
) -> Result<(), Errno> {
    retry_eagain(|| {
        guard.check(file.path())?;
        vfs::do_utimes_fd(cred, file.path(), times)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `retry_eagain` answers when the first `moves` calls each had a move
    /// land in them, and how many calls it made.
    fn retry_after_moves(moves: usize) -> (Result<(), Errno>, usize) {
        let mut made = 0;
        let answer = retry_eagain(|| {
            made += 1;
            match made <= moves {
                true => Err(Errno::WouldBlock),
                false => Ok(()),
            }
        });
        (answer, made)
    }

    // A move lands during a walk only between two of its steps, which no guest
    // call can time, so the retry itself witnesses it.
    // @behavior RT-256
    #[test]
    fn a_path_call_a_move_landed_in_is_made_again_four_times_at_most() {
        assert_eq!(
            [
                retry_after_moves(0),
                retry_after_moves(3),
                retry_after_moves(4)
            ],
            [(Ok(()), 1), (Ok(()), 4), (Err(Errno::WouldBlock), 4),],
            "a path_* call answered EAGAIN must be made again until it answers otherwise, four \
             times in all at most, as cap-std makes openat2"
        );
    }
}
