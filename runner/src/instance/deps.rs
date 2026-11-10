//! The dependency table: what every binding, site, derive and resource
//! argument reads, scanned once from the plan when a runner boots.
//!
//! @ref LLP 1005 §8 (the Deps table and dirty-set sweep)
//!
//! An input is a root slot, a derive, a resource, a status flag or the
//! clock: one bit each in [`Bits`]. What a code body reads of its enclosing
//! scopes is a mask of relative frame depths (bit 0 is the innermost frame in
//! force where it runs; bit 63 stands for every frame from there out). A row
//! slot is a slot bit too, but its value lives in one row: [`RowWrites`]
//! names the rows an action wrote. A site's reads are the union over its subtree,
//! with frame masks shifted out of the scopes the subtree introduces, so an
//! update can skip a site when nothing its subtree reads has changed.

use crate::held::Held;
use crate::vm::{self, Env, RowSlots};
use exact_plan::{Code, Opcode, Plan, Stdlib, Value};

/// Nonzero input words in ascending order. Expanded plans have high slot
/// indices but each binding reads few of them; zero prefixes waste work in
/// construction, interning and every later union.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Bits(Vec<(usize, u64)>);

impl Bits {
    pub(crate) fn set(&mut self, i: usize) {
        let word = i / 64;
        let bit = 1 << (i % 64);
        match self.0.binary_search_by_key(&word, |(at, _)| *at) {
            Ok(at) => self.0[at].1 |= bit,
            Err(at) => self.0.insert(at, (word, bit)),
        }
    }
    pub(crate) fn get(&self, i: usize) -> bool {
        self.0
            .binary_search_by_key(&(i / 64), |(at, _)| *at)
            .is_ok_and(|at| self.0[at].1 & (1 << (i % 64)) != 0)
    }
    pub(crate) fn union(&mut self, other: &Bits) {
        let mut at = 0;
        for &(word, bits) in &other.0 {
            while at < self.0.len() && self.0[at].0 < word {
                at += 1;
            }
            if at < self.0.len() && self.0[at].0 == word {
                self.0[at].1 |= bits;
            } else {
                self.0.insert(at, (word, bits));
            }
            at += 1;
        }
    }
    pub(crate) fn intersects(&self, other: &Bits) -> bool {
        let (mut a, mut b) = (0, 0);
        while a < self.0.len() && b < other.0.len() {
            let (aw, ab) = self.0[a];
            let (bw, bb) = other.0[b];
            match aw.cmp(&bw) {
                std::cmp::Ordering::Less => a += 1,
                std::cmp::Ordering::Greater => b += 1,
                std::cmp::Ordering::Equal => {
                    if ab & bb != 0 {
                        return true;
                    }
                    a += 1;
                    b += 1;
                }
            }
        }
        false
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The set bits, ascending.
    pub(crate) fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().flat_map(|&(word, bits)| {
            let mut rest = bits;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let bit = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                Some(word * 64 + bit)
            })
        })
    }
}

/// Where each kind of input sits in [`Bits`].
#[derive(Debug, Clone, Copy)]
struct Layout {
    slots: usize,
    derives: usize,
    resources: usize,
    mutations: usize,
}

impl Layout {
    fn derive(&self, i: usize) -> usize {
        self.slots + i
    }
    fn resource(&self, i: usize) -> usize {
        self.slots + self.derives + i
    }
    fn pending_resource(&self, i: usize) -> usize {
        self.slots + self.derives + self.resources + i
    }
    fn pending_mutation(&self, i: usize) -> usize {
        self.slots + self.derives + 2 * self.resources + i
    }
    fn failed_resource(&self, i: usize) -> usize {
        self.slots + self.derives + 2 * self.resources + self.mutations + i
    }
    fn clock(&self) -> usize {
        self.slots + self.derives + 3 * self.resources + self.mutations
    }
    fn width(&self) -> usize {
        self.clock() + 1
    }
}

