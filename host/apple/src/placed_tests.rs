//! Ranks and document extents are found again for the nodes a commit changed
//! and the boxes above them (`Host::emit_ranks`, `extents`): held to a pass
//! over the whole tree, answer for answer, and counted.
use super::*;
use exact_kernel::{MonospaceMeasurer, Op, StyleId, StyleProps, StyleValue};
use exact_runner::{DataError, Value};

struct NoData;
impl DataSource for NoData {
    fn query(&mut self, name: &str, _: &[Value]) -> Result<Value, DataError> {
        Err(DataError::UnknownSource(name.into()))
    }
}

/// A root that does not clip over `groups` groups of four boxes, the second
/// of each group clipping what it holds, and a popover.
fn fixture(groups: usize) -> Host<NoData> {
    let mut source = String::from("component App\n  view\n    column testId=\"root\"\n");
    source.push_str("      column id=\"menu\" popover=\"auto\"\n        text \"Menu\"\n");
    for g in 0..groups {
        source.push_str(&format!(
            "      column testId=\"g{g}\"\n        text \"first {g}\" testId=\"a{g}\"\n        box testId=\"b{g}\" width=40 height=20 overflow=\"hidden\"\n          box testId=\"c{g}\" width=400 height=300\n        text \"last {g}\" testId=\"d{g}\"\n"
        ));
    }
    let plan = contract::compile(&source).unwrap().encode();
    let measurer = Box::new(MonospaceMeasurer::default());
    Host::boot(&plan, NoData, measurer, 900., 700.).unwrap().0
}

fn id(host: &Host<NoData>, name: &str) -> ViewId {
    let k = host.runner.kernel();
    k.node_by_key(k.find_by_test_id(name)[0]).unwrap().id
}

fn style(id: ViewId, rows: &[(StyleId, &str)]) -> Op {
    let mut patch = StyleProps::default();
    for (row, value) in rows {
        let value = value
            .parse()
            .map_or_else(|_| StyleValue::Text((*value).into()), StyleValue::Number);
        patch.set_dynamic(*row, &value).unwrap();
    }
    Op::SetStyle {
        id,
        patch: Box::new(patch),
    }
}

fn commit(host: &mut Host<NoData>, batch: u64, ops: &[Op]) -> String {
    let receipt = host.runner.kernel_mut().apply(0, batch, ops).unwrap();
    host.commit(&[Timed { at_ms: 0., receipt }], None)
}

/// Every placed node's rank as a pass over the whole tree finds it, and the
/// root's extent as a walk of every box finds it.
fn equal_whole(host: &mut Host<NoData>) {
    let kernel = host.runner.kernel();
    for (view, placed) in kernel.paint_order() {
        assert_eq!(host.ranks.get(&view), Some(&placed.rank), "view {view}");
    }
    let root = kernel.node(kernel.roots()[0]).unwrap();
    let walked = extents::walked(&root, kernel);
    let kept = host.extents.document(&root, kernel);
    assert_eq!(
        (kept.0.to_bits(), kept.1.to_bits()),
        (walked.0.to_bits(), walked.1.to_bits())
    );
    assert_eq!(
        host.mirror[&root.id].content,
        Some(walked),
        "what the presenter heard"
    );
}

#[test]
fn a_change_reads_its_node_and_the_boxes_above_it_whatever_the_tree_s_size() {
    let mut read = Vec::new();
    for groups in [8, 800] {
        let mut host = fixture(groups);
        equal_whole(&mut host);
        let (placed, found) = (host.rank_placed, host.extents.read);
        assert!(placed > 5 * groups, "the first pass places every node");
        // A paint row on one leaf, then a box that grows in the last group,
        // which moves the one box after it.
        let leaf = id(&host, "d3");
        let batch = commit(&mut host, 2, &[style(leaf, &[(StyleId::Opacity, "0.5")])]);
        assert!(batch.contains(&format!("{{\"op\":\"rank\",\"id\":{leaf},\"rank\":1}}")));
        let grown = id(&host, &format!("b{}", groups - 1));
        commit(&mut host, 3, &[style(grown, &[(StyleId::Height, "90")])]);
        equal_whole(&mut host);
        read.push((host.rank_placed - placed, host.extents.read - found));
        // Nothing changed: nothing is placed or read again.
        let before = (host.rank_placed, host.extents.read);
        let mut none = Batch::new();
        host.layout(&mut none).unwrap();
        let roots = host.runner.kernel().roots().len();
        assert_eq!(
            host.rank_placed - before.0,
            roots + groups + 1,
            "the roots' children"
        );
        assert_eq!(host.extents.read, before.1);
    }
    // Extents: the boxes that moved and the group above them. Ranks: the
    // changed group's boxes and the root's children, which are decided
    // together, at each of the two commits; never the tree.
    let (small, large) = (read[0], read[1]);
    assert_eq!(small.1, large.1, "extents read: {read:?}");
    assert!(small.1 <= 8, "extents read: {read:?}");
    assert_eq!(large.0 - small.0, 2 * (800 - 8), "ranks placed: {read:?}");
}

