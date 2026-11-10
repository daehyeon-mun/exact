//! What a root that does not clip adds to the document: its border box and
//! every laid-out box below it, down to a box that clips or scrolls (whose
//! own box counts, not what it holds), as a browser's viewport scrolls the
//! root element's overflow (CSS Overflow 3 §2.2). The root's end padding is
//! not added: the viewport is the scroll container, not the root. A box in
//! another containing block, the top layer, an `svg`'s inside and the head
//! are elsewhere. Boxes are where layout put them: a transform below the root
//! is not projected (the presenter projects the root's own).
//!
//! Each box's share is kept from one layout to the next, so a layout that
//! moved one box reads that box and the boxes above it again, not the tree.
//! Shares are kept from the document's origin, where a root is laid out; a
//! root placed anywhere else is walked whole ([`walked`]).
use super::*;

/// The far corner of what a box adds: its own box and, where it neither
/// clips nor scrolls, what it holds.
type Corner = (f32, f32);

/// A box that adds nothing: one that is elsewhere, or empty and holding
/// nothing that counts. An empty box is skipped as Chromium's rectangle
/// union skips one.
const NOTHING: Corner = (f32::NEG_INFINITY, f32::NEG_INFINITY);

/// Each box's [`Corner`] as last found. A box with no entry is found again;
/// so is every box above it, down to which its entry was dropped.
#[derive(Default)]
pub(crate) struct Extents {
    known: IdMap<ViewId, Corner>,
    /// Boxes read since the host was made.
    #[cfg(test)]
    pub(crate) read: usize,
}

impl Extents {
    /// `id` moved, changed what it counts by (its overflow, its tag, its
    /// containing block) or changed its children: its share and that of each
    /// box above it are found again. The boxes above a box with no entry
    /// have none that counts it.
    pub(crate) fn changed(&mut self, kernel: &Kernel, id: ViewId) {
        let mut at = Some(id);
        while let Some(view) = at {
            if self.known.remove(&view).is_none() {
                break;
            }
            at = kernel.node(view).and_then(|n| n.parent);
        }
    }

    /// `id` is gone.
    pub(crate) fn forget(&mut self, id: ViewId) {
        self.known.remove(&id);
    }

    /// `root`'s document extent.
    pub(crate) fn document(&mut self, root: &NodeRef<'_>, kernel: &Kernel) -> (f32, f32) {
        if (root.frame.x, root.frame.y) != (0.0, 0.0) {
            return walked(root, kernel);
        }
        let (mut w, mut h) = (root.frame.width, root.frame.height);
        for child in root.children() {
            let (right, bottom) = self.corner(kernel, child);
            (w, h) = (w.max(right), h.max(bottom));
        }
        (w, h)
    }

    fn corner(&mut self, kernel: &Kernel, id: ViewId) -> Corner {
        if let Some(known) = self.known.get(&id) {
            return *known;
        }
        let Some(c) = kernel.node(id) else {
            return NOTHING;
        };
        #[cfg(test)]
        {
            self.read += 1;
        }
        let elsewhere = kernel.host_block(id).is_some()
            || c.node_type.is_svg_element()
            || c.node_type.is_metadata()
            || c.props.str(PropId::SemanticTag) == Some("dialog")
            || c.props.str(PropId::Popover).is_some();
        let mut corner = NOTHING;
        if !elsewhere {
            if c.frame.width > 0.0 && c.frame.height > 0.0 {
                corner = (c.frame.x + c.frame.width, c.frame.y + c.frame.height);
            }
            if style::effective_overflow(&c) == (Overflow::Visible, Overflow::Visible) {
                for child in c.children() {
                    let (right, bottom) = self.corner(kernel, child);
                    corner = (corner.0.max(right), corner.1.max(bottom));
                }
            }
        }
        self.known.insert(id, corner);
        corner
    }
}

/// The extent by a walk of every box below `root`.
pub(crate) fn walked(root: &NodeRef<'_>, kernel: &Kernel) -> (f32, f32) {
    let (x0, y0) = (root.frame.x, root.frame.y);
    let (mut w, mut h) = (root.frame.width, root.frame.height);
    let mut below = root.children();
    while let Some(id) = below.pop() {
        let Some(c) = kernel.node(id) else { continue };
        let elsewhere = kernel.host_block(id).is_some()
            || c.node_type.is_svg_element()
            || c.node_type.is_metadata()
            || c.props.str(PropId::SemanticTag) == Some("dialog")
            || c.props.str(PropId::Popover).is_some();
        if elsewhere {
            continue;
        }
        if c.frame.width > 0.0 && c.frame.height > 0.0 {
            w = w.max(c.frame.x - x0 + c.frame.width);
            h = h.max(c.frame.y - y0 + c.frame.height);
        }
        if style::effective_overflow(&c) == (Overflow::Visible, Overflow::Visible) {
            below.extend(c.children());
        }
    }
    (w, h)
}