/// What one code body, or the union over a subtree, reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Reads {
    /// Inputs, by [`Layout`].
    pub(crate) bits: Bits,
    /// Enclosing frames read, by relative depth (bit 63: that far or farther).
    pub(crate) frames: u64,
    /// The fields of the innermost frame's value read (bit `k` for field
    /// `k`, bit 63 for 63 and past; every bit when read other than one
    /// field at a time): LLP 1017.003 D6. Meaningful with bit 0 of `frames`.
    pub(crate) fields: u64,
    /// Enclosing frames whose POSITION is read (`each item, i in list`,
    /// LoadIndex), by relative depth as `frames`: bit 0 says a row's body
    /// reads its own position, at any depth, so a row that moved must
    /// re-evaluate (LLP 1062 D8); one that reads none keeps its bindings.
    pub(crate) positions: u64,
    /// Whether any row slot is among `bits`.
    pub(crate) row_slots: bool,
    /// Reads a parameter or has an effect: never skipped.
    pub(crate) opaque: bool,
}

impl Reads {
    /// Reads no input, frame, row slot or parameter: the same value always.
    pub(crate) fn is_constant(&self) -> bool {
        !self.opaque && !self.row_slots && self.frames == 0 && self.bits.is_empty()
    }

    fn union(&mut self, other: &Reads, shift: u32) {
        self.bits.union(&other.bits);
        let frames = out_of(other.frames, shift);
        if shift == 0 {
            self.fields |= other.fields;
        } else if frames & 1 != 0 {
            // A scope further in read this frame as an outer one: which
            // fields is not recorded that far out.
            self.fields = !0;
        }
        self.frames |= frames;
        self.positions |= out_of(other.positions, shift);
        self.row_slots |= other.row_slots;
        self.opaque |= other.opaque;
    }
    /// Reads nothing that can change: the value it had at creation stands.
    #[cfg(test)]
    pub(crate) fn constant(&self) -> bool {
        self.bits.is_empty() && self.frames == 0 && !self.opaque
    }
    /// Whether these reads, a row's body's, read the row's own position:
    /// opaque reads count, since a scan that stops at code it can't decode
    /// knows no more of them.
    pub(crate) fn reads_position(&self) -> bool {
        self.positions & 1 != 0 || self.opaque
    }
    /// The same reads seen from `shift` scopes further out.
    pub(crate) fn frames_outside(&self, shift: u32) -> u64 {
        out_of(self.frames, shift)
    }
}

/// A frame mask seen from `shift` scopes further out. Bit 63 means "that far
/// or farther", so it spreads over every depth it may now stand for.
fn out_of(frames: u64, shift: u32) -> u64 {
    if shift == 0 {
        return frames;
    }
    let shift = shift.min(63);
    let far = if frames >> 63 != 0 {
        !0 << (63 - shift)
    } else {
        0
    };
    (frames >> shift) | far
}

/// A dirty-frame mask seen from one scope further in, whose own frame is
/// `dirty`; bit 63 stays set once set.
pub(crate) fn into_scope(frames: u64, dirty: bool) -> u64 {
    (frames << 1) | (frames & (1 << 63)) | u64::from(dirty)
}

/// Read sets by index, each distinct set held once: most of a plan's
/// bindings are literal rows, which read nothing, and most of the rest read
/// what another reads (euxy: 5,651 bindings, 85 distinct sets).
#[derive(Debug, Default)]
pub(crate) struct ReadsTable {
    /// Each index's place in `reads`; 0 is the empty set.
    at: Vec<u32>,
    reads: Vec<Reads>,
}

impl FromIterator<Reads> for ReadsTable {
    fn from_iter<I: IntoIterator<Item = Reads>>(all: I) -> Self {
        let all = all.into_iter();
        let mut at = Vec::with_capacity(all.size_hint().0);
        let mut places: std::collections::HashMap<Reads, u32> = Default::default();
        for reads in all {
            let place = match reads == Reads::default() {
                true => 0,
                false => {
                    let next = places.len() as u32 + 1;
                    *places.entry(reads).or_insert(next)
                }
            };
            at.push(place);
        }
        let mut reads = vec![Reads::default(); places.len() + 1];
        for (set, place) in places {
            reads[place as usize] = set;
        }
        ReadsTable { at, reads }
    }
}

#[cfg(test)]
impl ReadsTable {
    /// Index `i`'s reads as a scan that stopped at undecodable code leaves
    /// them (`opaque`), for a test of a body nothing more is known of.
    pub(crate) fn set_opaque(&mut self, i: usize) {
        let mut reads = self[i].clone();
        reads.opaque = true;
        self.reads.push(reads);
        self.at[i] = (self.reads.len() - 1) as u32;
    }
}

impl std::ops::Index<usize> for ReadsTable {
    type Output = Reads;
    fn index(&self, i: usize) -> &Reads {
        &self.reads[self.at[i] as usize]
    }
}

