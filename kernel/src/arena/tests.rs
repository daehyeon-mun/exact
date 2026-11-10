use super::*;

#[test]
fn keys_fail_closed_after_reuse() {
    let mut arena = NodeArena::new();
    let a = arena.alloc(1, NodeType::View).unwrap();
    let key_a = arena.key(a);
    assert_eq!(arena.resolve(key_a), Some(a));
    arena.free_slot(a);
    assert_eq!(arena.resolve(key_a), None);
    let b = arena.alloc(1, NodeType::Text).unwrap();
    assert_eq!(b, a, "the freed slot is reused");
    assert_ne!(arena.key(b), key_a, "with a new generation");
    assert_eq!(
        arena.resolve(key_a),
        None,
        "the stale key never aliases the new node"
    );
    assert_eq!(arena.slot_of(1), Some(b));
    assert_eq!(arena.node_type(b), NodeType::Text);
    assert_eq!(arena.live_count(), 1);
}

#[test]
fn generation_zero_is_never_minted() {
    let mut arena = NodeArena::new();
    let a = arena.alloc(1, NodeType::View).unwrap();
    assert_eq!(arena.key(a).generation, 1);
}

#[test]
fn subtree_and_ancestry() {
    let mut arena = NodeArena::new();
    let root = arena.alloc(1, NodeType::View).unwrap();
    let a = arena.alloc(2, NodeType::View).unwrap();
    let b = arena.alloc(3, NodeType::View).unwrap();
    let c = arena.alloc(4, NodeType::Text).unwrap();
    arena.set_children(root, vec![a, b]);
    arena.set_parent(a, Some(root));
    arena.set_parent(b, Some(root));
    arena.set_children(a, vec![c]);
    arena.set_parent(c, Some(a));
    assert_eq!(arena.subtree(root), vec![root, a, c, b]);
    assert!(arena.is_ancestor(root, c));
    assert!(!arena.is_ancestor(b, c));
    assert_eq!(arena.depth(c), 2);
    assert_eq!(arena.measure_owner(c), c);
}

