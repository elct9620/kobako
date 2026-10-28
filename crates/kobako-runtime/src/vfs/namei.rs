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
use super::openat2::{vfs_open, FileDescription, OpenHow};
use super::path::{Identity, Path, Step};
use super::rwsem::Held;
use super::times::mask_file_times;
use super::Error;
use crate::filesystem::{Access, FileType, FilesystemError, Inode, Metadata, OpenOptions, Rename};
use crate::mount::Mount;

use super::{NAME_MAX, PATH_MAX};

/// The most links one walk follows, as Linux's.
const MAXSYMLINKS: usize = 40;

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
            let parent = self.dentry().clone();
            self.found = Some(
                down_read(&self.mount, &parent)
                    .and_then(|_held| look_up(self.base(), &self.child(), &self.name)),
            );
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
    pub(crate) fn dentry_path(&self) -> Result<String, FilesystemError> {
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
        Ok(self.mount.check(|| self.dentry_path(), access)?)
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

/// Hold `dentry`'s directory lock shared, as Linux's lookup holds the
/// directory it looks a name up in, so no change through the VFS lands
/// between the name's dentry and what the Filesystem answers for it.
pub(crate) fn down_read<'a>(
    mount: &Mount,
    dentry: &'a Dentry,
) -> Result<Held<'a>, FilesystemError> {
    dentry.rwsem.down_read(mount.filesystem().blocking())
}

/// Hold `dentry`'s directory lock exclusive, as Linux holds a directory
/// while a name in it is made, removed, or moved.
fn down_write<'a>(mount: &Mount, dentry: &'a Dentry) -> Result<Held<'a>, FilesystemError> {
    dentry.rwsem.down_write(mount.filesystem().blocking())
}