/// Per binding, surface argument, site, derive and resource argument list.
#[derive(Debug, Default)]
pub struct Deps {
    layout: Option<Layout>,
    pub(crate) bindings: ReadsTable,
    pub(crate) surface_args: ReadsTable,
    /// A node's own reads and every descendant's, relative to the node.
    pub(crate) nodes: ReadsTable,
    /// A region's subject, relative to the region.
    pub(crate) subjects: ReadsTable,
    /// A region's key, relative to the row (bit 0 is the item).
    pub(crate) keys: ReadsTable,
    /// Exactly the innermost row's position; identities survive item changes.
    pub(crate) index_keys: Vec<bool>,
    /// A region's arms' sites, relative to the row or arm frame.
    pub(crate) bodies: ReadsTable,
    /// Subject, key and body, relative to the region.
    pub(crate) regions: ReadsTable,
    /// Each derive's body.
    pub(crate) derives: Vec<Reads>,
    /// Each resource's arguments together.
    pub(crate) resource_args: Vec<Reads>,
}

/// One input a settlement body reads, decoded from [`Reads::bits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A root or row-owned state slot.
    Slot(usize),
    /// A settled derive value.
    Derive(usize),
    /// A resource's settled value.
    Resource(usize),
    /// A resource's pending flag.
    PendingResource(usize),
    /// A mutation's pending flag and failure state.
    PendingMutation(usize),
    /// A resource's failure state.
    FailedResource(usize),
    /// The evaluation clock.
    Clock,
}

/// A region body's dependencies shared by the native and JavaScript list engines.
#[derive(Debug)]
pub struct RowDependencies {
    /// Plan inputs read anywhere in the row's subtree.
    pub inputs: Vec<Input>,
    /// Enclosing frames, excluding the row itself (bit 63 includes every greater depth).
    pub outer_frames: u64,
    /// Reads whose inputs cannot be represented by the dependency table.
    pub opaque: bool,
}

impl Deps {
    pub(super) fn row_dependencies(&self, region: usize) -> RowDependencies {
        let reads = &self.bodies[region];
        RowDependencies {
            inputs: self.inputs(reads).collect(),
            outer_frames: reads.frames_outside(1),
            opaque: reads.opaque,
        }
    }

    /// Scan every body once and fold sites bottom-up over `children`.
    pub(crate) fn new(plan: &Plan, children: &super::SiteIndex) -> Self {
        let layout = Layout {
            slots: plan.slots.len(),
            derives: plan.derives.len(),
            resources: plan.resources.len(),
            mutations: plan.mutations.len(),
        };
        let scan = |code: Code| scan(plan, layout, code);
        let mut deps = Deps {
            layout: Some(layout),
            bindings: by_code(plan.bindings.iter().map(|b| b.expr), &scan),
            surface_args: plan.surface_args.iter().map(|a| scan(a.expr)).collect(),
            nodes: ReadsTable::default(),
            subjects: by_code(plan.regions.iter().map(|r| r.subject), &scan),
            keys: by_code(plan.regions.iter().map(|r| r.key), &scan),
            index_keys: plan
                .regions
                .iter()
                .map(|r| is_index_key(plan.code(r.key)))
                .collect(),
            bodies: ReadsTable::default(),
            regions: ReadsTable::default(),
            derives: plan.derives.iter().map(|d| scan(d.body)).collect(),
            resource_args: plan
                .resources
                .iter()
                .map(|r| {
                    let mut reads = Reads::default();
                    for a in r.args.iter() {
                        reads.union(&scan(plan.arg(a).expr), 0);
                    }
                    reads
                })
                .collect(),
        };
        // Post-order over the site tree without recursing to the plan's depth;
        // the sites' reads are tabled once every one is known.
        let mut nodes = vec![Reads::default(); plan.nodes.len()];
        let mut bodies = vec![Reads::default(); plan.regions.len()];
        let mut regions = vec![Reads::default(); plan.regions.len()];
        let mut stack: Vec<(super::Site, bool)> = children
            .children(None, None)
            .iter()
            .map(|(_, s)| (*s, false))
            .collect();
        while let Some((site, done)) = stack.pop() {
            let kids = |site| {
                let (direct, arms) = match site {
                    super::Site::Node(n) => (children.children(Some(n), plan.node(n).arm), None),
                    super::Site::Region(r) => (&[][..], Some(plan.region(r).arms)),
                };
                direct.iter().copied().chain(
                    arms.into_iter()
                        .flat_map(|arms| arms.iter())
                        .flat_map(|arm| children.children(None, Some(arm)).iter().copied()),
                )
            };
            if !done {
                stack.push((site, true));
                stack.extend(kids(site).map(|(_, s)| (s, false)));
                continue;
            }
            let of = |site| match site {
                super::Site::Node(n) => &nodes[n.0 as usize],
                super::Site::Region(r) => &regions[r.0 as usize],
            };
            match site {
                super::Site::Node(n) => {
                    let node = plan.node(n);
                    let mut reads = Reads::default();
                    for b in node.bindings.iter() {
                        reads.union(&deps.bindings[b.0 as usize], 0);
                    }
                    if let Some(surface) = node.surface {
                        for a in plan.surface(surface).args.iter() {
                            reads.union(&deps.surface_args[a.0 as usize], 0);
                        }
                    }
                    for (_, kid) in kids(site) {
                        reads.union(of(kid), 0);
                    }
                    nodes[n.0 as usize] = reads;
                }
                super::Site::Region(r) => {
                    let i = r.0 as usize;
                    let mut body = Reads::default();
                    for (_, kid) in kids(site) {
                        body.union(of(kid), 0);
                    }
                    let mut all = deps.subjects[i].clone();
                    all.union(&deps.keys[i], 1);
                    all.union(&body, 1);
                    bodies[i] = body;
                    regions[i] = all;
                }
            }
        }
        deps.nodes = nodes.into_iter().collect();
        deps.bodies = bodies.into_iter().collect();
        deps.regions = regions.into_iter().collect();
        deps
    }

