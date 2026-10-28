//! Resolving a guest path beneath a directory, one name at a time, and the
//! calls that act on the name it ends at: opening it, making it, removing
//! it, and moving it, as Linux's `namei.c` holds both.
//!
//! Each step hands a Filesystem one name in a directory already held,
//! opened without following a link there, so nothing another writer swaps
//! in on the way leads the walk elsewhere. A link is followed only where
//! the walk itself reads where it points, within the directory the walk
//! started from, as `openat2`'s `RESOLVE_BENEATH` keeps a resolution: one
//! leading out of it, or to an absolute path, is not permitted. A `..` or
//! a `.` taking the walk out of a directory a link led to is refused as a
//! loop, so a guest never learns where a link points. Beneath a Mount with
//! a Policy no link is followed at all, as `RESOLVE_NO_SYMLINKS` refuses
//! one.

use std::collections::VecDeque;
use std::sync::Arc;

use super::dentry::Dentry;
use super::openat2::{vfs_open, OpenHow, Opened};
use super::path::{Identity, Path, Step};
use super::times::mask_file_times;
use super::Error;
use crate::filesystem::{Access, FileType, FilesystemError, Inode, Metadata, OpenOptions, Rename};
use crate::mount::Mount;

use super::{NAME_MAX, PATH_MAX};

/// The most links one walk follows, as Linux's `MAXSYMLINKS`.
const MAX_LINKS: usize = 40;

/// Whether a guest path asks for a directory, ending in `/` or `/.`.
pub(crate) fn names_directory(path: &str) -> bool {
    matches!(path.rsplit('/').next(), Some("" | "."))
}

/// A guest path walked beneath a directory, as Linux's `nameidata` carries
/// one walk from where it starts to the call it ends in: the directories
/// it stands in, the names still to walk, and the last name, empty for
/// the directory itself, left for the call to act on.
pub(crate) struct Nameidata {
    pub(crate) mount: Arc<Mount>,
    /// Each directory from where the walk can be spelled from, the Mount's
    /// root beneath a Policy, down to the one it stands in.
    frames: Vec<Frame>,
    /// How many frames stand above the directory the walk started from,
    /// which no `..` climbs into.
    above: usize,
    pending: VecDeque<Part>,
    follows_links: bool,
    links: usize,
    /// Whether each directory stepped into is opened at once, so a link
    /// on the way is found where it stands; otherwise a Filesystem may
    /// open several names in one call when the walk is complete.
    eager: bool,
    pub(crate) name: String,
    /// The path from the Mount's root, each name as the guest gave it.
    pub(crate) judged: String,
    /// Whether the path ended in `/` or `/.`, which only a directory
    /// satisfies.
    pub(crate) names_directory: bool,
    /// What stands at `name`, a link reported as itself, once asked.
    found: Option<Result<Metadata, FilesystemError>>,
}

impl Nameidata {
    /// The directory the walk stands in, which holds the last name.
    pub(crate) fn base(&self) -> &Arc<dyn Inode> {
        &self.frame().dir
    }

