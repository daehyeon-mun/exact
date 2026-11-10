//! A layout prepares the native controls a change reached, and no other
//! (`controls.rs`): counted, and held to a kernel rebuilt from the columns,
//! which prepares every control, frame for frame.
use crate::{
    ButtonMeasure, ButtonMeasureRequest, FieldChrome, FieldChromeRequest, Kernel,
    MonospaceMeasurer, NodeType, Offer, Op, PropId, PropValue, StyleId, StyleProps, StyleValue,
    TextMeasureRequest, TextMeasurer, TextMetrics, ViewId,
};
use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

const OFFER: Offer = Offer {
    width: crate::AxisOffer::Definite(800.0),
    height: crate::AxisOffer::Definite(600.0),
};

/// What the host was asked, and what it says of itself.
#[derive(Default)]
struct Asked {
    buttons: Cell<usize>,
    fields: Cell<usize>,
    revision: Cell<u64>,
    provisional: Cell<bool>,
    /// A button's answer at a definite width is a stand-in.
    stand_in: Cell<bool>,
}

/// A host whose answer is a function of the whole request, so a control
/// prepared from stale inputs is laid out at another size.
struct Host(Rc<Asked>);

fn hashed(request: &impl std::fmt::Debug, revision: u64) -> f32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{request:?}").hash(&mut hasher);
    revision.hash(&mut hasher);
    (hasher.finish() % 64) as f32
}

impl TextMeasurer for Host {
    fn measure(&mut self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        MonospaceMeasurer::default().measure(request)
    }
    fn measure_revision(&self) -> u64 {
        self.0.revision.get()
    }
    fn button_measure(&mut self, request: &ButtonMeasureRequest) -> Option<ButtonMeasure> {
        self.0.buttons.set(self.0.buttons.get() + 1);
        // The offer is the engine's question, not an input of the face.
        let face = (&request.face, &request.style, &request.button_style);
        Some(ButtonMeasure {
            width: 40.0 + hashed(&face, self.0.revision.get()),
            height: 20.0 + hashed(&face, self.0.revision.get()) / 4.0,
            provisional: self.0.stand_in.get()
                && matches!(request.width, crate::AxisOffer::Definite(_)),
        })
    }
    fn field_chrome(&mut self, request: &FieldChromeRequest) -> FieldChrome {
        self.0.fields.set(self.0.fields.get() + 1);
        let inset = 1.0 + hashed(request, self.0.revision.get()) / 8.0;
        FieldChrome {
            top: inset,
            right: 2.0,
            bottom: inset,
            left: 2.0,
            minimum_height: 0.0,
            provisional: self.0.provisional.get(),
        }
    }
}

fn view(id: ViewId, node_type: NodeType) -> Op {
    Op::CreateView { id, node_type }
}
fn prop(id: ViewId, prop: PropId, value: &str) -> Op {
    Op::SetProp {
        id,
        prop,
        value: PropValue::Str(value.into()),
    }
}
fn style(id: ViewId, row: StyleId, value: &str) -> Op {
    let mut patch = StyleProps::default();
    let value = value
        .parse()
        .map_or_else(|_| StyleValue::Text(value.into()), StyleValue::Number);
    patch.set_dynamic(row, &value).unwrap();
    Op::SetStyle {
        id,
        patch: Box::new(patch),
    }
}
fn children(id: ViewId, children: &[ViewId]) -> Op {
    Op::SetChildren {
        id,
        children: children.to_vec(),
    }
}

/// 1 root > [2 text, 3 group > [per control: a button with its title, or a
/// field], 4 group > [5 button > 6 title]]. Buttons are 100 + 2i with titles
/// 101 + 2i; fields 100 + 2i. Laid out once.
fn tree(controls: u32) -> (Kernel, Rc<Asked>, Vec<ViewId>) {
    let asked = Rc::new(Asked::default());
    let mut k = Kernel::new(Box::new(Host(asked.clone())));
    let mut ops = vec![
        view(1, NodeType::View),
        view(2, NodeType::Text),
        view(3, NodeType::View),
        view(4, NodeType::View),
        view(5, NodeType::Control),
        view(6, NodeType::Text),
        prop(2, PropId::Text, "beside"),
        prop(5, PropId::Type, "button"),
        prop(6, PropId::Text, "Apart"),
    ];
    let mut ids = Vec::new();
    for i in 0..controls {
        let id = 100 + 2 * i;
        ids.push(id);
        if i.is_multiple_of(2) {
            ops.extend([
                view(id, NodeType::Control),
                view(id + 1, NodeType::Text),
                prop(id, PropId::Type, "button"),
                prop(id + 1, PropId::Text, &format!("Button {i}")),
                children(id, &[id + 1]),
            ]);
        } else {
            ops.push(view(id, NodeType::TextInput));
        }
    }
    ops.extend([
        children(5, &[6]),
        children(4, &[5]),
        children(3, &ids),
        children(1, &[2, 3, 4]),
        Op::AttachRoot { id: 1 },
    ]);
    k.apply(0, 1, &ops).unwrap();
    k.compute_layout(1, OFFER).unwrap();
    (k, asked, ids)
}