    /// What one code body reads, as the table's rows are scanned.
    pub(crate) fn reads_of(&self, plan: &Plan, code: Code) -> Reads {
        scan(plan, self.layout.expect("built from a plan"), code)
    }

    /// The inputs `reads` names, in bit order.
    pub(crate) fn inputs<'a>(&self, reads: &'a Reads) -> impl Iterator<Item = Input> + 'a {
        let l = self.layout.expect("built from a plan");
        let width = l.width();
        reads
            .bits
            .ones()
            .take_while(move |i| *i < width)
            .map(move |i| {
                let (d, r, m) = (l.derive(0), l.resource(0), l.pending_resource(0));
                let (pm, clock) = (l.pending_mutation(0), l.clock());
                let failed = l.failed_resource(0);
                match i {
                    _ if i < d => Input::Slot(i),
                    _ if i < r => Input::Derive(i - d),
                    _ if i < m => Input::Resource(i - r),
                    _ if i < pm => Input::PendingResource(i - m),
                    _ if i < failed => Input::PendingMutation(i - pm),
                    _ if i < clock => Input::FailedResource(i - failed),
                    _ => Input::Clock,
                }
            })
    }

    /// The inputs that differ between what the tree last showed and `env`.
    pub(crate) fn changed(&self, seen: &Seen, env: &Env<'_>) -> Bits {
        let layout = self.layout.expect("built from a plan");
        let mut bits = Bits::default();
        for (i, (old, new)) in seen.slots.iter().zip(env.slots).enumerate() {
            if !crate::compare::equivalent(old, new) {
                bits.set(i);
            }
        }
        let same = |a: &Option<Value>, b: &Option<Value>| match (a, b) {
            (Some(a), Some(b)) => crate::compare::same(a, b),
            (None, None) => true,
            _ => false,
        };
        for (i, (old, new)) in seen.derives.iter().zip(env.derives).enumerate() {
            if !same(old, new) {
                bits.set(layout.derive(i));
            }
        }
        for (i, (old, new)) in seen.resources.iter().zip(env.resources).enumerate() {
            let same = match (old, new) {
                (Some(a), Some(b)) => Held::same(a, b),
                (None, None) => true,
                _ => false,
            };
            if !same {
                bits.set(layout.resource(i));
            }
        }
        for (i, (old, new)) in seen
            .pending_resources
            .iter()
            .zip(env.pending_resources)
            .enumerate()
        {
            if old != new {
                bits.set(layout.pending_resource(i));
            }
        }
        for (i, (old, new)) in seen
            .pending_mutations
            .iter()
            .zip(env.pending_mutations)
            .enumerate()
        {
            if old != new || seen.failed_mutations.get(i) != env.failed_mutations.get(i) {
                bits.set(layout.pending_mutation(i));
            }
        }
        for (i, old) in seen.failed_resources.iter().enumerate() {
            // Its message too: `failure(x)` reads it (LLP 1109 D3).
            if *old != crate::failure::state(env.failed_resources, env.failed_why, i) {
                bits.set(layout.failed_resource(i));
            }
        }
        if seen.now_ms.to_bits() != env.now_ms.to_bits() {
            bits.set(layout.clock());
        }
        bits
    }
}

