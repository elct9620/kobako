//! `Overlay`: a Filesystem whose writes land in an upper Filesystem over a
//! lower one it only ever reads.
//!
//! A file in the lower Filesystem opened for writing is first copied up,
//! and an entry removed from it is hidden rather than touched, so the
//! lower Filesystem stays as it was handed over.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use super::path::{ancestors, is_beneath};
use super::{DirEntry, File, Filesystem, FilesystemError, Kind, Metadata, OpenOptions};

/// An upper Filesystem taking every write over a lower one it only reads.
pub struct Overlay {
    upper: Arc<dyn Filesystem>,
    lower: Arc<dyn Filesystem>,
    /// Lower paths removed through the Overlay, each hiding what lies
    /// beneath it too.
    hidden: Mutex<BTreeSet<String>>,
}

impl Overlay {
    pub fn new(upper: Arc<dyn Filesystem>, lower: Arc<dyn Filesystem>) -> Self {
        Self {
            upper,
            lower,
            hidden: Mutex::new(BTreeSet::new()),
        }
    }

    fn hidden(&self) -> MutexGuard<'_, BTreeSet<String>> {
        self.hidden
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the lower Filesystem's entry at `path` still shows.
    fn shows_lower(&self, path: &str) -> bool {
        let hidden = self.hidden();
        !hidden.contains(path) && !ancestors(path).any(|dir| hidden.contains(dir))
    }

    fn lower_metadata(&self, path: &str) -> Option<Metadata> {
        self.shows_lower(path)
            .then(|| self.lower.metadata(path).ok())
            .flatten()
    }

    fn hide(&self, path: &str) {
        self.hidden().insert(path.to_string());
    }

    /// Create in the upper Filesystem each directory above `path` that so
    /// far shows only from the lower one.
    fn copy_up_directories(&self, path: &str) -> Result<(), FilesystemError> {
        for dir in ancestors(path) {
            match self.upper.metadata(dir) {
                Ok(_) => continue,
                Err(FilesystemError::NotFound) => {}
                Err(err) => return Err(err),
            }
            match self.lower_metadata(dir) {
                Some(Metadata {
                    kind: Kind::Directory,
                    ..
                }) => self.upper.create_dir(dir)?,
                Some(_) => return Err(FilesystemError::NotADirectory),
                None => return Err(FilesystemError::NotFound),
            }
        }
        Ok(())
    }

