//! Resolving a guest path beneath a directory, one name at a time, as
//! Linux's namei walks a path.
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
use super::path::{Path, Step};
use super::Error;
use crate::filesystem::{Access, FileType, FilesystemError, Inode, Metadata};
use crate::mount::Mount;

use super::{NAME_MAX, PATH_MAX};

/// The most links one walk follows, as Linux's `MAXSYMLINKS`.
const MAX_LINKS: usize = 40;

/// Whether a guest path asks for a directory, ending in `/` or `/.`.
pub(crate) fn names_directory(path: &str) -> bool {
    matches!(path.rsplit('/').next(), Some("" | "."))
}

/// A guest path resolved beneath a directory: the directory holding its
/// last name, that name, empty for the directory itself, and the path from
/// the Mount's root the walk took.
pub(crate) struct Resolved {
    pub(crate) mount: Arc<Mount>,
    pub(crate) base: Arc<dyn Inode>,
    /// Where `base` stands in its Filesystem.
    pub(crate) dentry: Arc<Dentry>,
    pub(crate) name: String,
    /// The path from the Mount's root, each name as the guest gave it.
    pub(crate) judged: String,
    /// Whether the path ended in `/` or `/.`, which only a directory
    /// satisfies.
    pub(crate) names_directory: bool,
    /// Each directory from where the walk can be spelled from, the Mount's
    /// root beneath a Policy, down to `base`.
    frames: Vec<Frame>,
    /// What stands at `name`, a link reported as itself, as the walk
    /// completed.
    found: Result<Metadata, FilesystemError>,
}

impl Resolved {
    /// The dentry of the name in `base`, as `d_alloc` makes one for a name
    /// a call is about.
    pub(crate) fn child(&self) -> Arc<Dentry> {
        self.mount
            .filesystem()
            .dentries()
            .child(&self.dentry, &self.name)
    }

    /// What stands at the name, a link reported as itself, asked of the
    /// Filesystem once however many checks need it; the directory itself
    /// for the empty name.
    pub(crate) fn found(&self) -> Result<Metadata, FilesystemError> {
        self.found.clone()
    }

    /// Refuse the name as `NotADirectory` when it must be a directory and
    /// something else stands there.
    pub(crate) fn check_directory(&self) -> Result<(), Error> {
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
            names.push(self.base.stored_name(&self.child())?);
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

struct Walk {
    mount: Arc<Mount>,
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
}

/// Refuse what a directory just stepped into, or just opened for a
/// descriptor, answers unless it opened as a directory.
pub(crate) fn opened_as_directory(dir: &Arc<dyn Inode>) -> Result<(), FilesystemError> {
    match dir.metadata()?.file_type {
        FileType::Directory => Ok(()),
        _ => Err(FilesystemError::NotADirectory),
    }
}

impl Walk {
    fn top(&self) -> &Arc<dyn Inode> {
        &self.frames.last().expect("a walk stands somewhere").dir
    }

    /// The dentry of `name` in the directory the walk stands in.
    fn child(&self, name: &str) -> Arc<Dentry> {
        self.mount.filesystem().dentries().child(
            &self.frames.last().expect("a walk stands somewhere").dentry,
            name,
        )
    }

    /// Step into the directory `part` names, or follow the link standing
    /// there; `dot` is a `.` or `..` right after it, which no link takes,
    /// so the directory opens at once there. The directory is opened
    /// first, a link standing there refused, so only a name that opens as
    /// none is asked what it is.
    fn descend(&mut self, part: Part, dot: bool) -> Result<(), Error> {
        let dentry = self.child(&part.name);
        let opened = self.top().open_dir(&dentry).and_then(|dir| {
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
                match self.top().symlink_metadata(&dentry)?.file_type {
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
        let target = self.top().read_link(dentry)?;
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
        match self.top().symlink_metadata(&self.child(name))?.file_type {
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
        if found.is_err() && self.frames.len() > self.above + 1 {
            opened_as_directory(self.top())?;
        }
        Ok(found)
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
) -> Result<Resolved, Error> {
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
) -> Result<Resolved, Error> {
    let chain = start.reach()?;
    if path.is_empty() {
        return Err(FilesystemError::NotFound.into());
    }
    if path.starts_with('/') {
        return Err(Error::NotCapable);
    }
    let mut walk = Walk {
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
    };
    let mut last = walk.run(None)?;
    let mut found = None;
    let mut followed = false;
    while let Some(name) = last.as_ref().filter(|_| follow) {
        let dentry = walk.child(&name.name);
        let looked_up = walk.top().symlink_metadata(&dentry);
        if !matches!(&looked_up, Ok(metadata) if metadata.file_type == FileType::Symlink) {
            found = Some(looked_up);
            break;
        }
        walk.follow(&dentry)?;
        followed = true;
        last = walk.run(None)?;
    }
    let name = last.map(|part| part.name).unwrap_or_default();
    let found = match found {
        Some(found) => found,
        None if name.is_empty() => walk.top().metadata(),
        None => walk.top().symlink_metadata(&walk.child(&name)),
    };
    let found = walk.complete(found)?;
    if followed && creates && matches!(found, Err(FilesystemError::NotFound)) {
        return Err(FilesystemError::FilesystemLoop.into());
    }
    let judged = start
        .dentry()
        .names()
        .iter()
        .map(String::as_str)
        .chain(
            walk.frames[walk.above + 1..]
                .iter()
                .map(|frame| frame.name.as_str()),
        )
        .chain(std::iter::once(name.as_str()))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if judged.len() >= PATH_MAX {
        return Err(FilesystemError::InvalidFilename.into());
    }
    let top = walk.frames.last().expect("a walk stands somewhere");
    // A directory removed through the VFS holds no name any more, as
    // Linux's lookup in a dead directory finds nothing.
    if !name.is_empty() && top.dentry.is_dead() {
        return Err(FilesystemError::NotFound.into());
    }
    let (base, dentry) = (top.dir.clone(), top.dentry.clone());
    Ok(Resolved {
        mount: start.mount().clone(),
        base,
        dentry,
        name,
        judged,
        names_directory: names_directory(path),
        frames: walk.frames,
        found,
    })
}