/// The table of `codes`' reads, as `collect` makes it, with each distinct
/// code range scanned and placed once: an expanded plan's rows share their
/// component's code, so most rows repeat one.
fn by_code(codes: impl Iterator<Item = Code>, scan: &dyn Fn(Code) -> Reads) -> ReadsTable {
    let mut at = Vec::with_capacity(codes.size_hint().0);
    let mut placed: std::collections::HashMap<(u32, u32), u32> = Default::default();
    let mut places: std::collections::HashMap<Reads, u32> = Default::default();
    for code in codes {
        let place = *placed.entry((code.offset, code.len)).or_insert_with(|| {
            let reads = scan(code);
            match reads == Reads::default() {
                true => 0,
                false => {
                    let next = places.len() as u32 + 1;
                    *places.entry(reads).or_insert(next)
                }
            }
        });
        at.push(place);
    }
    let mut reads = vec![Reads::default(); places.len() + 1];
    for (set, place) in places {
        reads[place as usize] = set;
    }
    ReadsTable { at, reads }
}

fn is_index_key(code: &[u8]) -> bool {
    let mut code = vm::instructions(code);
    matches!(code.next(), Some(Ok(i)) if i.op == Opcode::LoadIndex && i.args[0] == 0)
        && matches!(code.next(), Some(Ok(i)) if i.op == Opcode::Return)
        && code.next().is_none()
}

fn scan(plan: &Plan, layout: Layout, code: Code) -> Reads {
    let mut reads = Reads::default();
    let mut code = vm::instructions(plan.code(code)).peekable();
    while let Some(instruction) = code.next() {
        let Ok(i) = instruction else {
            reads.opaque = true;
            return reads;
        };
        let index = i.args[0] as usize;
        match i.op {
            Opcode::LoadSlot => {
                reads.bits.set(index);
                reads.row_slots |= plan.slots.get(index).is_none_or(|s| s.owner.is_some());
            }
            Opcode::LoadDerive => reads.bits.set(layout.derive(index)),
            Opcode::LoadResource => reads.bits.set(layout.resource(index)),
            Opcode::PendingResource => {
                // Known only once the resource settled, like its value.
                reads.bits.set(layout.resource(index));
                reads.bits.set(layout.pending_resource(index));
            }
            Opcode::PendingMutation | Opcode::FailedMutation | Opcode::FailureMutation => {
                reads.bits.set(layout.pending_mutation(index))
            }
            Opcode::FailedResource | Opcode::FailureResource => {
                reads.bits.set(layout.resource(index));
                reads.bits.set(layout.failed_resource(index));
            }
            Opcode::Call if Stdlib::from_wire(index as u8) == Some(Stdlib::PerformanceNow) => {
                reads.bits.set(layout.clock())
            }
            // A share's files arrive by `set_shared`, after the location (LLP
            // 1133.024 D3): no input the scan sees changes with them.
            Opcode::Call if Stdlib::from_wire(index as u8) == Some(Stdlib::SharedFiles) => {
                reads.opaque = true
            }
            Opcode::LoadItem | Opcode::LoadBound | Opcode::LoadIndex => {
                reads.frames |= 1 << index.min(63);
                if i.op == Opcode::LoadIndex {
                    reads.positions |= 1 << index.min(63);
                }
                if index == 0 {
                    // One field of the innermost frame's value, or all of it
                    // (its position, LLP 1062 D8, counts as all of it).
                    reads.fields |= match code.peek() {
                        Some(Ok(next)) if i.op != Opcode::LoadIndex && next.op == Opcode::Field => {
                            1 << next.args[0].min(63)
                        }
                        _ => !0,
                    };
                }
            }
            Opcode::LoadParam
            | Opcode::StoreSlot
            | Opcode::Command
            | Opcode::Send
            | Opcode::Refresh => reads.opaque = true,
            _ => {}
        }
    }
    reads
}

