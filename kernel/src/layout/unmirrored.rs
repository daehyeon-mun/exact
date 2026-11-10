//! The engine of a kernel that mirrors none.
use super::*;

/// A commit's engine when the kernel mirrors none: new nodes get no engine
/// node, so no other call reaches it.
pub struct Unmirrored;

impl LayoutMirror for Unmirrored {
    fn new_leaf(&mut self, _: &NodeArena, _: u32, _: bool) -> Option<NodeId> {
        None
    }

    fn new_leaf_unstyled(&mut self, _: u32, _: bool) -> Option<NodeId> {
        None
    }

    fn restyle(&mut self, _: &NodeArena, _: u32, _: NodeId) {}

    fn sync_children(&mut self, _: &NodeArena, _: u32, _: NodeId) -> usize {
        0
    }

    fn remove(&mut self, _: NodeId) {}

    fn mark_dirty(&mut self, _: NodeId) {}

    #[cfg(test)]
    fn attached(&self, _: NodeId) -> bool {
        false
    }

    fn tree(&mut self) -> Option<&mut LayoutTree> {
        None
    }

    fn tree_ref(&self) -> Option<&LayoutTree> {
        None
    }
}