/// Lay out, and say how many controls that prepared.
fn prepared(k: &mut Kernel) -> usize {
    let before = k.tree().controls_prepared();
    k.compute_layout(1, OFFER).unwrap();
    k.tree().controls_prepared() - before
}

/// Every frame as a kernel rebuilt from the columns lays it out.
fn equal_fresh(k: &Kernel, asked: &Rc<Asked>) {
    let host = Rc::new(Asked::default());
    host.revision.set(asked.revision.get());
    let mut fresh = k.rehydrate(Box::new(Host(host)));
    fresh.compute_layout(1, OFFER).unwrap();
    for slot in k.arena().iter_live() {
        let node = k.node(k.arena().local_id(slot)).unwrap();
        let other = fresh.node(node.id).unwrap();
        assert!(
            node.frame.bits_eq(other.frame),
            "view {}: {:?}, rebuilt {:?}",
            node.id,
            node.frame,
            other.frame
        );
    }
}

#[test]
fn a_change_elsewhere_prepares_no_control_whatever_their_number() {
    for controls in [4, 400] {
        let (mut k, asked, _) = tree(controls);
        assert_eq!(k.tree().controls_prepared(), controls as usize + 1);
        let (buttons, fields) = (asked.buttons.get(), asked.fields.get());
        k.apply(0, 2, &[prop(2, PropId::Text, "another text")])
            .unwrap();
        assert_eq!(prepared(&mut k), 0, "{controls} controls");
        k.apply(0, 3, &[style(4, StyleId::BackgroundColor, "#ff0000")])
            .unwrap();
        assert_eq!(prepared(&mut k), 0, "a paint row above a button");
        assert_eq!(prepared(&mut k), 0, "a layout nothing changed before");
        assert_eq!((asked.buttons.get(), asked.fields.get()), (buttons, fields));
        equal_fresh(&k, &asked);
    }
}

#[test]
fn a_change_in_a_control_s_subtree_or_of_what_it_inherits_prepares_it() {
    let (mut k, asked, ids) = tree(6);
    let buttons = asked.buttons.get();
    // A button's title: that button.
    k.apply(0, 2, &[prop(101, PropId::Text, "Renamed")])
        .unwrap();
    assert_eq!(prepared(&mut k), 1);
    assert!(asked.buttons.get() > buttons, "its face is measured again");
    equal_fresh(&k, &asked);
    // A field's own row: that field.
    let fields = asked.fields.get();
    k.apply(0, 3, &[style(102, StyleId::FontSize, "20")])
        .unwrap();
    assert_eq!(prepared(&mut k), 1);
    assert_eq!(asked.fields.get(), fields + 1);
    equal_fresh(&k, &asked);
    // An inherited row on the group: every control under it, and not the
    // button in the other group.
    k.apply(0, 4, &[style(3, StyleId::FontWeight, "700")])
        .unwrap();
    assert_eq!(prepared(&mut k), ids.len());
    equal_fresh(&k, &asked);
}

#[test]
fn a_hidden_control_waits_for_its_box_and_a_shown_one_is_prepared() {
    let (mut k, asked, _) = tree(4);
    k.apply(0, 2, &[style(4, StyleId::Display, "none")])
        .unwrap();
    assert_eq!(prepared(&mut k), 0);
    let buttons = asked.buttons.get();
    k.apply(0, 3, &[prop(6, PropId::Text, "Changed while hidden")])
        .unwrap();
    assert_eq!(prepared(&mut k), 0, "nothing under a hidden box is asked");
    assert_eq!(prepared(&mut k), 0, "nor walked to again");
    assert_eq!(asked.buttons.get(), buttons);
    k.apply(0, 4, &[style(4, StyleId::Display, "flex")])
        .unwrap();
    assert_eq!(prepared(&mut k), 1);
    assert!(asked.buttons.get() > buttons);
    equal_fresh(&k, &asked);
    // Hidden and shown with nothing changed under it: nothing to ask.
    k.apply(0, 5, &[style(4, StyleId::Display, "none")])
        .unwrap();
    prepared(&mut k);
    k.apply(0, 6, &[style(4, StyleId::Display, "flex")])
        .unwrap();
    assert_eq!(prepared(&mut k), 0);
    equal_fresh(&k, &asked);
}

