//! Resolving a guest path beneath a directory descriptor, one name at a
//! time, as the VFS above a filesystem walks a path.
//!
//! The runtime is the boundary: a path never climbs above the directory
//! descriptor it is resolved beneath, as a capability never widens, and
//! the Mount is asked what the guest may do there before the Filesystem
//! is, so a Filesystem of a third party's need check neither. Each step
//! hands a Filesystem one name in a directory already held, opened without
//! following a link there, so nothing another writer swaps in on the way
//! leads the walk elsewhere.
//!
//! A link is followed only where the walk itself reads where it points,
//! within the directory the descriptor opened, as `openat2`'s
//! `RESOLVE_BENEATH` keeps a resolution: one leading out of it, or to an
//! absolute path, is not permitted. A `..` or a `.` taking the walk out of
//! a directory a link led to is refused as a loop, so a guest never learns
//! where a link points. Beneath a Mount with a Policy no link is followed
//! at all, as `RESOLVE_NO_SYMLINKS` refuses one, and the descriptor's
//! directory is reached again from the Mount's root, so the Policy judges
//! the path it stands at now.

use std::collections::VecDeque;
use std::sync::Arc;

use kobako_runtime::filesystem::{FileType, Filesystem, FilesystemError, Metadata};

use super::errno::{Answer, Errno};
use crate::descriptors::{Identity, OpenDirectory};

/// Linux's limits on a path, its terminating NUL counted, and on one name
/// within it.
pub(crate) const PATH_MAX: usize = 4096;
pub(crate) const NAME_MAX: usize = 255;
/// The most links one walk follows, as Linux's `MAXSYMLINKS`.
const MAX_LINKS: usize = 40;

/// A guest path resolved beneath a directory: the directory holding its
/// last name, that name, empty for the directory itself, the path from the
/// Mount's root a Policy judges, and whether it ended in `/` or `/.`,
/// which only a directory satisfies.
pub(crate) struct ResolvedPath {
    pub(crate) base: Arc<dyn Filesystem>,
    pub(crate) path: String,
    pub(crate) judged: String,
    pub(crate) names_directory: bool,
}

