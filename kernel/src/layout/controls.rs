//! Which native controls a layout prepares. LLP 1104 D5, LLP 1069.011.001 D11.
//!
//! Before the engine runs, a native text field's chrome and a native
//! button's measure are asked of the host (`fields.rs`, `buttons.rs`). An
//! answer depends on the control's own subtree, on what that inherits, on a
//! few facts of the boxes above it (their `display`, a `menu` or `tablist`
//! role) and on the host's measuring traits, so it stands until one of those
//! changes. This is the record of which controls may have changed since
//! they were prepared: a layout prepares those, not every control of the
//! tree, and does nothing here when none did.
//!
//! A control becomes stale when its node is made or its engine style
//! written, when a commit touches a node of its subtree
//! (`Kernel::controls_changed`), when a box above it takes another parent or
//! another role or is a grouped list's that a commit touches, and with every
//! other control when the environment, the root font size or the host's
//! measure revision changes: a host whose answer to the same request would
//! change says so by that revision ([`TextMeasurer::measure_revision`]).
//! A stale control a layout cannot reach, hidden or outside the root laid
//! out, waits under the box that kept the layout from it.
use super::*;

#[derive(Clone, Default)]
pub(super) struct Controls {
    /// Whether `all` and `roles` hold the whole tree. A tree made whole (a
    /// rebuild, a trial's subtrees) is read once, at its first layout.
    indexed: bool,
    /// The engine nodes of every `Control` and `TextInput`.
    all: IdSet<NodeId>,
    /// Those to prepare at the next layout that reaches them.
    stale: IdSet<NodeId>,
    /// Stale controls under a hidden box, by that box: they wait for its
    /// style to change.
    hidden: IdMap<NodeId, Vec<NodeId>>,
    /// Stale controls in a tree other than the one laid out, by its top:
    /// they wait for it to be laid out, or to take a parent.
    elsewhere: IdMap<NodeId, Vec<NodeId>>,
    /// Boxes with a new parent or a new role since the last layout: every
    /// control under one is stale.
    moved: IdSet<NodeId>,
    /// The boxes whose role a control below reads, with which.
    roles: IdMap<NodeId, u8>,
    /// What every answer depends on, as of the last layout: the
    /// environment, the host's measure revision and the root font size.
    env: Option<crate::Env>,
    revision: u64,
    root_font: f32,
    /// Controls prepared since the tree was made.
    #[cfg(test)]
    pub(super) prepared: usize,
}

fn is_control(arena: &NodeArena, slot: u32) -> bool {
    matches!(
        arena.node_type(slot),
        NodeType::Control | NodeType::TextInput
    )
}

/// The role of `slot` that a native button below it reads
/// (`NodeArena::neutral_role_floor`, `NodeArena::is_neutral_tab`).
fn role(arena: &NodeArena, slot: u32) -> u8 {
    match arena.props(slot).str(crate::PropId::AccessibilityRole) {
        Some("menu") => 1,
        Some("tablist") => 2,
        _ => 0,
    }
}

impl Controls {
    fn note(&mut self, arena: &NodeArena, slot: u32, node: NodeId) {
        if is_control(arena, slot) {
            self.all.insert(node);
            self.stale.insert(node);
        }
        let now = role(arena, slot);
        if now == 0 && self.roles.is_empty() {
            return;
        }
        let was = match now {
            0 => self.roles.remove(&node),
            _ => self.roles.insert(node, now),
        };
        if was.unwrap_or(0) != now {
            self.moved.insert(node);
        }
    }
}

impl LayoutTree {
    /// `node` was made for `slot`, or its engine style derived again, which
    /// follows every change of its type, role or layout rows.
    pub(super) fn note_control(&mut self, arena: &NodeArena, slot: u32, node: NodeId) {
        self.controls.note(arena, slot, node);
    }

    /// Something `node`'s answer may depend on changed, if it is a control's.
    pub(crate) fn touch_control(&mut self, node: NodeId) {
        if self.controls.all.contains(&node) {
            self.controls.stale.insert(node);
        }
    }

    /// Something every answer may depend on changed.
    pub(crate) fn touch_controls(&mut self) {
        let controls = &mut self.controls;
        controls.stale.extend(controls.all.iter().copied());
        controls.hidden.clear();
        controls.elsewhere.clear();
    }

    /// Something every control under `node` may depend on changed.
    pub(crate) fn touch_controls_under(&mut self, node: NodeId) {
        self.controls.moved.insert(node);
    }

    /// Whether a change is worth reporting: the tree holds a control, and
    /// has been read (its first layout prepares every control).
    pub(crate) fn tracks_controls(&self) -> bool {
        self.controls.indexed && !self.controls.all.is_empty()
    }

    /// `node` is removed: a control, a box that moved, or one controls
    /// waited under.
    pub(super) fn forget_control(&mut self, node: NodeId) {
        let controls = &mut self.controls;
        let waiting = controls.hidden.is_empty() && controls.elsewhere.is_empty();
        let idle = controls.all.is_empty() && controls.roles.is_empty();
        if idle && waiting && controls.moved.is_empty() {
            return;
        }
        controls.moved.remove(&node);
        controls.all.remove(&node);
        controls.stale.remove(&node);
        controls.hidden.remove(&node);
        controls.elsewhere.remove(&node);
        controls.roles.remove(&node);
    }