#[test]
fn seeded_changes_answer_as_a_pass_over_the_whole_tree() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut random = |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    const GROUPS: u64 = 6;
    let mut host = fixture(GROUPS as usize);
    let root = id(&host, "root");
    let menu = host.runner.kernel().node(root).unwrap().children()[0];
    // Which groups are under the root, in order.
    let mut attached: Vec<u64> = (0..GROUPS).collect();
    let mut next = 5_000;
    for step in 0..400u64 {
        let g = random(GROUPS);
        let node = id(
            &host,
            &format!("{}{g}", ["g", "a", "b", "c", "d"][random(5) as usize]),
        );
        let ops = match random(10) {
            0 => vec![style(
                node,
                &[(StyleId::Opacity, ["0.5", "1"][random(2) as usize])],
            )],
            1 => vec![style(
                node,
                &[
                    (StyleId::ZIndex, &random(4).to_string()),
                    (StyleId::PositionType, "relative"),
                ],
            )],
            2 => vec![style(
                node,
                &[(
                    StyleId::PositionType,
                    ["static", "absolute"][random(2) as usize],
                )],
            )],
            3 => vec![style(
                node,
                &[(StyleId::Height, &(10 + random(400)).to_string())],
            )],
            4 => vec![style(
                node,
                &[(StyleId::Width, &(10 + random(1200)).to_string())],
            )],
            5 => vec![style(
                node,
                &[
                    (
                        StyleId::OverflowX,
                        ["visible", "hidden"][random(2) as usize],
                    ),
                    (
                        StyleId::OverflowY,
                        ["visible", "hidden"][random(2) as usize],
                    ),
                ],
            )],
            6 => vec![style(
                node,
                &[(StyleId::Display, ["flex", "none"][random(2) as usize])],
            )],
            7 => {
                // A group leaves the tree, or comes back at the end.
                match attached.iter().position(|a| *a == g) {
                    Some(at) => drop(attached.remove(at)),
                    None => attached.push(g),
                }
                let mut children = vec![menu];
                children.extend(attached.iter().map(|a| id(&host, &format!("g{a}"))));
                vec![Op::SetChildren { id: root, children }]
            }
            8 => {
                // A new box under a group, wide enough to count.
                next += 1;
                let group = id(&host, &format!("g{g}"));
                let mut children = host.runner.kernel().node(group).unwrap().children();
                children.push(next);
                vec![
                    Op::CreateView {
                        id: next,
                        node_type: NodeType::View,
                    },
                    style(
                        next,
                        &[
                            (StyleId::Width, &(random(1500)).to_string()),
                            (StyleId::Height, "30"),
                        ],
                    ),
                    Op::SetChildren {
                        id: group,
                        children,
                    },
                ]
            }
            _ => {
                // The last box added under a group goes.
                let group = id(&host, &format!("g{g}"));
                let children = host.runner.kernel().node(group).unwrap().children();
                match children.last().filter(|c| **c > 5_000) {
                    Some(last) => vec![Op::DestroyView { id: *last }],
                    None => vec![],
                }
            }
        };
        commit(&mut host, step + 2, &ops);
        equal_whole(&mut host);
    }
}

/// A commit that makes a few nodes reads each one's listeners; one that
/// makes much of the tree reads every declaration at once. Either way the
/// batch names the same listeners.
#[test]
fn created_nodes_carry_their_listeners_however_many_a_commit_makes() {
    let mut batches = Vec::new();
    for filler in [0, 400] {
        let mut source = String::from(
            "component App\n  state open = false\n  state hot = false\n  action flip\n    open = not open\n  action over(hovered: bool)\n    hot = hovered\n  view\n    column\n      button press=flip testId=\"flip\"\n        text \"Flip\"\n      if open\n        column\n          row press=flip hover=over\n            text \"pressed\"\n          text \"plain\"\n",
        );
        for i in 0..filler {
            source.push_str(&format!("      text \"filler {i}\"\n"));
        }
        let plan = contract::compile(&source).unwrap().encode();
        let measurer = Box::new(MonospaceMeasurer::default());
        let (mut host, _) = Host::boot(&plan, NoData, measurer, 900., 700.).unwrap();
        let flip = id(&host, "flip");
        let batch = host.dispatch_at(flip, exact_runner::Event::Press, 16.0);
        let creates: Vec<&str> = batch
            .split("{\"op\":\"create\"")
            .skip(1)
            .map(|op| {
                let from = op.find("\"handlers\":").expect("a create's listeners");
                &op[from..from + op[from..].find(']').unwrap() + 1]
            })
            .collect();
        assert_eq!(creates.len(), 4, "the column, the row and two texts");
        batches.push(creates.join(" "));
    }
    assert!(batches[0].contains("\"press\""), "{}", batches[0]);
    assert_eq!(batches[0], batches[1]);
}