    /// Where `base` stands in its Filesystem.
    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        &self.frame().dentry
    }

    fn frame(&self) -> &Frame {
        self.frames.last().expect("a walk stands somewhere")
    }

    /// The dentry of the last name in `base`, as `d_alloc` makes one for a
    /// name a call is about.
    pub(crate) fn child(&self) -> Arc<Dentry> {
        self.child_of(&self.name)
    }

    /// The dentry of `name` in the directory the walk stands in.
    fn child_of(&self, name: &str) -> Arc<Dentry> {
        self.mount
            .filesystem()
            .dentries()
            .child(self.dentry(), name)
    }

    /// What stands at the name, a link reported as itself, asked of the
    /// Filesystem once however many checks need it; the directory itself
    /// for the empty name.
    pub(crate) fn found(&mut self) -> Result<Metadata, FilesystemError> {
        if self.found.is_none() {
            self.found = Some(look_up(self.base(), &self.child(), &self.name));
        }
        self.found.clone().expect("found was just asked")
    }

    /// Refuse the name as `NotADirectory` when it must be a directory and
    /// something else stands there.
    pub(crate) fn check_directory(&mut self) -> Result<(), Error> {
        if self.names_directory {
            match self.found() {
                Ok(metadata) if metadata.file_type != FileType::Directory => {
                    return Err(FilesystemError::NotADirectory.into())
                }
                Ok(_) | Err(FilesystemError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }

    /// The path from the Mount's root with each name spelled as the
    /// directory holding it stores it, a name nothing holds kept as given.
    pub(crate) fn spelled(&self) -> Result<String, FilesystemError> {
        let mut names = Vec::new();
        for pair in self.frames.windows(2) {
            names.push(pair[0].dir.stored_name(&pair[1].dentry)?);
        }
        if !self.name.is_empty() {
            names.push(self.base().stored_name(&self.child())?);
        }
        Ok(names
            .iter()
            .filter(|name| !name.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("/"))
    }

    /// Ask the Mount whether the guest may reach the path with `access`.
    pub(crate) fn check(&self, access: Access) -> Result<(), Error> {
        Ok(self.mount.check(|| self.spelled(), access)?)
    }
}

/// One name of a path still to walk, and whether a link's target named it.
struct Part {
    name: String,
    from_link: bool,
}

fn parts(path: &str, from_link: bool) -> VecDeque<Part> {
    path.split('/')
        .map(|name| Part {
            name: name.to_string(),
            from_link,
        })
        .collect()
}

/// A directory the walk stands in, its dentry, and whether a link led to
/// it.
struct Frame {
    dir: Arc<dyn Inode>,
    name: String,
    dentry: Arc<Dentry>,
    through_link: bool,
}

/// What stands at `name` in `dir`, a link reported as itself; `dir` itself
/// for the empty name.
fn look_up(dir: &Arc<dyn Inode>, child: &Dentry, name: &str) -> Result<Metadata, FilesystemError> {
    match name {
        "" => dir.metadata(),
        _ => dir.symlink_metadata(child),
    }
}

/// Refuse what a directory just stepped into, or just opened for a
/// descriptor, answers unless it opened as a directory.
pub(crate) fn opened_as_directory(dir: &Arc<dyn Inode>) -> Result<(), FilesystemError> {
    match dir.metadata()?.file_type {
        FileType::Directory => Ok(()),
        _ => Err(FilesystemError::NotADirectory),
    }
}

impl Nameidata {
    /// Step into the directory `part` names, or follow the link standing
    /// there; `dot` is a `.` or `..` right after it, which no link takes,
    /// so the directory opens at once there. The directory is opened
    /// first, a link standing there refused, so only a name that opens as
    /// none is asked what it is.
    fn descend(&mut self, part: Part, dot: bool) -> Result<(), Error> {
        let dentry = self.child_of(&part.name);
        let opened = self.base().open_dir(&dentry).and_then(|dir| {
            if self.eager || dot {
                opened_as_directory(&dir)?;
            }
            Ok(dir)
        });
        match opened {
            Ok(dir) => {
                self.frames.push(Frame {
                    dir,
                    name: part.name,
                    dentry,
                    through_link: part.from_link,
                });
                Ok(())
            }
            Err(FilesystemError::FilesystemLoop) => {
                match self.base().symlink_metadata(&dentry)?.file_type {
                    FileType::Symlink if dot => Err(FilesystemError::FilesystemLoop.into()),
                    FileType::Symlink => self.follow(&dentry),
                    _ => Err(FilesystemError::FilesystemLoop.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Walk on through where the link `dentry` names points, before
    /// whatever is left of the path.
    fn follow(&mut self, dentry: &Dentry) -> Result<(), Error> {
        if !self.follows_links {
            return Err(FilesystemError::FilesystemLoop.into());
        }
        self.links += 1;
        if self.links > MAX_LINKS {
            return Err(FilesystemError::FilesystemLoop.into());
        }
        let target = self.base().read_link(dentry)?;
        if target.starts_with('/') {
            return Err(FilesystemError::NotPermitted.into());
        }
        if target.is_empty() {
            return Err(FilesystemError::NotFound.into());
        }
        for part in parts(&target, true).into_iter().rev() {
            self.pending.push_front(part);
        }
        Ok(())
    }

    /// Step out of the directory the walk stands in. Out of the one the
    /// walk started from, a guest's `..` is no capability it holds and a
    /// link's is one leading out; out of one a link led to, a guest's `..`
    /// would tell where the link points.
    fn step_out(&mut self, from_link: bool) -> Result<(), Error> {
        if self.frames.len() == self.above + 1 {
            return Err(if from_link {
                FilesystemError::NotPermitted.into()
            } else {
                Error::NotCapable
            });
        }
        if !from_link && self.frames.last().is_some_and(|frame| frame.through_link) {
            return Err(FilesystemError::FilesystemLoop.into());
        }
        self.frames.pop();
        Ok(())
    }

    /// A final `.` after `name` stands in it, which must be a directory
    /// and no link.
    fn stays_in(&self, name: &str) -> Result<(), Error> {
        match self
            .base()
            .symlink_metadata(&self.child_of(name))?
            .file_type
        {
            FileType::Directory => Ok(()),
            FileType::Symlink => Err(FilesystemError::FilesystemLoop.into()),
            _ => Err(FilesystemError::NotADirectory.into()),
        }
    }

    /// Settle what the walk found at its last name, as Linux's
    /// `complete_walk` settles a lazy walk: a failure there may belong to
    /// a directory stepped into but not yet opened, so that directory is
    /// opened now, and its own failure answers the walk instead.
    fn complete(
        &self,
        found: Result<Metadata, FilesystemError>,
    ) -> Result<Result<Metadata, FilesystemError>, Error> {
        if found.is_err() && self.descended() {
            opened_as_directory(self.base())?;
        }
        Ok(found)
    }

    /// Whether the walk stepped into any directory beneath where it began.
    fn descended(&self) -> bool {
        self.frames.len() > self.above + 1
    }

    /// Walk every pending name but the last, which is left to act on.
    fn run(&mut self, mut last: Option<Part>) -> Result<Option<Part>, Error> {
        while let Some(part) = self.pending.pop_front() {
            if part.name.len() > NAME_MAX {
                return Err(FilesystemError::InvalidFilename.into());
            }
            let trailing = !part.from_link
                && self
                    .pending
                    .iter()
                    .all(|rest| !rest.from_link && matches!(rest.name.as_str(), "" | "."));
            match part.name.as_str() {
                "" | "." if trailing => {
                    if let (".", Some(name)) = (part.name.as_str(), &last) {
                        self.stays_in(&name.name)?;
                    }
                }
                "" | "." | ".." => {
                    if let Some(name) = last.take() {
                        let dot = !part.name.is_empty();
                        self.pending.push_front(part);
                        self.descend(name, dot)?;
                        continue;
                    }
                    if part.name == ".." {
                        self.step_out(part.from_link)?;
                    }
                }
                _ => {
                    if let Some(name) = last.take() {
                        self.pending.push_front(part);
                        self.descend(name, false)?;
                        continue;
                    }
                    last = Some(part);
                }
            }
        }
        Ok(last)
    }
}

/// `path`, beneath `start`. An empty path names nothing; an absolute path
/// or a `..` above `start` itself is not the guest's to reach; a `..` or a
/// final `.` after a component that is missing, no directory, or a link is
/// `NotFound`, `NotADirectory`, or `FilesystemLoop`. A name past
/// `NAME_MAX`, or a path the Policy could not be handed within `PATH_MAX`,
/// is `InvalidFilename`. Where `follow` asks, a link at the end is followed
/// too; one pointing at nothing is `FilesystemLoop` to a call that
/// `creates`, since what it would create could lie outside `start`.
///
/// The walk first lets a Filesystem open the directories it steps through
/// together, as the last is first asked about. A link on the way then
/// answers as a loop, and where links are followed the walk starts over,
/// opening each directory as it steps in, to find the link and follow it.
pub(crate) fn resolve(
    start: &Path,
    path: &str,
    follow: bool,
    creates: bool,
) -> Result<Nameidata, Error> {
    match path_lookupat(start, path, follow, creates, false) {
        Err(Error::Filesystem(FilesystemError::FilesystemLoop))
            if start.mount().follows_links() =>
        {
            path_lookupat(start, path, follow, creates, true)
        }
        resolved => resolved,
    }
}

/// One walk of `path`, as Linux's `path_lookupat` is one attempt that
/// `filename_lookup` repeats.
fn path_lookupat(
    start: &Path,
    path: &str,
    follow: bool,
    creates: bool,
    eager: bool,
) -> Result<Nameidata, Error> {
    let chain = start.reach()?;
    if path.is_empty() {
        return Err(FilesystemError::NotFound.into());
    }
    if path.starts_with('/') {
        return Err(Error::NotCapable);
    }
    let mut nd = Nameidata {
        mount: start.mount().clone(),
        above: chain.len() - 1,
        frames: chain
            .into_iter()
            .map(|Step { dir, name, dentry }| Frame {
                dir,
                name,
                dentry,
                through_link: false,
            })
            .collect(),
        pending: parts(path, false),
        follows_links: start.mount().follows_links(),
        links: 0,
        eager,
        name: String::new(),
        judged: String::new(),
        names_directory: names_directory(path),
        found: None,
    };
    let mut last = nd.run(None)?;
    let mut found = None;
    let mut followed = false;
    while let Some(name) = last.as_ref().filter(|_| follow) {
        let dentry = nd.child_of(&name.name);
        let looked_up = nd.base().symlink_metadata(&dentry);
        if !matches!(&looked_up, Ok(metadata) if metadata.file_type == FileType::Symlink) {
            found = Some(looked_up);
            break;
        }
        nd.follow(&dentry)?;
        followed = true;
        last = nd.run(None)?;
    }
    nd.name = last.map(|part| part.name).unwrap_or_default();
    // A walk that stepped into directories not yet opened settles its last
    // name now; one that stepped into none leaves it to the call to ask.
    nd.found = match found {
        Some(found) => Some(nd.complete(found)?),
        None if nd.descended() => Some(nd.complete(look_up(nd.base(), &nd.child(), &nd.name))?),
        None => None,
    };
    if followed && creates && matches!(nd.found, Some(Err(FilesystemError::NotFound))) {
        return Err(FilesystemError::FilesystemLoop.into());
    }
    nd.judged = start
        .dentry()
        .names()
        .iter()
        .map(String::as_str)
        .chain(
            nd.frames[nd.above + 1..]
                .iter()
                .map(|frame| frame.name.as_str()),
        )
        .chain(std::iter::once(nd.name.as_str()))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if nd.judged.len() >= PATH_MAX {
        return Err(FilesystemError::InvalidFilename.into());
    }
    // A directory removed through the VFS holds no name any more, as
    // Linux's lookup in a dead directory finds nothing.
    if !nd.name.is_empty() && nd.dentry().is_dead() {
        return Err(FilesystemError::NotFound.into());
    }
    Ok(nd)
}

/// Open `path` beneath `start` as `how` asks, as Linux's `path_openat`
/// walks to the last name and `do_open` opens it. `room` says whether the
/// caller has a descriptor left to hand the guest; without one the open
/// answers `TooManyOpenFiles` once the path is known to stand.
pub fn open(start: &Path, path: &str, how: &OpenHow, room: bool) -> Result<Opened, Error> {
    // A path ending in `/` asks for a directory, which cannot be created
    // as a file. Linux answers it once the names before the last are
    // walked, before it would follow a link standing at the last.
    if path.ends_with('/') && how.create {
        resolve(start, path, false, how.create)?;
        return Err(FilesystemError::IsADirectory.into());
    }
    // A trailing `/` resolves a link standing there, as Linux's `open` does.
    let follow = how.follow || names_directory(path);
    let mut nd = resolve(start, path, follow, how.create)?;
    // The walk followed every link the call asked it to, so a link at the
    // end now is one swapped in since, and is opened as itself.
    nd.check_directory()?;
    if !room {
        return Err(FilesystemError::TooManyOpenFiles.into());
    }
    let (existing, identity) = match nd.found() {
        Ok(metadata) => (Some(metadata.file_type), Identity::of(&metadata)),
        Err(FilesystemError::NotFound) => (None, None),
        Err(err) => return Err(err.into()),
    };
    refuse_what_stands(existing, how)?;
    // Creating changes the Mount only where nothing stands yet, so the
    // Mount judges the open once, knowing what it does.
    let changes = how.write || how.truncate || (how.create && existing.is_none());
    nd.check(if changes {
        Access::ReadWrite
    } else {
        Access::Read
    })?;
    match existing {
        Some(FileType::Directory) => vfs_open(start, nd, identity),
        Some(_) | None => {
            let options = OpenOptions {
                read: how.read,
                write: how.write,
                // A file already there is opened as it is, so a host that
                // opens only to create never asks for more than the guest.
                create: how.create && existing.is_none(),
                create_new: how.create && how.exclusive,
                truncate: how.truncate,
                nofollow: true,
            };
            let file = nd.base().open(&nd.child(), &options)?;
            Ok(Opened::File(mask_file_times(file, start.profile())))
        }
    }
}

/// What stands at the path answers before any access is judged, as Linux's
/// `open` does on a read-only mount.
fn refuse_what_stands(existing: Option<FileType>, how: &OpenHow) -> Result<(), Error> {
    let refusal = match existing {
        None if !how.create => Some(FilesystemError::NotFound),
        Some(_) if how.create && how.exclusive => Some(FilesystemError::AlreadyExists),
        Some(FileType::Directory) if how.write || how.truncate || how.create => {
            Some(FilesystemError::IsADirectory)
        }
        Some(FileType::Directory) | None => None,
        Some(_) if how.directory => Some(FilesystemError::NotADirectory),
        Some(_) => None,
    };
    match refusal {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

/// Make the directory `path` beneath `start`. A trailing `/` asks for the
/// directory being made, so it is no refusal here, as `mkdir(2)` takes it.
pub fn create_dir(start: &Path, path: &str) -> Result<(), Error> {
    let nd = resolve(start, path, false, false)?;
    nd.check(Access::ReadWrite)?;
    refuse_root(&nd.name, FilesystemError::AlreadyExists)?;
    Ok(nd.base().create_dir(&nd.child())?)
}

/// Remove the empty directory `path` beneath `start`.
pub fn remove_dir(start: &Path, path: &str) -> Result<(), Error> {
    refuse_dot_end(start, path, FilesystemError::InvalidInput)?;
    let mut nd = resolve(start, path, false, false)?;
    nd.check(Access::ReadWrite)?;
    nd.check_directory()?;
    refuse_root(&nd.name, FilesystemError::InvalidInput)?;
    nd.base().remove_dir(&nd.child())?;
    nd.mount
        .filesystem()
        .dentries()
        .delete(nd.dentry(), &nd.name);
    Ok(())
}

/// Remove the file `path` beneath `start`. Linux's `unlink` follows no
/// link at the end, so a trailing `/` is answered by the name itself: no
/// file removes as a directory.
pub fn remove_file(start: &Path, path: &str) -> Result<(), Error> {
    let mut nd = resolve(start, path, false, false)?;
    nd.check(Access::ReadWrite)?;
    nd.check_directory()?;
    refuse_root(&nd.name, FilesystemError::IsADirectory)?;
    Ok(nd.base().remove_file(&nd.child())?)
}

/// Move `from_path` beneath `from` to `to_path` beneath `to`. A rename
/// stays within one Mount; across two it crosses devices, as across two
/// mounted devices. A directory moves only beneath a Mount that allows it,
/// the Filesystem checking what it moves as it moves it.
pub fn rename(from: &Path, from_path: &str, to: &Path, to_path: &str) -> Result<(), Error> {
    refuse_dot_end(from, from_path, FilesystemError::ResourceBusy)?;
    refuse_dot_end(to, to_path, FilesystemError::ResourceBusy)?;
    let mut source = resolve(from, from_path, false, false)?;
    let mut target = resolve(to, to_path, false, false)?;
    refuse_root(&source.name, FilesystemError::ResourceBusy)?;
    refuse_root(&target.name, FilesystemError::ResourceBusy)?;
    // A trailing `/` on either side asks that the source be a directory,
    // which `rename(2)` checks before it finds a rename onto its own path
    // leaves everything as it was. A rename moves a link rather than what
    // it points at, so each side is checked as it stands.
    let names_directory = source.names_directory || target.names_directory;
    let onto_itself = target.judged == source.judged;
    source.names_directory = names_directory;
    source.check_directory()?;
    if !onto_itself {
        target.check_directory()?;
    }
    if !Arc::ptr_eq(from.mount(), to.mount()) {
        return Err(FilesystemError::CrossesDevices.into());
    }
    source.check(Access::ReadWrite)?;
    target.check(Access::ReadWrite)?;
    // Each side was walked on its own, so a rename onto its own path is
    // known by the path rather than by the directories each walk opened;
    // it changes nothing once the source stands, as `rename(2)` answers.
    if onto_itself {
        source.found()?;
        return Ok(());
    }
    let rename = match (from.mount().moves_directories(), names_directory) {
        (false, _) => Rename::File,
        (true, true) => Rename::Directory,
        (true, false) => Rename::Any,
    };
    source.base().rename(
        &source.child(),
        target.base().as_ref(),
        &target.child(),
        rename,
    )?;
    // A directory replaced at the target is gone for whoever holds it, and
    // one moved from the source is reached where it went.
    let dentries = source.mount.filesystem().dentries();
    dentries.delete(target.dentry(), &target.name);
    dentries.rename(source.dentry(), &source.name, target.dentry(), &target.name);
    Ok(())
}

/// Refuse a guest path ending in `.` or `..` as `refusal`, since it names
/// no entry a call may remove or move. Linux looks up what stands before
/// it first, following a link there as it follows one that is not the
/// last name, so a missing or non-directory parent answers as itself.
fn refuse_dot_end(start: &Path, path: &str, refusal: FilesystemError) -> Result<(), Error> {
    let trimmed = path.trim_end_matches('/');
    let (parent, last) = trimmed.rsplit_once('/').unwrap_or(("", trimmed));
    if !matches!(last, "." | "..") {
        return Ok(());
    }
    if !parent.is_empty()
        && resolve(start, parent, true, false)?.found()?.file_type != FileType::Directory
    {
        return Err(FilesystemError::NotADirectory.into());
    }
    Err(refusal.into())
}

/// A Mount's root is where the guest stands, not a directory it may
/// remove or move; each call refuses it as Linux refuses its own root.
fn refuse_root(name: &str, refusal: FilesystemError) -> Result<(), Error> {
    if name.is_empty() {
        return Err(refusal.into());
    }
    Ok(())
}
