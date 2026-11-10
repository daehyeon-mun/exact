//! Plain keyed rows can keep their detached views until the key returns.
//! Slots restart from their initializers and bindings receive a full update.
//! Named, interactive and lifecycle-bearing subtrees must be destroyed:
//! detached kernel nodes remain addressable until their allocation ends.

use super::region::init_owned;
use super::*;
use exact_kernel::{PropId, StyleId};
use std::cell::Cell;

/// Rows kept per region; the oldest is destroyed past this.
const PER_REGION: usize = 3;

/// Allocated only when a region has a detached row, outside the hot `Child`.
#[derive(Debug, Default)]
pub(super) struct Kept {
    rows: Vec<Row>,
}

thread_local! {
    static REINIT: Cell<bool> = const { Cell::new(false) };
}

/// Whether slots start again from their initializers (a kept row's reuse).
pub(super) fn reinit() -> bool {
    REINIT.with(Cell::get)
}

/// During a reuse: a row's own slots from their initializers, in its frames.
pub(super) fn reinit_row(
    u: &mut Update<'_>,
    arm: Option<ArmsId>,
    r: &Row,
    frames: &[Frame],
) -> Result<(), InstanceError> {
    if !reinit() {
        return Ok(());
    }
    if let (Some(arm), Some(slots)) = (arm, r.slots.as_ref()) {
        init_owned(u, arm, slots, &with_frame(frames, r.frame.clone()))?;
    }
    Ok(())
}

fn refuses_retention(plan: &Plan, children: &[Child]) -> bool {
    children.iter().any(|c| match c {
        Child::Node(n) => {
            n.collection().is_some()
                || site_refused(plan, n.node)
                || refuses_retention(plan, &n.children)
        }
        Child::Region(r) => match &r.active {
            Active::Arm { roots, .. } => refuses_retention(plan, roots),
            Active::Rows { rows } => rows.iter().any(|row| refuses_retention(plan, &row.roots)),
        },
    })
}

fn site_refused(plan: &Plan, node: NodesId) -> bool {
    let site = plan.node(node);
    // A rebind resets host state; detached retention does not. Keep only
    // presentation here, including no selector, command target or handler.
    !matches!(
        NodeType::from_wire(site.node_type),
        Some(NodeType::View | NodeType::Text)
    ) || site.handlers.iter().next().is_some()
        || collection::reuse::site_refused(plan, node)
        || site
            .bindings
            .iter()
            .map(|b| plan.binding(b))
            .any(|b| match b.kind {
                BindingKind::Prop => b.id != PropId::Text as u16,
                BindingKind::Style => [
                    StyleId::Animation,
                    StyleId::Transition,
                    StyleId::ContentTransition,
                    StyleId::OverflowX,
                    StyleId::OverflowY,
                ]
                .iter()
                .any(|id| *id as u16 == b.id),
            })
}

/// A row that left: retain only subtrees whose host state is safe to keep.
pub(super) fn leave(u: &mut Update<'_>, kept: &mut Option<Box<Kept>>, gone: Row, frames: &[Frame]) {
    // A collection rebind must not leave children of the previous item alive.
    if collection::shown::in_collection_row(u.env.plan, frames)
        || refuses_retention(u.env.plan, &gone.roots)
    {
        destroy_all(u, gone.roots);
        return;
    }

    let rows = &mut kept.get_or_insert_with(Default::default).rows;
    rows.push(gone);
    if rows.len() > PER_REGION {
        let oldest = rows.remove(0);
        destroy_all(u, oldest.roots);
    }
}

pub(super) fn destroy(u: &mut Update<'_>, kept: Option<Box<Kept>>) {
    if let Some(kept) = kept {
        for row in kept.rows {
            destroy_all(u, row.roots);
        }
    }
}

/// The kept row for `key_text`, brought up to date for `frame`.
#[allow(clippy::too_many_arguments)]
pub(super) fn take(
    u: &mut Update<'_>,
    arm: Option<ArmsId>,
    kept: &mut Option<Box<Kept>>,
    key_text: &str,
    dup: u32,
    frames: &[Frame],
    mut frame: Frame,
    body: &Reads,
) -> Result<Option<Row>, InstanceError> {
    let Some(rows) = kept.as_mut().map(|k| &mut k.rows) else {
        return Ok(None);
    };
    let Some(at) = rows
        .iter()
        .position(|r| r.dup == dup && ident(&r.key, r.dup).as_deref() == Some(key_text))
    else {
        return Ok(None);
    };
    let mut r = rows.remove(at);
    if rows.is_empty() {
        *kept = None;
    }
    frame.row = r.slots.clone();
    r.frame = frame;
    let full = std::mem::replace(&mut u.full, true);
    let was = REINIT.with(|c| c.replace(true));
    let result =
        reinit_row(u, arm, &r, frames).and_then(|()| update_row(u, &mut r, frames, !0, body));
    REINIT.with(|c| c.set(was));
    u.full = full;

    result?;
    Ok(Some(r))
}

#[cfg(test)]
mod tests {
    use crate::{CollectionFeedback, DataError, DataSource, Runner};
    use exact_kernel::{Kernel, Offer, ViewId};
    use exact_plan::Value;

    struct NoData;
    impl DataSource for NoData {
        fn query(&mut self, name: &str, _: &[Value]) -> Result<Value, DataError> {
            Err(DataError::UnknownSource(name.into()))
        }
    }

