//! Making what a change needs in a writable Layer from what a lower Layer
//! shows, as overlayfs's `copy_up.c` copies a file or a directory up.

use super::super::super::{FileType, FilesystemError, OpenOptions};
use super::super::path::OverlayPath;
use super::{copy_whole, entry, Chain, OverlayFs};

impl OverlayFs {
    /// The directory `chain` reaches in Layer `into`, made there along
    /// with each directory above it that so far shows only from another
    /// Layer.
    pub(super) fn upper_dir(
        &self,
        chain: &mut Chain,
        into: usize,
    ) -> Result<OverlayPath, FilesystemError> {
        self.copy_up(chain, chain.steps.len() - 1, into)
    }

    /// The directory at `step` of `chain` in Layer `into`, copied up with
    /// each directory above it, as overlayfs's `ovl_copy_up` copies up a
    /// dentry's parents first.
    fn copy_up(
        &self,
        chain: &mut Chain,
        step: usize,
        into: usize,
    ) -> Result<OverlayPath, FilesystemError> {
        if let Some(dir) = &chain.steps[step].level[into] {
            return Ok(dir.clone());
        }
        if step == 0 {
            return Err(FilesystemError::CrossesDevices);
        }
        let parent = self.copy_up(chain, step - 1, into)?;
        let name = chain.steps[step].name.clone();
        let dir = match entry(&parent, &name)? {
            Some(found) if found.file_type == FileType::Directory => parent.open_dir(&name)?,
            Some(found) if found.file_type == FileType::Symlink => {
                return Err(FilesystemError::CrossesDevices)
            }
            Some(_) => return Err(FilesystemError::NotADirectory),
            None => {
                parent.create_dir(&name)?;
                parent.open_dir(&name)?
            }
        };
        chain.steps[step].level[into] = Some(dir.clone());
        self.inodes.inode_update(&chain.path_to(step), into, &dir);
        Ok(dir)
    }

    /// Copy the file `name` from Layer `from` into Layer `into` whole.
    /// Layer `into` held nothing there, so a copy that fails part way is
    /// taken away again rather than left to hide the whole file. A link
    /// is not copied up, as no Layer is handed one to hold.
    pub(super) fn copy_up_file(
        &self,
        chain: &mut Chain,
        name: &str,
        from: usize,
        into: usize,
    ) -> Result<(), FilesystemError> {
        let source_dir = chain.level()[from]
            .clone()
            .ok_or(FilesystemError::NotFound)?;
        if entry(&source_dir, name)?.map(|found| found.file_type) == Some(FileType::Symlink) {
            return Err(FilesystemError::CrossesDevices);
        }
        let target_dir = self.upper_dir(chain, into)?;
        let mut source = source_dir.open(
            name,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        let mut copy = target_dir.open(
            name,
            &OpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )?;
        let copied = copy_whole(source.as_mut(), copy.as_mut()).and(copy.close());
        if copied.is_err() {
            let _ = target_dir.remove_file(name);
            return copied;
        }
        self.readers
            .inode_update(&chain.path_of(name), &target_dir, name)
    }
}