impl ResolvedPath {
    /// The directory and name, refused as `NOTDIR` when the name must be a
    /// directory and `lookup` finds anything else there: `metadata` for a
    /// call that follows a link at the end, `symlink_metadata` for one that
    /// acts on the link itself.
    pub(crate) fn check_directory(
        self,
        lookup: fn(&(dyn Filesystem + 'static), &str) -> Result<Metadata, FilesystemError>,
    ) -> Answer<(Arc<dyn Filesystem>, String)> {
        if self.names_directory {
            match lookup(self.base.as_ref(), &self.path) {
                Ok(metadata) if metadata.file_type != FileType::Directory => {
                    return Err(Errno::Notdir.into())
                }
                Ok(_) | Err(FilesystemError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok((self.base, self.path))
    }
}

/// Whether a guest path asks for a directory, ending in `/` or `/.`.
pub(crate) fn names_directory(path: &str) -> bool {
    matches!(path.rsplit('/').next(), Some("" | "."))
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

/// A directory the walk stands in, and whether a link led to it.
struct Frame {
    dir: Arc<dyn Filesystem>,
    name: String,
    through_link: bool,
}

struct Walk {
    frames: Vec<Frame>,
    pending: VecDeque<Part>,
    follows_links: bool,
    links: usize,
}

impl Walk {
    fn top(&self) -> &Arc<dyn Filesystem> {
        &self.frames.last().expect("a walk stands somewhere").dir
    }

    /// Step into the directory `part` names, or follow the link standing
    /// there; `dot` is a `.` or `..` right after it, which no link takes.
    fn descend(&mut self, part: Part, dot: bool) -> Answer {
        let found = self.top().symlink_metadata(&part.name)?;
        match found.file_type {
            FileType::Directory => {
                let dir = self.top().open_dir_nofollow(&part.name)?;
                self.frames.push(Frame {
                    dir,
                    name: part.name,
                    through_link: part.from_link,
                });
                Ok(())
            }
            FileType::Symlink if dot => Err(Errno::Loop.into()),
            FileType::Symlink => self.follow(&part.name),
            _ => Err(Errno::Notdir.into()),
        }
    }

    /// Walk on through where the link `name` points, before whatever is
    /// left of the path.
    fn follow(&mut self, name: &str) -> Answer {
        if !self.follows_links {
            return Err(Errno::Loop.into());
        }
        self.links += 1;
        if self.links > MAX_LINKS {
            return Err(Errno::Loop.into());
        }
        let target = self.top().read_link(name)?;
        if target.starts_with('/') {
            return Err(Errno::Perm.into());
        }
        if target.is_empty() {
            return Err(Errno::Noent.into());
        }
        for part in parts(&target, true).into_iter().rev() {
            self.pending.push_front(part);
        }
        Ok(())
    }

    /// Step out of the directory the walk stands in. Out of the one the
    /// descriptor opened, a guest's `..` is no capability it holds and a
    /// link's is one leading out; out of one a link led to, a guest's `..`
    /// would tell where the link points.
    fn step_out(&mut self, from_link: bool) -> Answer {
        if self.frames.len() == 1 {
            return Err(if from_link {
                Errno::Perm
            } else {
                Errno::Notcapable
            }
            .into());
        }
        if !from_link && self.frames.last().is_some_and(|frame| frame.through_link) {
            return Err(Errno::Loop.into());
        }
        self.frames.pop();
        Ok(())
    }

    /// A final `.` after `name` stands in it, which must be a directory
    /// and no link.
    fn stays_in(&self, name: &str) -> Answer {
        match self.top().symlink_metadata(name)?.file_type {
            FileType::Directory => Ok(()),
            FileType::Symlink => Err(Errno::Loop.into()),
            _ => Err(Errno::Notdir.into()),
        }
    }

    /// Walk every pending name but the last, which is left to act on.
    fn run(&mut self, mut last: Option<Part>) -> Answer<Option<Part>> {
        while let Some(part) = self.pending.pop_front() {
            if part.name.len() > NAME_MAX {
                return Err(Errno::Nametoolong.into());
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

/// Where a call through `dir` starts: the directory itself, or the
/// directory found again from the Mount's root beneath a Mount following
/// no link, which is one with a Policy.
fn reached(dir: &OpenDirectory) -> Answer<Arc<dyn Filesystem>> {
    if dir.mount.follows_links() {
        return Ok(dir.handle.clone());
    }
    let mut at = dir.mount.filesystem().clone();
    for name in dir.path.split('/').filter(|name| !name.is_empty()) {
        at = match at.open_dir_nofollow(name) {
            Ok(next) => next,
            Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
                return Err(Errno::Noent.into())
            }
            Err(err) => return Err(err.into()),
        };
    }
    confirm(dir, at.as_ref())?;
    Ok(at)
}

/// The directory `dir` stands for, and its path from there, for a call on
/// the directory itself.
pub(crate) fn directory_of(dir: &OpenDirectory) -> Answer<(Arc<dyn Filesystem>, String)> {
    Ok((reached(dir)?, String::new()))
}

/// `path`, beneath `dir`. An empty path names nothing; an absolute path or
/// a `..` above `dir` itself is not the guest's to reach; a `..` or a final
/// `.` after a component that is missing, no directory, or a link is
/// `NOENT`, `NOTDIR`, or `LOOP`. A name past `NAME_MAX`, or a path the
/// Policy could not be handed within `PATH_MAX`, is `NAMETOOLONG`. Where
/// `follow` asks, a link at the end is followed too; one pointing at
/// nothing is `LOOP` to a call that `creates`, since what it would create
/// could lie outside `dir`.
pub(crate) fn resolve(
    dir: &OpenDirectory,
    path: &str,
    follow: bool,
    creates: bool,
) -> Answer<ResolvedPath> {
    let start = reached(dir)?;
    if path.is_empty() {
        return Err(Errno::Noent.into());
    }
    if path.starts_with('/') {
        return Err(Errno::Notcapable.into());
    }
    let mut walk = Walk {
        frames: vec![Frame {
            dir: start,
            name: String::new(),
            through_link: false,
        }],
        pending: parts(path, false),
        follows_links: dir.mount.follows_links(),
        links: 0,
    };
    let mut last = walk.run(None)?;
    let mut followed = false;
    while let Some(name) = last.as_ref().filter(|_| follow) {
        match walk.top().symlink_metadata(&name.name) {
            Ok(found) if found.file_type == FileType::Symlink => {}
            _ => break,
        }
        walk.follow(&name.name)?;
        followed = true;
        last = walk.run(None)?;
    }
    if followed && creates {
        if let Some(name) = &last {
            if let Err(FilesystemError::NotFound) = walk.top().symlink_metadata(&name.name) {
                return Err(Errno::Loop.into());
            }
        }
    }
    let name = last.map(|part| part.name).unwrap_or_default();
    let judged = std::iter::once(dir.path.as_str())
        .chain(walk.frames.iter().map(|frame| frame.name.as_str()))
        .chain(std::iter::once(name.as_str()))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if judged.len() >= PATH_MAX {
        return Err(Errno::Nametoolong.into());
    }
    Ok(ResolvedPath {
        base: walk.top().clone(),
        path: name,
        judged,
        names_directory: names_directory(path),
    })
}

/// `found`, the directory reached at `dir`'s path, is still the one `dir`
/// opened, `NOENT` once another directory stands there, as a POSIX
/// descriptor never reaches another directory. A Mount's root never moves;
/// any other directory opened with no identity cannot be told from
/// another, so it is reached no more.
fn confirm(dir: &OpenDirectory, found: &dyn Filesystem) -> Answer {
    if dir.preopen {
        return Ok(());
    }
    let Some(identity) = dir.identity else {
        return Err(Errno::Noent.into());
    };
    match found.metadata("") {
        Ok(found) if Identity::of(&found) == Some(identity) => Ok(()),
        Ok(_) | Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
            Err(Errno::Noent.into())
        }
        Err(err) => Err(err.into()),
    }
}