/// What the tree last showed: the inputs an update compares against.
#[derive(Debug, Default)]
pub struct Seen {
    slots: Vec<Value>,
    derives: Vec<Option<Value>>,
    resources: Vec<Option<Held>>,
    pending_resources: Vec<bool>,
    pending_mutations: Vec<bool>,
    failed_mutations: Vec<Option<crate::failure::Failure>>,
    failed_resources: Vec<Option<crate::failure::Failure>>,
    now_ms: f64,
}

impl Seen {
    /// Resource `i` is now `held`, the same value for every reader
    /// ([`Held::released`]): what the tree showed holds it no more.
    pub(crate) fn release(&mut self, i: usize, held: &Held) {
        if let Some(Some(old)) = self.resources.get_mut(i) {
            if Held::same(old, held) {
                *old = held.clone();
            }
        }
    }

    pub(crate) fn of(env: &Env<'_>) -> Self {
        Seen {
            slots: env.slots.to_vec(),
            derives: env.derives.to_vec(),
            resources: env.resources.to_vec(),
            pending_resources: env.pending_resources.to_vec(),
            pending_mutations: env.pending_mutations.to_vec(),
            failed_mutations: env.failed_mutations.to_vec(),
            failed_resources: crate::failure::states(env.failed_resources, env.failed_why),
            now_ms: env.now_ms,
        }
    }
}

/// Rows an action wrote since the last update: each row's slots and the
/// rows enclosing it, by identity. A row-slot write dirties only its row.
#[derive(Debug, Default, Clone)]
pub struct RowWrites {
    /// Every row on the written rows' scope chains.
    pub(crate) path: Vec<usize>,
    /// `(row, slot)` for each write.
    pub(crate) writes: Vec<(usize, u32)>,
}

