//! Presentation retained while a subtree is detached, and the last paragraph sent.
use super::*;

#[derive(Debug, Default)]
pub(crate) struct RetainedPresentation {
    held: IdSet<ViewId>,
    connected: IdMap<ViewId, bool>,
    runs: IdMap<ViewId, String>,
}

impl<D: DataSource> Host<D> {
    /// A commit's start: the tree may have moved since the last one.
    pub(super) fn retained_begin(&mut self) {
        self.retained.connected.clear();
    }

    /// Whether `id` is out of every root, in which case its presentation is held.
    pub(super) fn retained_hold(&mut self, id: ViewId) -> bool {
        if self.retained_connected(id) {
            return false;
        }
        self.retained.held.insert(id);
        true
    }

    fn retained_connected(&mut self, id: ViewId) -> bool {
        let kernel = self.runner.kernel();
        let mut path = Vec::new();
        let mut at = Some(id);
        let mut answer = false;
        while let Some(v) = at {
            if let Some(known) = self.retained.connected.get(&v) {
                answer = *known;
                break;
            }
            let Some(node) = kernel.node(v) else {
                break;
            };
            path.push(v);
            if node.is_root {
                answer = true;
                break;
            }
            at = node.parent;
        }
        for v in path {
            self.retained.connected.insert(v, answer);
        }
        answer
    }

    /// The held nodes a root reaches again, updated as touched ones are; the caller sends their children.
    pub(super) fn retained_release(&mut self, batch: &mut Batch) -> Vec<ViewId> {
        if self.retained.held.is_empty() {
            return Vec::new();
        }
        let held: Vec<ViewId> = self.retained.held.iter().copied().collect();
        let mut back = Vec::new();
        for id in held {
            if self.runner.kernel().node(id).is_none() {
                self.retained.held.remove(&id);
            } else if self.retained_connected(id) {
                self.retained.held.remove(&id);
                back.push(id);
            }
        }
        back.sort_unstable();
        for id in &back {
            self.update(*id, batch);
        }
        back
    }

    /// Whether these runs are what the presenter last heard for `owner`.
    pub(super) fn retained_same_runs(&mut self, owner: ViewId, runs: &str) -> bool {
        if self.retained.runs.get(&owner).is_some_and(|r| r == runs) {
            return true;
        }
        self.retained.runs.insert(owner, runs.to_owned());
        false
    }

    /// A view made or gone: nothing it was told carries over.
    pub(super) fn retained_forget(&mut self, id: ViewId) {
        self.retained.runs.remove(&id);
        self.retained.held.remove(&id);
    }
}