    /// `node`'s engine style was written. A field's is derived from its
    /// chrome again, whoever wrote it (a restyle, a height a transition
    /// presents, a what-if measure); under a box that was hidden, what
    /// waited is looked at again.
    pub(super) fn restyled_control(&mut self, node: NodeId) {
        self.touch_control(node);
        if self.controls.hidden.is_empty() {
            return;
        }
        if let Some(waiting) = self.controls.hidden.remove(&node) {
            self.controls.stale.extend(waiting);
        }
    }

    /// Write the engine style `node`'s preparation derived: no change to
    /// prepare it for again.
    pub(super) fn set_prepared_style(&mut self, node: NodeId, style: taffy::Style) {
        let again = self.controls.stale.contains(&node);
        self.set_style(node, style);
        if !again {
            self.controls.stale.remove(&node);
        }
    }

    /// `children` become `parent`'s: under one that arrives, every control
    /// has other boxes above it now.
    pub(super) fn children_above_controls(&mut self, parent: NodeId, children: &[NodeId]) {
        if !self.controls.indexed || self.controls.all.is_empty() {
            return;
        }
        for &child in children {
            if self.taffy.parent(child) == Some(parent) {
                continue;
            }
            self.controls.elsewhere.remove(&child);
            // A leaf that is no control, or one already stale, has nothing
            // below it to find.
            let holds = self.taffy.child_count(child) > 0;
            let fresh = self.controls.stale.contains(&child);
            if holds || (self.controls.all.contains(&child) && !fresh) {
                self.controls.moved.insert(child);
            }
        }
    }

    /// The stale controls a layout of `root` reaches, each with its slot,
    /// taken off the record: preparing one that must be asked again puts it
    /// back ([`LayoutTree::ask_control_again`]). The others wait.
    pub(super) fn stale_controls(
        &mut self,
        root: NodeId,
        arena: &NodeArena,
        revision: u64,
    ) -> Vec<(NodeId, u32)> {
        let mut controls = std::mem::take(&mut self.controls);
        if !controls.indexed {
            controls.indexed = true;
            for (&node, &slot) in &self.slots {
                controls.note(arena, slot, node);
            }
            controls.moved.clear();
        }
        if controls.all.is_empty() {
            controls.moved.clear();
            self.controls = controls;
            return Vec::new();
        }
        let root_font = arena.root_font_size();
        if controls.env.as_ref() != Some(arena.env())
            || controls.revision != revision
            || controls.root_font.to_bits() != root_font.to_bits()
        {
            controls.env = Some(arena.env().clone());
            (controls.revision, controls.root_font) = (revision, root_font);
            controls.stale.extend(controls.all.iter().copied());
            controls.hidden.clear();
            controls.elsewhere.clear();
        }
        if !controls.moved.is_empty() {
            // Each is live: a box removed since it moved was dropped then.
            let mut below: Vec<NodeId> = controls.moved.drain().collect();
            let mut seen = IdSet::default();
            while let Some(node) = below.pop() {
                if !seen.insert(node) {
                    continue;
                }
                if controls.all.contains(&node) {
                    controls.stale.insert(node);
                }
                below.extend(self.taffy.child_ids(node));
            }
        }
        if let Some(waiting) = controls.elsewhere.remove(&root) {
            controls.stale.extend(waiting);
        }
        let mut ready = Vec::new();
        for node in std::mem::take(&mut controls.stale) {
            // One that waited under two boxes was removed since.
            let Some(&slot) = self
                .slots
                .get(&node)
                .filter(|_| controls.all.contains(&node))
            else {
                continue;
            };
            let (mut at, mut top) = (Some(node), node);
            let kept = loop {
                let Some(n) = at else {
                    break Some((&mut controls.elsewhere, top));
                };
                let shown = |s: &taffy::Style| s.display != taffy::Display::None;
                if !self.taffy.style(n).is_ok_and(shown) {
                    break Some((&mut controls.hidden, n));
                }
                if n == root {
                    break None;
                }
                (at, top) = (self.taffy.parent(n), n);
            };
            match kept {
                Some((waiting, under)) => waiting.entry(under).or_default().push(node),
                None => ready.push((node, slot)),
            }
        }
        #[cfg(test)]
        {
            controls.prepared += ready.len();
        }
        self.controls = controls;
        ready
    }

    /// `node`'s answer was a stand-in, or none: it is asked again at the
    /// next layout.
    pub(super) fn ask_control_again(&mut self, node: NodeId) {
        self.controls.stale.insert(node);
    }

    /// An answer was refused: none of `ready` was prepared.
    pub(super) fn ask_controls_again(&mut self, ready: &[(NodeId, u32)]) {
        self.controls.stale.extend(ready.iter().map(|r| r.0));
    }

    /// Controls prepared since the tree was made.
    #[cfg(test)]
    pub(crate) fn controls_prepared(&self) -> usize {
        self.controls.prepared
    }
}
