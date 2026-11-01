//! Holds calls racing on one shared instance to the Linux they align with. No
//! interleaving repeats, so answers are compared as sets: what kobako answers a
//! kind of call, Linux must have answered it too over the same sequences. What
//! stands once every call returned is held to rules any interleaving keeps.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Barrier;
use std::thread;

use crate::calls::{Call, Rng, SLOTS};
use crate::kobako::Kobako;
use crate::linux::Linux;
use crate::Snapshot;

/// How many threads race, each with descriptors of its own.
pub const THREADS: usize = 4;

/// What a sequence races over: plain names, so a path names one entry whichever
/// descriptor or link would otherwise lead there.
const PATHS: [&str; 12] = [
    "a", "b", "c", "d", "d/f", "d/g", "d/e", "d/e/h", "m", "m/s", "m/s/t", "m/x",
];

/// One side of the comparison, one instance per racing thread.
pub trait Side: Send {
    fn run(&mut self, call: &Call) -> String;
    fn snapshot(&self) -> Snapshot;
}

impl Side for Kobako {
    fn run(&mut self, call: &Call) -> String {
        Kobako::run(self, call)
    }
    fn snapshot(&self) -> Snapshot {
        Kobako::snapshot(self)
    }
}

impl Side for Linux {
    fn run(&mut self, call: &Call) -> String {
        Linux::run(self, call)
    }
    fn snapshot(&self) -> Snapshot {
        Linux::snapshot(self)
    }
}

/// One call as it happened, between two stamps of one clock all threads share.
struct Event {
    call: Call,
    answer: String,
    began: u64,
    ended: u64,
}

impl Event {
    fn succeeded(&self) -> bool {
        self.answer.starts_with("ok")
    }

    /// The name this call removed.
    fn removed(&self) -> Option<&str> {
        match &self.call {
            Call::Unlink(_, path) | Call::Rmdir(_, path) if self.succeeded() => Some(path),
            _ => None,
        }
    }

    /// Whether this call may have put an entry at or beneath `name`.
    fn may_make(&self, name: &str) -> bool {
        let related = |path: &str| {
            path == name
                || path.starts_with(&format!("{name}/"))
                || name.starts_with(&format!("{path}/"))
        };
        if !self.succeeded() {
            return false;
        }
        match &self.call {
            Call::Open(open) => open.create && related(&open.path),
            Call::Mkdir(_, path) => related(path),
            Call::Rename(_, from, to) => related(from) || related(to),
            _ => false,
        }
    }
}

/// A call and its answer by what it says of the call, not its data: a read is
/// only `ok`, a stat only the kind it found, and what an open asked is kept where
/// it decides the answer.
fn class(event: &Event) -> String {
    let answer = event.answer.split([':', ' ']).next().unwrap_or("");
    let asked = match &event.call {
        Call::Open(open) => format!(" create={} exclusive={}", open.create, open.exclusive),
        _ => String::new(),
    };
    format!("{}{asked} {answer}", event.call.kind())
}

/// One thread's sequence.
fn sequence(seed: u64, thread: usize, longest: u64) -> Vec<Call> {
    let paths = PATHS.map(str::to_string);
    let mut rng = Rng::new(seed.wrapping_mul(31).wrapping_add(thread as u64 + 1));
    let length = 1 + rng.below(longest);
    (0..length)
        .map(|_| {
            let mut call = rng.call(&paths);
            match &mut call {
                Call::Open(open) => open.base = None,
                Call::Stat { base, .. }
                | Call::Mkdir(base, _)
                | Call::Rmdir(base, _)
                | Call::Unlink(base, _)
                | Call::Rename(base, ..) => *base = None,
                _ => {}
            }
            call
        })
        .collect()
}

/// Race `sequences` over one side, one thread each, answering what each call did
/// and the thread's side to look at afterwards.
fn race<S: Side>(sides: Vec<S>, sequences: &[Vec<Call>]) -> (Vec<S>, Vec<Event>) {
    let clock = AtomicU64::new(0);
    let start = Barrier::new(sides.len());
    let (mut left, mut events) = (Vec::new(), Vec::new());
    thread::scope(|scope| {
        let threads: Vec<_> = sides
            .into_iter()
            .zip(sequences)
            .map(|(mut side, calls)| {
                let (clock, start) = (&clock, &start);
                scope.spawn(move || {
                    start.wait();
                    let mut events = Vec::with_capacity(calls.len());
                    for call in calls {
                        let began = clock.fetch_add(1, Ordering::SeqCst);
                        let answer = side.run(call);
                        events.push(Event {
                            call: call.clone(),
                            answer,
                            began,
                            ended: clock.fetch_add(1, Ordering::SeqCst),
                        });
                    }
                    (side, events)
                })
            })
            .collect();
        for thread in threads {
            let (side, done) = thread.join().expect("a racing thread must not panic");
            left.push(side);
            events.extend(done);
        }
    });
    (left, events)
}

/// What no interleaving allows of what stands once every call returned: an entry
/// its own stat refuses, a descriptor that no longer answers, and a name removed
/// that stands with nothing having made it since the removal began.
fn broken_rules<S: Side>(sides: &mut [S], events: &[Event]) -> BTreeSet<String> {
    let mut broken = BTreeSet::new();
    for side in sides.iter_mut() {
        for slot in 0..SLOTS {
            let answer = side.run(&Call::Fstat(slot));
            if answer.starts_with('E') && answer != "EBADF" {
                broken.insert(format!("a held descriptor answers {answer} at rest"));
            }
        }
    }
    let tree = sides[0].snapshot();
    for (path, seen) in &tree {
        if seen.starts_with('E') || seen.contains("unreadable") {
            broken.insert(format!("{path} is listed but answers {seen}"));
        }
    }
    for removal in events {
        let Some(name) = removal.removed() else {
            continue;
        };
        let remade = events
            .iter()
            .any(|event| event.ended >= removal.began && event.may_make(name));
        let stands = tree
            .keys()
            .find(|path| *path == name || path.starts_with(&format!("{name}/")));
        if let (false, Some(path)) = (remade, stands) {
            broken.insert(format!(
                "{path} stands after its removal, with nothing making it"
            ));
        }
    }
    broken
}

/// What one setup found over its rounds.
#[derive(Default)]
pub struct Found {
    pub kobako: BTreeSet<String>,
    pub linux: BTreeSet<String>,
    pub broken_by_kobako: BTreeSet<String>,
    pub broken_by_linux: BTreeSet<String>,
}

/// Race one round's sequences through a side of each.
pub fn round(
    seed: u64,
    longest: u64,
    kobako: Vec<Kobako>,
    linux: Vec<Linux>,
    found: &mut Found,
) -> (Vec<Kobako>, Vec<Linux>) {
    let sequences: Vec<Vec<Call>> = (0..kobako.len())
        .map(|thread| sequence(seed, thread, longest))
        .collect();
    let (mut kobako, ours) = race(kobako, &sequences);
    found
        .broken_by_kobako
        .extend(broken_rules(&mut kobako, &ours));
    found.kobako.extend(ours.iter().map(class));
    let (mut linux, theirs) = race(linux, &sequences);
    found
        .broken_by_linux
        .extend(broken_rules(&mut linux, &theirs));
    found.linux.extend(theirs.iter().map(class));
    (kobako, linux)
}