impl RowWrites {
    /// The identity of a row's slot storage.
    pub(crate) fn id(row: &RowSlots) -> usize {
        std::rc::Rc::as_ptr(row) as *const () as usize
    }
    /// Record that `slot` of the innermost row in `frames` owning it was written.
    pub(crate) fn record(&mut self, frames: &[vm::Frame], row: &RowSlots, slot: u32) {
        for frame in frames {
            if let Some(r) = &frame.row {
                let id = Self::id(r);
                if !self.path.contains(&id) {
                    self.path.push(id);
                }
            }
        }
        let id = Self::id(row);
        if !self.path.contains(&id) {
            self.path.push(id);
        }
        self.writes.push((id, slot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_code_ranges_keep_full_range_and_plan_identity() {
        use exact_plan::asm::Asm;
        use std::cell::Cell;

        let layout = Layout {
            slots: 0,
            derives: 0,
            resources: 0,
            mutations: 0,
        };
        for depth in [0, 1, 63] {
            let mut code = Asm::new();
            code.load_item(depth).load_index(depth);
            let bytes = code.finish();
            let whole = Code {
                offset: 0,
                len: bytes.len() as u32,
            };
            let truncated = Code { len: 1, ..whole };
            let empty = Code { len: 0, ..whole };
            let plan = Plan {
                code: bytes.into(),
                ..Plan::default()
            };
            let calls = Cell::new(0);
            let codes = [whole, truncated, whole, empty, truncated, whole, empty];
            let table = by_code(codes.into_iter(), &|code| {
                calls.set(calls.get() + 1);
                scan(&plan, layout, code)
            });
            assert_eq!(calls.get(), 3, "each full range is scanned once");
            for i in [0, 2, 5] {
                assert_eq!(table[i].frames, 1 << depth);
                assert_eq!(table[i].positions, 1 << depth);
                assert_eq!(table[i].fields, if depth == 0 { !0 } else { 0 });
                assert!(!table[i].opaque);
            }
            for i in [1, 4] {
                assert!(table[i].opaque, "a shorter range still refuses decoding");
            }
            for i in [3, 6] {
                assert_eq!(table[i], Reads::default());
            }
        }
    }

    #[test]
    fn input_sets_match_a_set_reference_for_sparse_and_dense_dependencies() {
        use std::collections::BTreeSet;
        use std::hash::{Hash, Hasher};
        let hash = |bits: &Bits| {
            let mut state = std::collections::hash_map::DefaultHasher::new();
            bits.hash(&mut state);
            state.finish()
        };
        let from = |set: &BTreeSet<usize>| {
            let mut bits = Bits::default();
            for &i in set {
                bits.set(i);
            }
            bits
        };
        for seed in 0..128_u64 {
            let mut random = seed + 1;
            let mut next = || {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                (random >> 32) as usize
            };
            let (mut a, mut b) = (BTreeSet::new(), BTreeSet::new());
            for _ in 0..256 {
                let i = next() % if seed % 2 == 0 { 256 } else { 65_536 };
                if next() % 2 == 0 {
                    a.insert(i);
                } else {
                    b.insert(i);
                }
            }
            // Word boundaries, isolated high slots, and the empty set.
            if seed == 0 {
                a.clear();
                b.clear();
            }
            if seed == 1 {
                a.extend([0, 63, 64, 127, 128, 65_535]);
                b.clear();
            }
            if seed == 2 {
                a.extend(0..4096);
                b.extend(2048..8192);
            }
            let (mut left, right) = (from(&a), from(&b));
            assert_eq!(left.is_empty(), a.is_empty());
            assert_eq!(left.intersects(&right), !a.is_disjoint(&b));
            assert_eq!(right.intersects(&left), !a.is_disjoint(&b));
            let mut reversed = Bits::default();
            for &i in a.iter().rev() {
                reversed.set(i);
                reversed.set(i);
            }
            assert_eq!(left, reversed);
            assert_eq!(hash(&left), hash(&reversed));
            for i in (0..512).chain([1023, 1024, 32_767, 65_535, 65_536]) {
                assert_eq!(left.get(i), a.contains(&i));
            }
            let expected: BTreeSet<_> = a.union(&b).copied().collect();
            left.union(&right);
            assert_eq!(
                left.ones().collect::<Vec<_>>(),
                expected.iter().copied().collect::<Vec<_>>()
            );
            let mut reversed_union = right.clone();
            reversed_union.union(&reversed);
            assert_eq!(left, reversed_union);
            assert_eq!(hash(&left), hash(&reversed_union));
            left.union(&right);
            assert_eq!(left, from(&expected));
        }
    }

    #[test]
    fn sparse_bits_preserve_ascending_word_boundaries() {
        let mut bits = Bits::default();
        for bit in [0, 63, 64, 127, 128, 4095] {
            bits.set(bit);
        }
        assert_eq!(
            bits.ones().collect::<Vec<_>>(),
            vec![0, 63, 64, 127, 128, 4095]
        );
        assert_eq!(Bits::default().ones().next(), None);
    }

    #[test]
    fn frame_masks_shift_with_a_sticky_far_bit() {
        assert_eq!(out_of(0b110, 1), 0b11);
        assert_eq!(out_of(0b1, 1), 0);
        assert_eq!(out_of(1 << 63, 1), 0b11 << 62);
        assert_eq!(into_scope(0b1, true), 0b11);
        assert_eq!(into_scope(0b1, false), 0b10);
        assert_eq!(into_scope(1 << 62, false), 1 << 63);
        assert_eq!(into_scope(1 << 63, false), 1 << 63);
    }

    #[test]
    fn position_reads_shift_as_frames_do_with_the_same_sticky_far_bit() {
        let at = |positions: u64| Reads {
            frames: positions,
            positions,
            ..Reads::default()
        };
        // An inner body reading the position one scope out: the row's own, seen from the row.
        let mut row = Reads::default();
        row.union(&at(0b10), 1);
        assert!(row.reads_position());
        assert_eq!(row.positions, row.frames);
        // Its own position read from one scope further in is not this row's.
        let mut outer = Reads::default();
        outer.union(&at(0b1), 1);
        assert!(!outer.reads_position());
        // Bit 63 is 63 scopes out or farther, as for frames: seen `shift` scopes out it spreads from 63 - shift up,
        // so it reaches this row's bit 0 once 63 shifts have passed, and no shift drops it.
        for shift in [1, 30, 62, 63, 64] {
            let mut far = Reads::default();
            far.union(&at(1 << 63), shift);
            assert_eq!(far.positions, !0 << (63 - shift.min(63)), "shift {shift}");
            assert_eq!(far.reads_position(), shift >= 63, "shift {shift}");
            assert_eq!(far.positions, far.frames);
        }
        // An opaque body counts as reading it.
        assert!(Reads {
            opaque: true,
            ..Reads::default()
        }
        .reads_position());
    }
}