    fn boot(source: &str) -> Runner<NoData> {
        Runner::boot(
            contract::compile(source).unwrap(),
            NoData,
            Kernel::with_monospace(),
            Default::default(),
            "/",
        )
        .unwrap()
    }

    #[test]
    fn retained_rows_reuse_plain_views_but_destroy_editor_state() {
        for (tag, retained) in [("text \"row\"", true), ("input", false)] {
            let source = format!("component App\n  state count = 1\n  action hide\n    count = 0\n  action show\n    count = 1\n  view\n    column\n      for n in range(count) key=n\n        {tag}\n");
            let mut runner = Runner::boot(
                contract::compile(&source).unwrap(),
                NoData,
                Kernel::with_monospace(),
                Default::default(),
                "/",
            )
            .unwrap();
            let root = runner.kernel().roots()[0];
            let first = runner.kernel().node(root).unwrap().children()[0];
            runner.act("hide", vec![]).unwrap();
            assert_eq!(runner.kernel().node(first).is_some(), retained, "{tag}");
            runner.act("show", vec![]).unwrap();
            let next = runner.kernel().node(root).unwrap().children()[0];
            assert_eq!(first == next, retained, "{tag}");
        }
    }

    #[test]
    fn named_interactive_and_animated_descendants_end_their_lifetime_on_removal() {
        for child in [
            "text \"row\" id=\"named\"",
            "text \"row\" testId=\"named\"",
            "text \"row\" press=poke",
            "text \"row\" tabindex=0",
            "box animation=\"fade 100ms\"",
            "box overflow=\"auto\"",
        ] {
            let source = format!(
                r#"keyframes fade
  from opacity=0
  to opacity=1
component App
  state count = 1
  action hide
    count = 0
  action show
    count = 1
  action poke
    count = count
  view
    column
      for n in range(count) key=n
        column
          {child}
"#
            );
            let mut r = boot(&source);
            let root = r.kernel().roots()[0];
            for _ in 0..3 {
                let row = r.kernel().node(root).unwrap().children()[0];
                let leaf = r.kernel().node(row).unwrap().children()[0];
                let old = r.kernel().node(leaf).unwrap().key;
                r.act("hide", vec![]).unwrap();
                assert!(r.kernel().node(row).is_none(), "{child}");
                assert!(r.kernel().node_by_key(old).is_none(), "{child}");
                assert!(r.kernel().find_by_id("named").is_empty(), "{child}");
                assert!(r.kernel().find_by_test_id("named").is_empty(), "{child}");
                assert_eq!(r.kernel().live_count(), 1, "{child}");
                r.act("show", vec![]).unwrap();
                assert!(r.kernel().node_by_key(old).is_none(), "{child}");
            }
        }
    }

    #[test]
    fn removed_observer_rows_drop_pending_entries_and_observe_again_on_mount() {
        let mut r = boot(
            r#"component App
  state count = 1
  action hide
    count = 0
  action show
    count = 1
  action seen(e: IntersectionObserverEntry)
    count = count
  view
    column
      for n in range(count) key=n
        box width=20 height=20 intersect=seen
"#,
        );
        let root = r.kernel().roots()[0];
        for _ in 0..3 {
            r.kernel_mut()
                .compute_layout(root, Offer::definite(100., 100.))
                .unwrap();
            let due = r.intersect_due();
            assert_eq!(due.len(), 1);
            let old = due[0].clone();
            assert!(r.intersect_delivered(&old));
            r.act("hide", vec![]).unwrap();
            assert!(r.intersect_due().is_empty());
            assert!(!r.intersect_delivered(&old));
            r.act("show", vec![]).unwrap();
            assert!(!r.intersect_delivered(&old));
        }
    }

    #[test]
    fn collection_rebinds_do_not_retain_nested_rows_of_departed_items() {
        let mut r = boot(
            r#"component App
  view
    list virtualized=true height=320 estimated-item-height=64
      for item in range(100) key=item
        column height=64
          for n in range(item % 3) key=n
            text `${n}`
"#,
        );
        r.set_row_reuse(true);
        let mut rebound = 0;
        for offset in [0., 256., 1024., 512., 4096., 0.] {
            let c = &r.collections()[0];
            let feedback = CollectionFeedback {
                view: c.view,
                revision: c.revision,
                scroll_sequence: c.scroll_sequence + 1,
                offset,
                port_cross: 640.,
                port_main: 320.,
                cross: 640.,
                padding: [0.; 2],
                measurements: vec![],
                focus_view: None,
                interaction_view: None,
            };
            let result = r.collection_feedback(feedback).unwrap();
            assert!(result.error.is_none(), "{:?}", result.error);
            rebound += r.last_instance_work().rows_rebound;
            fn attached(k: &Kernel, id: ViewId) -> usize {
                1 + k
                    .node(id)
                    .unwrap()
                    .children()
                    .iter()
                    .map(|id| attached(k, *id))
                    .sum::<usize>()
            }
            let mounted: usize = r
                .kernel()
                .roots()
                .iter()
                .map(|id| attached(r.kernel(), *id))
                .sum();
            assert_eq!(r.kernel().live_count(), mounted, "offset {offset}");
        }
        assert!(rebound > 0);
    }
}