#[test]
fn compact_inheritance_matches_full_styles_through_overrides_and_detach() {
    use crate::style::StyleValue;
    let number = StyleValue::Number;
    let text = |s: &'static str| StyleValue::Text(s.into());
    let samples = [
        (StyleId::Direction, text("rtl")),
        (StyleId::CaretColor, text("light-dark(#ffffff, #112233)")),
        (StyleId::AccentColor, text("light-dark(#ff9500, #0a84ff)")),
        (StyleId::FontSize, number(24.0)),
        (StyleId::FontWeight, number(700.0)),
        (StyleId::FontStyle, text("italic")),
        (StyleId::FontStretch, text("75%")),
        (StyleId::FontVariationSettings, text(r#""GRAD" 150"#)),
        (StyleId::FontOpticalSizing, text("none")),
        (StyleId::FontFamily, number(3.0)),
        (StyleId::Cursor, text("grab")),
        (StyleId::TextAlign, text("right")),
        (StyleId::LineHeight, number(1.5)),
        (StyleId::LetterSpacing, number(2.0)),
        (StyleId::FontVariantNumeric, text("tabular-nums")),
        (StyleId::FontVariantLigatures, text("none")),
        (StyleId::TextColor, text("light-dark(#112233, #ffffff)")),
        (StyleId::WhiteSpace, text("pre-wrap")),
        (StyleId::DynamicRangeLimit, text("constrained")),
        (StyleId::ColorScheme, text("dark")),
        // LLP 1121 D5: the app tuple, plus one.
        (StyleId::Mode, number(2.0)),
        (StyleId::OverflowWrap, text("anywhere")),
        (StyleId::InterpolateSize, text("allow-keywords")),
        (StyleId::TextTransform, text("uppercase")),
        (StyleId::TextIndent, number(24.0)),
        (StyleId::Hyphens, text("auto")),
        // LLP 1093 D4.
        (StyleId::Widows, number(4.0)),
        (StyleId::Orphans, number(3.0)),
        (StyleId::TextShadow, text("1px 2px 3px #000")),
        (StyleId::TextStrokeWidth, number(2.0)),
        (StyleId::TextStrokeColor, text("#ff0000")),
        (StyleId::SymbolRendering, text("palette")),
        (StyleId::SymbolPalette, text("#ff0000 #0000ff")),
        // SVG 2 presentation properties (LLP 1055 D2).
        (StyleId::Fill, text("#16a34a")),
        (StyleId::Stroke, text("currentcolor")),
        (StyleId::StrokeWidth, number(1.5)),
        (StyleId::StrokeLinecap, text("round")),
        (StyleId::StrokeLinejoin, text("round")),
        (StyleId::StrokeMiterlimit, number(2.0)),
        (StyleId::StrokeDasharray, text("1 2")),
        (StyleId::StrokeDashoffset, number(1.0)),
        (StyleId::FillOpacity, number(0.5)),
        (StyleId::StrokeOpacity, number(0.5)),
        (StyleId::FillRule, text("evenodd")),
        (StyleId::Visibility, text("hidden")),
        (StyleId::PaintOrder, text("stroke")),
        (StyleId::ClipRule, text("evenodd")),
        (StyleId::TextAnchor, text("middle")),
        (StyleId::DominantBaseline, text("central")),
        (StyleId::PointerEvents, text("stroke")),
        (StyleId::Cursor, text("crosshair")),
        (StyleId::MarkerStart, text("url(#a)")),
        (StyleId::MarkerMid, text("url(#a)")),
        (StyleId::MarkerEnd, text("url(#a)")),
        (StyleId::ColorInterpolationFilters, text("sRGB")),
        (StyleId::TextBoxEdge, text("cap-alphabetic")),
        (StyleId::TextWrapStyle, text("balance")),
        (StyleId::FontSmoothing, text("antialiased")),
    ];
    let mut covered = StyleMask::EMPTY;
    for (id, value) in samples {
        covered.set(id);
        let mut arena = NodeArena::new();
        let root = arena.alloc(1, NodeType::View).unwrap();
        let parent = arena.alloc(2, NodeType::View).unwrap();
        let leaf = arena.alloc(3, NodeType::Text).unwrap();
        arena.set_parent(parent, Some(root));
        arena.set_parent(leaf, Some(parent));
        let mut full = arena.computed_style(leaf, StyleMask::INHERITED);
        let mut compact = arena.computed_inherited(leaf);
        for step in 0..5 {
            match step {
                0 => arena.style_mut(root).set_dynamic(id, &value).unwrap(),
                // Explicit initial values must override the ancestor.
                1 => arena.style_mut(parent).mask.set(id),
                2 => arena.style_mut(parent).clear(StyleMask::of(id)),
                3 => arena.set_parent(leaf, None),
                _ => arena
                    .style_mut(leaf)
                    .set_dynamic(StyleId::Width, &number(75.0))
                    .unwrap(),
            }
            let next_full = arena.computed_style(leaf, StyleMask::INHERITED);
            let next_compact = arena.computed_inherited(leaf);
            let mut full_changed = StyleMask::EMPTY;
            for row in StyleMask::INHERITED.iter() {
                if full.get(row) != next_full.get(row) {
                    full_changed.set(row);
                }
            }
            let expected = if step == 4 {
                StyleMask::EMPTY
            } else {
                StyleMask::of(id)
            };
            assert_eq!(full_changed, expected, "{id:?}, step {step}");
            assert_eq!(compact.changed_mask(&next_compact), full_changed);
            full = next_full;
            compact = next_compact;
        }
        assert_eq!(full.width, crate::style::Dimension::Points(75.0));
    }
    assert_eq!(covered, StyleMask::INHERITED);
    println!(
        "inheritance snapshot: {} bytes; full style: {} bytes",
        std::mem::size_of::<InheritedStyle>(),
        std::mem::size_of::<StyleProps>()
    );
}

#[test]
fn text_runs_flatten_inline_children() {
    let mut arena = NodeArena::new();
    let p = arena.alloc(1, NodeType::Text).unwrap();
    let r1 = arena.alloc(2, NodeType::Text).unwrap();
    let r2 = arena.alloc(3, NodeType::Text).unwrap();
    arena.set_children(p, vec![r1, r2]);
    arena.set_parent(r1, Some(p));
    arena.set_parent(r2, Some(p));
    arena.set_prop(r1, PropId::Text, "Hello ".into());
    arena.set_prop(r2, PropId::Text, "world".into());
    arena.style_mut(r2).font_size = 20.0;
    let mut runs = Vec::new();
    arena.text_runs(p, &mut runs);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text, "Hello ");
    assert_eq!(runs[1].text, "world");
    assert_eq!(runs[1].style.font_size, 20.0);
    assert!(arena.is_inline_run(r1));
    assert_eq!(arena.measure_owner(r2), p);
    // An own `text` prop suppresses the children.
    arena.set_prop(p, PropId::Text, "override".into());
    let mut runs = Vec::new();
    arena.text_runs(p, &mut runs);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "override");
}

#[test]
fn relative_environment_is_owned_by_the_arena_and_cleared_on_reset() {
    let mut arena = NodeArena::new();
    arena.relative_environment = Some((arena.env().clone(), None, 16.0, false));
    let copy = arena.clone();
    arena.reset();
    assert!(arena.relative_environment.is_none());
    assert!(copy.relative_environment.is_some());
}