#[test]
fn a_role_above_a_button_and_a_move_prepare_it() {
    let (mut k, asked, ids) = tree(4);
    // A `menu` above a button is a floor under its height.
    k.apply(0, 2, &[prop(4, PropId::AccessibilityRole, "menu")])
        .unwrap();
    assert_eq!(prepared(&mut k), 1, "the button under the menu");
    equal_fresh(&k, &asked);
    k.apply(
        0,
        3,
        &[Op::ClearProp {
            id: 4,
            prop: PropId::AccessibilityRole,
        }],
    )
    .unwrap();
    assert_eq!(prepared(&mut k), 1);
    equal_fresh(&k, &asked);
    // A button moved under another box has other boxes above it.
    let mut rest = ids.clone();
    let moved = rest.remove(0);
    k.apply(0, 4, &[children(3, &rest), children(4, &[5, moved])])
        .unwrap();
    assert_eq!(prepared(&mut k), 1, "the button that moved");
    equal_fresh(&k, &asked);
}

#[test]
fn the_host_s_revision_and_the_environment_prepare_every_control() {
    let (mut k, asked, ids) = tree(6);
    asked.revision.set(1);
    assert_eq!(prepared(&mut k), ids.len() + 1);
    equal_fresh(&k, &asked);
    let mut env = k.env();
    env.top = 12.0;
    assert!(!k.set_env(env).unwrap(), "no style reads an inset");
    assert_eq!(prepared(&mut k), ids.len() + 1);
    equal_fresh(&k, &asked);
}

#[test]
fn a_provisional_answer_is_asked_again_until_it_is_final() {
    let asked = Rc::new(Asked::default());
    asked.provisional.set(true);
    let mut k = Kernel::new(Box::new(Host(asked.clone())));
    k.apply(
        0,
        1,
        &[
            view(1, NodeType::View),
            view(2, NodeType::TextInput),
            children(1, &[2]),
            Op::AttachRoot { id: 1 },
        ],
    )
    .unwrap();
    k.compute_layout(1, OFFER).unwrap();
    assert_eq!(prepared(&mut k), 1, "a stand-in is asked for again");
    assert_eq!(k.provisional_layouts(), 2);
    asked.provisional.set(false);
    assert_eq!(prepared(&mut k), 1);
    assert_eq!(prepared(&mut k), 0, "a final answer stands");
    assert_eq!(k.provisional_layouts(), 2);
}

/// A box that took a new parent, or a role, can be gone before the next
/// layout looks under it.
#[test]
fn a_moved_box_removed_before_the_next_layout_is_passed_over() {
    let (mut k, asked, ids) = tree(4);
    // Group 4 moves under group 3 and takes a role, then goes with its button.
    let mut under: Vec<ViewId> = ids.clone();
    under.push(4);
    k.apply(
        0,
        2,
        &[
            children(1, &[2, 3]),
            children(3, &under),
            prop(4, PropId::AccessibilityRole, "menu"),
        ],
    )
    .unwrap();
    k.apply(0, 3, &[children(3, &ids), Op::DestroyView { id: 4 }])
        .unwrap();
    assert_eq!(prepared(&mut k), 0, "nothing left under it to ask");
    equal_fresh(&k, &asked);
}