    /// Copy the lower file at `path` into the upper Filesystem whole.
    fn copy_up_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.copy_up_directories(path)?;
        let mut source = self.lower.open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        let mut target = self.upper.open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )?;
        let mut buf = vec![0; 64 * 1024];
        let mut offset = 0;
        loop {
            let read = source.read_at(&mut buf, offset)?;
            if read == 0 {
                return Ok(());
            }
            target.write_at(&buf[..read], offset)?;
            offset += read as u64;
        }
    }

    fn in_upper(&self, path: &str) -> Result<Option<Metadata>, FilesystemError> {
        match self.upper.metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(FilesystemError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl Filesystem for Overlay {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        let writes = options.write || options.truncate;
        if self.in_upper(path)?.is_none() {
            match self.lower_metadata(path) {
                Some(_) if options.create_new => return Err(FilesystemError::AlreadyExists),
                Some(Metadata {
                    kind: Kind::Directory,
                    ..
                }) => return Err(FilesystemError::IsADirectory),
                Some(_) if writes => self.copy_up_file(path)?,
                Some(_) => return self.lower.open(path, options),
                None if options.create || options.create_new => self.copy_up_directories(path)?,
                None => return Err(FilesystemError::NotFound),
            }
        }
        self.upper.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        match self.in_upper(path)? {
            Some(metadata) => Ok(metadata),
            None => self.lower_metadata(path).ok_or(FilesystemError::NotFound),
        }
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        let mut entries = match self.in_upper(path)? {
            Some(_) => self.upper.read_dir(path)?,
            None => Vec::new(),
        };
        if let Some(metadata) = self.lower_metadata(path) {
            if metadata.kind != Kind::Directory {
                return Err(FilesystemError::NotADirectory);
            }
            let prefix = if path.is_empty() {
                String::new()
            } else {
                format!("{path}/")
            };
            for entry in self.lower.read_dir(path)? {
                let full = format!("{prefix}{}", entry.name);
                if self.shows_lower(&full) && !entries.iter().any(|seen| seen.name == entry.name) {
                    entries.push(entry);
                }
            }
        } else if self.in_upper(path)?.is_none() {
            return Err(FilesystemError::NotFound);
        }
        Ok(entries)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        if self.metadata(path).is_ok() {
            return Err(FilesystemError::AlreadyExists);
        }
        self.copy_up_directories(path)?;
        self.upper.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        match self.metadata(path)? {
            Metadata {
                kind: Kind::Directory,
                ..
            } => {}
            _ => return Err(FilesystemError::NotADirectory),
        }
        if !self.read_dir(path)?.is_empty() {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        if self.in_upper(path)?.is_some() {
            self.upper.remove_dir(path)?;
        }
        if self.lower_metadata(path).is_some() {
            self.hide(path);
        }
        Ok(())
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        if self.metadata(path)?.kind == Kind::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        if self.in_upper(path)?.is_some() {
            self.upper.remove_file(path)?;
        }
        if self.lower_metadata(path).is_some() {
            self.hide(path);
        }
        Ok(())
    }

    /// A directory that shows from the lower Filesystem cannot move, since
    /// moving it would mean copying up everything beneath it.
    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError> {
        let moving = self.metadata(from)?;
        if from == to {
            return Ok(());
        }
        if is_beneath(to, from) {
            return Err(FilesystemError::InvalidInput);
        }
        let from_lower = self.lower_metadata(from).is_some();
        match moving.kind {
            Kind::Directory if from_lower => return Err(FilesystemError::Unsupported),
            Kind::File if self.in_upper(from)?.is_none() => self.copy_up_file(from)?,
            _ => {}
        }
        self.copy_up_directories(to)?;
        self.upper.rename(from, to)?;
        if from_lower {
            self.hide(from);
        }
        Ok(())
    }

    /// A fresh Overlay when either layer starts each Invocation fresh, so
    /// what was hidden is shown again along with it.
    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
        let upper = self.upper.for_invocation();
        let lower = self.lower.for_invocation();
        if upper.is_none() && lower.is_none() {
            return None;
        }
        Some(Arc::new(Self::new(
            upper.unwrap_or_else(|| self.upper.clone()),
            lower.unwrap_or_else(|| self.lower.clone()),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::super::memory::Memory;
    use super::super::temporary::Temporary;
    use super::*;

    fn read(fs: &dyn Filesystem, path: &str) -> Vec<u8> {
        let mut file = fs
            .open(
                path,
                &OpenOptions {
                    read: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the file must open");
        let mut buf = vec![0; 64];
        let read = file.read_at(&mut buf, 0).expect("the file must read");
        buf.truncate(read);
        buf
    }

    fn overwrite(fs: &dyn Filesystem, path: &str, contents: &[u8]) {
        fs.open(
            path,
            &OpenOptions {
                write: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )
        .expect("the file must open for writing")
        .write_at(contents, 0)
        .expect("the write must land");
    }

    /// An Overlay over a lower Memory holding `dir/file` = "lower".
    fn layered(upper: Arc<dyn Filesystem>) -> (Overlay, Arc<Memory>) {
        let lower: Arc<Memory> = Arc::new([("dir/file", "lower")].into_iter().collect());
        (Overlay::new(upper, lower.clone()), lower)
    }

    // @behavior FS-008
    #[test]
    fn an_overlay_reads_through_to_its_lower_filesystem() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        assert_eq!(
            read(&overlay, "dir/file"),
            b"lower",
            "a lower file read through Overlay::open must read back the lower content"
        );
    }

    // @behavior FS-009
    #[test]
    fn an_overlay_writes_into_its_upper_filesystem() {
        let upper = Arc::new(Memory::new());
        let (overlay, _) = layered(upper.clone());
        overwrite(&overlay, "dir/file", b"upper");
        assert_eq!(
            read(upper.as_ref(), "dir/file"),
            b"upper",
            "a lower file written through Overlay::open must land in the upper Filesystem"
        );
    }

    // @behavior FS-010
    #[test]
    fn an_overlay_leaves_its_lower_filesystem_as_it_was() {
        let (overlay, lower) = layered(Arc::new(Memory::new()));
        overwrite(&overlay, "dir/file", b"upper");
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            read(lower.as_ref(), "dir/file"),
            b"lower",
            "a lower file written and removed through an Overlay must stay as it was below it"
        );
    }

    // @behavior FS-011
    #[test]
    fn a_file_removed_through_an_overlay_is_gone_from_its_view() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            overlay.metadata("dir/file"),
            Err(FilesystemError::NotFound),
            "a lower file removed through Overlay::remove_file must no longer be found there"
        );
    }

    // @behavior FS-012
    #[test]
    fn an_overlay_over_a_temporary_starts_every_invocation_from_its_lower_filesystem() {
        let (overlay, _) = layered(Arc::new(Temporary::new()));
        overwrite(&overlay, "dir/file", b"upper");
        let next = overlay
            .for_invocation()
            .expect("an Overlay over a Temporary must start each Invocation afresh");
        assert_eq!(
            read(next.as_ref(), "dir/file"),
            b"lower",
            "Overlay::for_invocation over a Temporary must hand the next Invocation the lower content"
        );
    }

    #[test]
    fn an_overlay_lists_both_layers_without_what_it_hid() {
        let upper = Arc::new(Memory::new());
        let (overlay, _) = layered(upper);
        overlay
            .open(
                "dir/new",
                &OpenOptions {
                    write: true,
                    create: true,
                    ..OpenOptions::default()
                },
            )
            .expect("a new file must be created");
        let mut names: Vec<String> = overlay
            .read_dir("dir")
            .expect("the directory must list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        names.sort();
        assert_eq!(names, ["file", "new"]);
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        let names: Vec<String> = overlay
            .read_dir("dir")
            .expect("the directory must list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["new"]);
    }
}