/// Refuse what a directory just stepped into, or just opened for a
/// descriptor, answers unless it opened as a directory.
pub(crate) fn d_can_lookup(dir: &Arc<dyn Inode>) -> Result<(), FilesystemError> {
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
    fn walk_component(&mut self, part: Part, dot: bool) -> Result<(), Error> {
        let parent = self.dentry().clone();
        let _held = down_read(&self.mount, &parent)?;
        let dentry = self.child_of(&part.name);
        let opened = self.base().open_dir(&dentry).and_then(|dir| {
            if self.eager || dot {
                d_can_lookup(&dir)?;
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
                    FileType::Symlink => self.pick_link(&dentry),
                    _ => Err(FilesystemError::FilesystemLoop.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Walk on through where the link `dentry` names points, before
    /// whatever is left of the path.
    fn pick_link(&mut self, dentry: &Dentry) -> Result<(), Error> {
        if !self.follows_links {
            return Err(FilesystemError::FilesystemLoop.into());
        }
        self.links += 1;
        if self.links > MAXSYMLINKS {
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
    fn follow_dotdot(&mut self, from_link: bool) -> Result<(), Error> {
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
        let _held = down_read(&self.mount, self.dentry())?;
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
    fn complete_walk(
        &self,
        found: Result<Metadata, FilesystemError>,
    ) -> Result<Result<Metadata, FilesystemError>, Error> {
        if found.is_err() && self.descended() {
            d_can_lookup(self.base())?;
        }
        Ok(found)
    }

    /// Whether the walk stepped into any directory beneath where it began.
    fn descended(&self) -> bool {
        self.frames.len() > self.above + 1
    }

    /// Walk every pending name but the last, which is left to act on.
    fn link_path_walk(&mut self, mut last: Option<Part>) -> Result<Option<Part>, Error> {
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
                        self.walk_component(name, dot)?;
                        continue;
                    }
                    if part.name == ".." {
                        self.follow_dotdot(part.from_link)?;
                    }
                }
                _ => {
                    if let Some(name) = last.take() {
                        self.pending.push_front(part);
                        self.walk_component(name, false)?;
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
pub(crate) fn filename_lookup(
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
    let mut last = nd.link_path_walk(None)?;
    let mut found = None;
    let mut followed = false;
    while let Some(name) = last.as_ref().filter(|_| follow) {
        let parent = nd.dentry().clone();
        let held = down_read(&nd.mount, &parent)?;
        let dentry = nd.child_of(&name.name);
        let looked_up = nd.base().symlink_metadata(&dentry);
        if !matches!(&looked_up, Ok(metadata) if metadata.file_type == FileType::Symlink) {
            found = Some(looked_up);
            break;
        }
        nd.pick_link(&dentry)?;
        drop(held);
        followed = true;
        last = nd.link_path_walk(None)?;
    }
    nd.name = last.map(|part| part.name).unwrap_or_default();
    // A walk that stepped into directories not yet opened settles its last
    // name now; one that stepped into none leaves it to the call to ask.
    nd.found = match found {
        Some(found) => Some(nd.complete_walk(found)?),
        None if nd.descended() => {
            let looked_up = nd.found();
            Some(nd.complete_walk(looked_up)?)
        }
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
pub fn open(start: &Path, path: &str, how: &OpenHow, room: bool) -> Result<FileDescription, Error> {
    // A path ending in `/` asks for a directory, which cannot be created
    // as a file. Linux answers it once the names before the last are
    // walked, before it would follow a link standing at the last.
    if path.ends_with('/') && how.create {
        filename_lookup(start, path, false, how.create)?;
        return Err(FilesystemError::IsADirectory.into());
    }
    // A trailing `/` resolves a link standing there, as Linux's `open` does.
    let follow = how.follow || names_directory(path);
    let mut nd = filename_lookup(start, path, follow, how.create)?;
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
            let parent = nd.dentry().clone();
            // A file made holds the directory, as Linux's `O_CREAT` does.
            let _held = match options.create {
                true => Some(down_write(&nd.mount, &parent)?),
                false => None,
            };
            let file = nd.base().open(&nd.child(), &options)?;
            Ok(FileDescription::File(mask_file_times(
                file,
                start.profile(),
            )))
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
    let nd = filename_lookup(start, path, false, false)?;
    nd.check(Access::ReadWrite)?;
    refuse_root(&nd.name, FilesystemError::AlreadyExists)?;
    let _held = down_write(&nd.mount, nd.dentry())?;
    Ok(nd.base().create_dir(&nd.child())?)
}

/// Remove the empty directory `path` beneath `start`.
pub fn remove_dir(start: &Path, path: &str) -> Result<(), Error> {
    refuse_dot_end(start, path, FilesystemError::InvalidInput)?;
    let mut nd = filename_lookup(start, path, false, false)?;
    nd.check(Access::ReadWrite)?;
    nd.check_directory()?;
    refuse_root(&nd.name, FilesystemError::InvalidInput)?;
    // The directory and the one it removes are held until its dentry is
    // dead, so no lookup in between finds the dentry still alive.
    let victim = nd.child();
    let _parent = down_write(&nd.mount, nd.dentry())?;
    let _victim = down_write(&nd.mount, &victim)?;
    nd.base().remove_dir(&victim)?;
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
    let mut nd = filename_lookup(start, path, false, false)?;
    nd.check(Access::ReadWrite)?;
    nd.check_directory()?;
    refuse_root(&nd.name, FilesystemError::IsADirectory)?;
    let victim = nd.child();
    let _parent = down_write(&nd.mount, nd.dentry())?;
    let _victim = down_write(&nd.mount, &victim)?;
    Ok(nd.base().remove_file(&victim)?)
}

/// Move `from_path` beneath `from` to `to_path` beneath `to`. A rename
/// stays within one Mount; across two it crosses devices, as across two
/// mounted devices. A directory moves only beneath a Mount that allows it,
/// the Filesystem checking what it moves as it moves it.
pub fn rename(from: &Path, from_path: &str, to: &Path, to_path: &str) -> Result<(), Error> {
    refuse_dot_end(from, from_path, FilesystemError::ResourceBusy)?;
    refuse_dot_end(to, to_path, FilesystemError::ResourceBusy)?;
    let mut source = filename_lookup(from, from_path, false, false)?;
    let mut target = filename_lookup(to, to_path, false, false)?;
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
    let (old, new) = (source.child(), target.child());
    let filesystem = from.mount().filesystem();
    let (old_dir, new_dir) = (source.dentry(), target.dentry());
    let blocking = filesystem.blocking();
    // A move between two directories holds the Filesystem's rename lock,
    // so which stands beneath the other holds still while it is asked, as
    // Linux's `lock_rename` asks before it locks either.
    let _rename = match Arc::ptr_eq(old_dir, new_dir) {
        true => None,
        false => {
            let held = filesystem.rename_mutex().down_write(blocking)?;
            if new_dir.is_subdir(&old) {
                return Err(FilesystemError::InvalidInput.into());
            }
            if old_dir.is_subdir(&new) {
                return Err(FilesystemError::DirectoryNotEmpty.into());
            }
            Some(held)
        }
    };
    let _parents = lock_parents(&source.mount, old_dir, new_dir)?;
    let _old = down_write(&source.mount, &old)?;
    let _new = down_write(&source.mount, &new)?;
    source
        .base()
        .rename(&old, target.base().as_ref(), &new, rename)?;
    // A directory replaced at the target is gone for whoever holds it, and
    // one moved from the source is reached where it went.
    let dentries = source.mount.filesystem().dentries();
    dentries.delete(target.dentry(), &target.name);
    dentries.d_move(source.dentry(), &source.name, target.dentry(), &target.name);
    Ok(())
}

/// Hold both directories a move changes exclusive, the one above the other
/// first and otherwise in a fixed order, as Linux locks a rename's parents
/// ancestors first, so two moves never wait on each other.
fn lock_parents<'a>(
    mount: &Mount,
    old_dir: &'a Arc<Dentry>,
    new_dir: &'a Arc<Dentry>,
) -> Result<Vec<Held<'a>>, FilesystemError> {
    if Arc::ptr_eq(old_dir, new_dir) {
        return Ok(vec![down_write(mount, old_dir)?]);
    }
    let old_first = new_dir.is_subdir(old_dir)
        || (!old_dir.is_subdir(new_dir) && Arc::as_ptr(old_dir) < Arc::as_ptr(new_dir));
    let (first, second) = match old_first {
        true => (old_dir, new_dir),
        false => (new_dir, old_dir),
    };
    let first = down_write(mount, first)?;
    Ok(vec![first, down_write(mount, second)?])
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
        && filename_lookup(start, parent, true, false)?
            .found()?
            .file_type
            != FileType::Directory
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use crate::filesystem::{
        Access, DirEntry, File, FilesystemError, Inode, Metadata, OpenOptions, Rename, Tmpfs,
    };
    use crate::mount::Mount;
    use crate::profile::Profile;
    use crate::vfs::{self, Blocking, Dentry, FileDescription, Filesystem, OpenHow, Path};

    type Hook = Box<dyn Fn() -> Result<(), FilesystemError> + Send + Sync>;

    /// A Tmpfs root a test steps into: after it removes a directory, in
    /// place of making one, and as it moves an entry. A directory it opens
    /// is the Tmpfs's own, so only calls on the root are stepped into.
    #[derive(Default)]
    struct Hooked {
        tmpfs: Tmpfs,
        removed: Option<Hook>,
        making: Option<Hook>,
        renames: AtomicUsize,
    }

    impl Inode for Hooked {
        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.tmpfs.open(dentry, options)
        }

        fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
            self.tmpfs.open_dir(dentry)
        }

        fn metadata(&self) -> Result<Metadata, FilesystemError> {
            self.tmpfs.metadata()
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.tmpfs.symlink_metadata(dentry)
        }

        fn stored_name(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
            self.tmpfs.stored_name(dentry)
        }

        fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
            self.tmpfs.read_link(dentry)
        }

        fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
            self.tmpfs.entries()
        }

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            match &self.making {
                Some(making) => making(),
                None => self.tmpfs.create_dir(dentry),
            }
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_dir(dentry)?;
            self.removed.as_ref().map_or(Ok(()), |removed| removed())
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            self.renames.fetch_add(1, Ordering::SeqCst);
            self.tmpfs.rename(from, to_dir, to, rename)
        }
    }

    /// Counts each wait a Filesystem's directory locks make through it.
    #[derive(Default)]
    struct CountsWaits(AtomicUsize);

    impl Blocking for CountsWaits {
        fn wait(&self, wait: &mut dyn FnMut()) {
            self.0.fetch_add(1, Ordering::SeqCst);
            wait();
        }
    }

    fn root_of(filesystem: Arc<Filesystem>) -> Path {
        let mount =
            Mount::new("/data", filesystem, Access::ReadWrite).expect("/data is a guest path");
        Path::root(mount, Profile::Permissive)
    }

    fn open_dir(start: &Path, path: &str) -> Path {
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        match vfs::open(start, path, &how, true) {
            Ok(FileDescription::Directory(dir)) => dir,
            _ => panic!("{path} must open as a directory"),
        }
    }

    // The removal pauses between the Filesystem removing the directory and
    // the VFS marking its dentry dead, which is where a make and an open of
    // the same name would take the dentry about to die.
    // @behavior RT-163
    #[test]
    fn a_directory_made_while_another_is_removed_in_its_place_stays_alive() {
        let (removed, removing) = mpsc::channel();
        let (made, making) = mpsc::channel::<()>();
        let (removed, making) = (Mutex::new(removed), Mutex::new(making));
        let hooked = Hooked {
            removed: Some(Box::new(move || {
                removed.lock().expect("unpoisoned").send(()).ok();
                let _ = making
                    .lock()
                    .expect("unpoisoned")
                    .recv_timeout(Duration::from_millis(200));
                Ok(())
            })),
            ..Hooked::default()
        };
        let root = root_of(Filesystem::new(Arc::new(hooked)));
        vfs::create_dir(&root, "d").expect("the directory must be made");
        let remover = {
            let root = root.clone();
            thread::spawn(move || vfs::remove_dir(&root, "d"))
        };
        removing
            .recv()
            .expect("the removal must reach the Filesystem");
        vfs::create_dir(&root, "d").expect("another directory must be made in its place");
        let dir = open_dir(&root, "d");
        made.send(()).ok();
        remover
            .join()
            .expect("the remover must not panic")
            .expect("the removal must succeed");
        assert_eq!(
            vfs::create_dir(&dir, "inner"),
            Ok(()),
            "vfs::create_dir through a directory vfs::create_dir made while vfs::remove_dir \
             removed the one in its place must make into the directory made"
        );
    }

    // The Filesystem answers a move as its Tmpfs would, so only a count of
    // zero shows the VFS refused it before asking.
    // @behavior RT-164
    #[test]
    fn a_directory_moved_beneath_itself_is_refused_whatever_its_filesystem() {
        let hooked = Arc::new(Hooked::default());
        let root = root_of(Filesystem::new(hooked.clone()));
        vfs::create_dir(&root, "a").expect("the directory must be made");
        vfs::create_dir(&root, "a/b").expect("the directory beneath it must be made");
        assert_eq!(
            (
                vfs::rename(&root, "a", &root, "a/b/a"),
                hooked.renames.load(Ordering::SeqCst)
            ),
            (
                Err(vfs::Error::Filesystem(FilesystemError::InvalidInput)),
                0
            ),
            "vfs::rename of a directory into a directory beneath it must answer InvalidInput \
             without asking the Filesystem"
        );
    }

    // @behavior RT-165
    #[test]
    fn a_filesystem_changing_the_directory_its_own_change_holds_answers_a_deadlock() {
        let root = Arc::new(OnceLock::<Path>::new());
        let hooked = Hooked {
            making: Some(Box::new({
                let root = root.clone();
                move || match vfs::create_dir(root.get().expect("the root is set"), "other") {
                    Err(vfs::Error::Filesystem(err)) => Err(err),
                    _ => Ok(()),
                }
            })),
            ..Hooked::default()
        };
        let path = root_of(Filesystem::new(Arc::new(hooked)));
        root.set(path.clone()).ok();
        assert_eq!(
            vfs::create_dir(&path, "made"),
            Err(vfs::Error::Filesystem(FilesystemError::Deadlock)),
            "vfs::create_dir called by a Filesystem's own create_dir on the directory the outer \
             vfs::create_dir holds must answer Deadlock"
        );
    }

    // @behavior RT-166
    #[test]
    fn a_call_waiting_for_a_directorys_lock_waits_through_its_filesystems_blocking() {
        let counts = Arc::new(CountsWaits::default());
        let (removed, removing) = mpsc::channel();
        let removed = Mutex::new(removed);
        let hooked = Hooked {
            removed: Some(Box::new({
                let counts = counts.clone();
                move || {
                    removed.lock().expect("unpoisoned").send(()).ok();
                    // Bounded, so a call that never waits fails the count
                    // rather than hanging the test.
                    for _ in 0..2000 {
                        if counts.0.load(Ordering::SeqCst) > 0 {
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                }
            })),
            ..Hooked::default()
        };
        let root = root_of(Filesystem::with_blocking(Arc::new(hooked), counts.clone()));
        vfs::create_dir(&root, "d").expect("the directory must be made");
        let remover = {
            let root = root.clone();
            thread::spawn(move || vfs::remove_dir(&root, "d"))
        };
        removing
            .recv()
            .expect("the removal must reach the Filesystem");
        let made = vfs::create_dir(&root, "e");
        remover
            .join()
            .expect("the remover must not panic")
            .expect("the removal must succeed");
        assert_eq!(
            (made, counts.0.load(Ordering::SeqCst)),
            (Ok(()), 1),
            "vfs::create_dir on a directory another thread's vfs::remove_dir holds must wait \
             through the Blocking given by Filesystem::with_blocking, then answer"
        );
    }
}