/// A field in a grouped list's row is its cell's (`is_cell_field`): the
/// list's style and the ground and tag of the rows' group decide it, with no
/// change at the field.
#[test]
fn a_grouped_list_s_field_follows_its_list_and_its_group() {
    let asked = Rc::new(Asked::default());
    let mut k = Kernel::new(Box::new(Host(asked.clone())));
    // 1 root > 2 list > 3 section > 4 group > 5 row > 6 box > 7 field; the
    // group on its card, as the compiler fills a section's rows.
    let mut ops: Vec<Op> = (1..=6).map(|id| view(id, NodeType::View)).collect();
    ops.push(view(7, NodeType::TextInput));
    ops.push(style(4, StyleId::BackgroundColor, "#ffffff"));
    ops.extend((1..=6).map(|id| children(id, &[id + 1])));
    ops.push(Op::AttachRoot { id: 1 });
    k.apply(0, 1, &ops).unwrap();
    k.compute_layout(1, OFFER).unwrap();
    assert!(!k.node(7).unwrap().is_cell_field());
    assert_eq!(prepared(&mut k), 0, "no list: asked once");

    k.apply(0, 2, &[prop(2, PropId::ListStyle, "insetGrouped")])
        .unwrap();
    assert_eq!(prepared(&mut k), 1, "the list became a grouped one");
    assert!(k.node(7).unwrap().is_cell_field());
    equal_fresh(&k, &asked);
    assert_eq!(
        prepared(&mut k),
        1,
        "a cell's field is asked at every layout"
    );

    k.apply(0, 3, &[style(4, StyleId::BackgroundColor, "#00000000")])
        .unwrap();
    k.compute_layout(1, OFFER).unwrap();
    assert!(!k.node(7).unwrap().is_cell_field(), "no card, no cell");
    equal_fresh(&k, &asked);
    assert_eq!(prepared(&mut k), 0);

    k.apply(0, 4, &[style(4, StyleId::BackgroundColor, "#ffffff")])
        .unwrap();
    assert_eq!(prepared(&mut k), 1, "the group's ground");
    equal_fresh(&k, &asked);
    k.apply(0, 5, &[prop(4, PropId::SemanticTag, "footer")])
        .unwrap();
    k.compute_layout(1, OFFER).unwrap();
    assert!(
        !k.node(7).unwrap().is_cell_field(),
        "a footer holds no rows"
    );
    equal_fresh(&k, &asked);
    k.apply(
        0,
        6,
        &[
            Op::ClearProp {
                id: 4,
                prop: PropId::SemanticTag,
            },
            Op::ClearProp {
                id: 2,
                prop: PropId::ListStyle,
            },
        ],
    )
    .unwrap();
    k.compute_layout(1, OFFER).unwrap();
    equal_fresh(&k, &asked);
    assert_eq!(prepared(&mut k), 0, "no list again");
}

/// A host answers a width it has not measured with a stand-in while the
/// engine runs, after the controls were prepared.
#[test]
fn a_stand_in_answered_during_the_engine_s_measure_is_asked_for_again() {
    let (mut k, asked, _) = tree(2);
    let before = k.provisional_layouts();
    asked.stand_in.set(true);
    asked.revision.set(1);
    assert_eq!(prepared(&mut k), 3, "a new revision: every control");
    assert_eq!(k.provisional_layouts(), before + 1);
    asked.stand_in.set(false);
    assert_eq!(prepared(&mut k), 2, "the two buttons a stand-in answered");
    assert_eq!(k.provisional_layouts(), before + 1);
    assert_eq!(prepared(&mut k), 0);
    equal_fresh(&k, &asked);
}

/// Seeded changes of every kind a control's answer reads, each layout held
/// to a rebuilt kernel's.
#[test]
fn random_changes_lay_out_as_a_rebuilt_kernel_does() {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut random = |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    let (mut k, asked, ids) = tree(10);
    // Which group each control is in, and whether each group shows.
    let mut home: Vec<ViewId> = vec![3; ids.len()];
    let mut shown = [true, true];
    for step in 0..300u64 {
        let at = random(ids.len() as u64) as usize;
        let (id, button) = (ids[at], at.is_multiple_of(2));
        let group = [3, 4][random(2) as usize];
        let ops = match random(9) {
            0 if button => vec![prop(id + 1, PropId::Text, &format!("Title {step}"))],
            1 => vec![style(id, StyleId::FontSize, &(12 + random(8)).to_string())],
            2 => vec![style(
                group,
                StyleId::FontWeight,
                ["400", "700"][random(2) as usize],
            )],
            3 => {
                let now = &mut shown[(group - 3) as usize];
                *now = !*now;
                vec![style(
                    group,
                    StyleId::Display,
                    if *now { "flex" } else { "none" },
                )]
            }
            4 => match random(2) {
                0 => vec![prop(group, PropId::AccessibilityRole, "menu")],
                _ => vec![Op::ClearProp {
                    id: group,
                    prop: PropId::AccessibilityRole,
                }],
            },
            5 => {
                home[at] = group;
                let of = |g: ViewId| -> Vec<ViewId> {
                    let fixed = (g == 4).then_some(5);
                    let moved = ids.iter().zip(&home).filter(|(_, h)| **h == g);
                    fixed.into_iter().chain(moved.map(|(id, _)| *id)).collect()
                };
                vec![children(3, &of(3)), children(4, &of(4))]
            }
            6 if button => vec![prop(
                id,
                PropId::ButtonStyle,
                ["plain", "bordered"][random(2) as usize],
            )],
            7 => vec![style(id, StyleId::Width, &(60 + random(60)).to_string())],
            _ => vec![prop(2, PropId::Text, &format!("beside {step}"))],
        };
        k.apply(0, step + 2, &ops).unwrap();
        if random(11) == 0 {
            asked.revision.set(asked.revision.get() + 1);
        }
        k.compute_layout(1, OFFER).unwrap();
        equal_fresh(&k, &asked);
    }
}
