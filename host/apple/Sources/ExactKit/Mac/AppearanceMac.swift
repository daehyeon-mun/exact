#if os(macOS)
import AppKit

extension NodeView {
    /// Whether this view draws in the dark appearance. The **owning view's**
    /// appearance, not `NSAppearance.currentDrawing()`: `currentDrawing()`
    /// names whatever is drawing at that instant, and colours are not all
    /// applied inside a draw — a field's `textColor` is assigned in
    /// `applyStyle`. @ref LLP 1034 D2
    var drawsDark: Bool {
        effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
    }

    /// The four channels a colour row carries, resolved. A fixed colour is
    /// the four; a `light-dark()` pair is two fours and this picks one
    /// (LLP 1034 D1). Anything else is not a colour.
    func channels(_ key: String, dark: Bool? = nil) -> [Double]? {
        style[key].flatMap { $0.channels(dark: dark ?? drawsDark, contrast: drawsHighContrast, elevated: window is SheetWindow, tint: ownTint(for: $0)) }
    }

    /// Whether any colour on this node is a pair — what says an appearance
    /// change is something to this view rather than nothing.
    var hasSchemeColor: Bool {
        style.values.contains { $0.isSchemeColor || $0.isSchemeGradient || $0.containsSystemColor }
    }

    /// The appearance changed under this view. A repaint is not enough: the
    /// text engine caches a paragraph spec and a laid-out paragraph, and a
    /// `Run` carries a concrete colour, so ink from the previous appearance
    /// would survive a redisplay. Inline text nodes are not in the native
    /// hierarchy, so the paragraph that owns them is invalidated too, and
    /// the colours assigned outside a draw are re-applied. @ref LLP 1034 D2
    package override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        presenter?.tables.appearanceChanged(self); refreshGroupedSeparator(); presenter?.navigation.split.recolor(self)
        if let regions = presenter?.session?.regions, regions.owns(self) { regions.geometryChanged() }
        // An `svg`'s paints are resolved again; paint motion hears the appearance (LLP 1062 D4).
        presenter?.svg.reappear(id, dark: drawsDark, clock: presenter?.session?.clock, tint: viewTint); presenter?.session?.noteAppearance(self)
        // A path icon's fill is a resolved colour; an image view resolved its tint as it drew.
        tintIconShape()
        guard hasSchemeColor || fieldGround != nil || inlineText.contains(where: { $0.hasSchemeColor }) else { return }
        recolor()
    }
    /// An appearance change re-resolves only what carries a colour. The box's
    /// background, border and gradient are the layer paint `updateLayer` makes (or `draw`), the shadow is the layer's,
    /// and the text is its paragraph's paint; nothing else in the style depends on the appearance. A view whose
    /// appearance reaches further (a field's AppKit control, a material, a filter, a symbol) takes the whole style.

    func recolor() {
        guard field == nil, textArea == nil, materialView == nil, !hasBoxFilter, !hasSymbolLeaf, scroll == nil || hasBoxPaint else {
            reapplyColors(); return
        }

        paragraphOwner.invalidateText()
        paragraphOwner.needsDisplay = true
        layerPaintCache = nil
        if shadowCaster != nil || !boxShadows.isEmpty { applyShadow() }
        needsDisplay = true
    }
    /// A system colour changed under this view (the accent, LLP 1095 D5):
    /// what it resolved is applied again, as for an appearance change; an
    /// untinted symbol follows the accent, so it counts too.
    func systemColorsChanged() {
        presenter?.navigation.split.recolor(self)
        guard hasSchemeColor || hasSymbolLeaf || inlineText.contains(where: { $0.hasSchemeColor }) else { return }
        reapplyColors()
    }
    func reapplyColors() {
        paragraphOwner.invalidateText()
        paragraphOwner.needsDisplay = true
        applyStyle(style)
        needsDisplay = true
    }
}
#endif
