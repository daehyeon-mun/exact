//! What a commit, or a host's fact, changed for the native controls a
//! layout prepares (`layout/controls.rs`).
use super::*;
use crate::id::IdSet;

impl Kernel {
    /// The nodes `receipt` created or touched: the control each is, or is in
    /// the subtree of, is asked again at the next layout. A touched node
    /// names every change of style, props or children, and what a node
    /// inherits is touched with it (LLP 1035.000 D4).
    pub(super) fn controls_changed(&mut self, receipt: &CommitReceipt) {
        let Some(tree) = self.layout.as_deref_mut().and_then(|l| l.tree()) else {
            return;
        };
        if !tree.tracks_controls() {
            return;
        }
        // The boxes found with no control at or above them: walked once.
        let mut clear = IdSet::default();
        let lists = self.arena.has_prop(PropId::ListStyle);
        for key in receipt.touched.iter().chain(&receipt.created) {
            let Some(slot) = self.arena.resolve(*key) else {
                continue;
            };
            changed_under_control(&self.arena, tree, slot, &mut clear);
            if lists && decides_cells(&self.arena, slot) {
                if let Some(node) = self.arena.taffy(slot) {
                    tree.touch_controls_under(node);
                }
            }
        }
    }

    /// A host's fact about `slot` changed outside a commit: its text's
    /// metrics, or its natural size.
    pub(super) fn control_changed_at(&mut self, slot: u32) {
        if let Some(tree) = self.layout.as_deref_mut().and_then(|l| l.tree()) {
            if tree.tracks_controls() {
                changed_under_control(&self.arena, tree, slot, &mut IdSet::default());
            }
        }
    }

    /// A host's fact every control may read changed outside a commit.
    pub(super) fn controls_all_changed(&mut self) {
        if let Some(tree) = self.layout.as_deref_mut().and_then(|l| l.tree()) {
            tree.touch_controls();
        }
    }
}

/// Whether `slot` is a grouped list or one of its rows' groups: its style,
/// tag and ground decide whether each field below is its cell's
/// (`NodeArena::is_cell_field`).
fn decides_cells(arena: &NodeArena, slot: u32) -> bool {
    let styled = |s: u32| arena.props(s).str(PropId::ListStyle).is_some();
    let list = |group: u32| {
        arena
            .parent(group)
            .and_then(|section| arena.parent(section))
    };
    styled(slot) || list(slot).is_some_and(styled)
}

/// Mark the control `slot` is, or is in the subtree of.
fn changed_under_control(
    arena: &NodeArena,
    tree: &mut LayoutTree,
    slot: u32,
    clear: &mut IdSet<u32>,
) {
    let mut path = Vec::new();
    let mut at = Some(slot);
    while let Some(s) = at.filter(|s| !clear.contains(s)) {
        if matches!(arena.node_type(s), NodeType::Control | NodeType::TextInput) {
            if let Some(node) = arena.taffy(s) {
                tree.touch_control(node);
            }
            return;
        }
        path.push(s);
        at = arena.parent(s);
    }
    clear.extend(path);
}
