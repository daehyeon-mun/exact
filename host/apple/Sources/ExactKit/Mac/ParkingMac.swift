// What stays hidden leaves the window, so that an input's cost does not grow
// with what is mounted but not shown. AppKit's passes follow the number of
// views in a window, hidden or not: `-[NSWindow layoutIfNeeded]` scans every
// view when any view needs layout, the layer-tree build visits every view when
// a view is added or removed anywhere, and a hide invalidates cursor rects over
// all of them. Measured on macOS 27 with 20,000 hidden nodes: a turn that
// changes the window's views costs 1 to 2 µs for each hidden node in it, and
// nothing for a view out of the hierarchy.
//
// So a hidden node keeps its own view where it is, and its children wait in a
// view of their own that is in no window (`ParkedContent`): a kept navigation
// route that is not shown, a closed popover, a `display: none` box. The node
// still answers `container` with where its children are, so ops reach them,
// frames are set and `children` ops order them as before. They go back among
// the node's views when it is shown.
//
// Taking a subtree out and putting it back is AppKit's work too: about 40 µs
// a node with the layout and the commit that follow (measured on a page with
// a native control in every eleventh node; a control leaving a window is the
// most of it), where hiding it in place and showing it again is under 3. So a
// node hidden in the window keeps its children there until it has stayed
// hidden through `Parking.patience` turns that changed the window's views,
// which is about when keeping them has cost what parking them will; a page a
// person goes back to at once is still there. A node hidden before it is in a
// window (a popover as it is created, a route its root does not name, a
// `display: none` box) holds its children out from the start, and one hidden
// in the batch that mounted it parks as that batch ends.
#if os(macOS)
import AppKit

/// A hidden node's children while it is hidden. It is hidden itself, so
/// AppKit answers `isHiddenOrHasHiddenAncestor` for what it holds as it did
/// under the hidden node, and it carries the appearance the children had
/// there, so a colour resolved while they wait is the one they return to.
final class ParkedContent: NSView {
    weak var owner: NodeView?
    init(owner: NodeView, like live: NSView) {
        self.owner = owner
        super.init(frame: live.bounds)
        isHidden = true
        appearance = live.effectiveAppearance
        setAccessibilityElement(false)
    }
    required init?(coder: NSCoder) { nil }
    override var isFlipped: Bool { true }
}

extension NSView {
    /// The view above this one in the page as it is authored: its superview,
    /// and for a hidden node's parked children the node (their holder is no
    /// part of the page). An ancestor walk that reads what a node inherits
    /// (inertness, an accent, a grid origin) goes up this, so it reads the
    /// same whether or not the node is shown.
    var structuralSuperview: NSView? {
        guard let above = superview else { return (self as? ParkedContent)?.owner }
        return (above as? ParkedContent)?.owner ?? above
    }
    /// The window the page it belongs to is in: its own, or for a hidden node's parked children the node's.
    var structuralWindow: NSWindow? {
        var at: NSView? = self
        while let view = at {
            if let window = view.window { return window }
            at = view.structuralSuperview
        }
        return nil
    }
    /// `isDescendant(of:)` up `structuralSuperview`: whether it is `ancestor` or under it in the page as authored,
    /// shown or not. `isDescendant(of:)` itself asks whether it is under it in the window's views.
    func isWithin(_ ancestor: NSView) -> Bool {
        var at: NSView? = self
        while let view = at {
            if view === ancestor { return true }
            at = view.structuralSuperview
        }
        return false
    }
    /// The views under this one in the page as authored: its subviews, and for a hidden node the children it holds
    /// out of the window. A walk that writes what a subtree inherits (a tint, a theme's roles, a row's emphasis)
    /// goes down this, so what waits out of the window returns as the rest of the page is.
    var structuralSubviews: [NSView] {
        guard let held = (self as? NodeView)?.parked?.subviews, !held.isEmpty else { return subviews }
        return subviews + held
    }
}

extension NodeView {
    /// Where its children are while it is shown (`container` while hidden is `parked`).
    var liveContainer: NSView { glassGroupContent ?? baseContainer }

    /// `isHidden` changed (the one writer, `NodeView.isHidden`). Shown, the
    /// children return at once: layout, the focus and a hit test that follow
    /// need them. Hidden outside every window, they are held out from now on;
    /// hidden in a window, they stay until the node has been hidden a while
    /// (`Parking.settle`).
    func hiddenChanged(_ hidden: Bool) {
        guard hidden else { unpark(); presenter?.navigation.shownAgain(self); presenter?.parking.shown(self); return }
        presenter?.navigation.hiddenNow(self)
        // The kinds that hold nodes: a box, a scroller, a button the app paints.
        if window == nil, parked == nil, ["view", "scroll", "button"].contains(kind) {
            parked = ParkedContent(owner: self, like: liveContainer)
            // Refused (a held press is among its children): they stay where they are, all of them, until a settle takes them.
            if !park() { parked = nil; presenter?.parking.owe(self, now: true) }
        }
        // Hidden in the batch that put it in the window, it has not been on screen: nothing returns to it sooner than to any other.
        else if let presenter { presenter.parking.owe(self, now: presenter.isApplying && joinedBatch == presenter.session?.appliedBatches ?? 0) }
    }

    /// Takes the children out of the window, unless a held press or a field
    /// being edited is among them: those wait for the next settle.
    @discardableResult func park() -> Bool {
        // A list's rows are its collection's or its table's, which read them where they are.
        guard isHidden, kind != "list", let presenter else { return true }
        let live = liveContainer
        let children = live.subviews.filter { $0 is NodeView || $0 is FlightSlot }
        guard !children.isEmpty else { return true }
        // AppKit sends a held button's drags and up only to a view in its window (`MouseChainMac`).
        if presenter.pointerHolds, let source = presenter.pointerSource, children.contains(where: { source.isDescendant(of: $0) }) { return false }
        // A hidden node holds no focus (`refreshInputGates` took it). One that still does is not ready to go.
        if let window, let responder = window.firstResponder as? NSView, children.contains(where: { responder.isDescendant(of: $0) }) { return false }
        let holder = parked ?? ParkedContent(owner: self, like: live)
        parked = holder
        for child in children { holder.addSubview(child) }
        presenter.rehome(from: live, to: holder)
        presenter.parkingChanged()
        return true
    }

    /// Puts the children back among the node's views, in the order they
    /// were kept in, after whatever else its container holds.
    func unpark() {
        guard let holder = parked else { return }
        parked = nil
        let live = liveContainer, children = holder.subviews
        presenter?.rehome(from: holder, to: live)
        guard !children.isEmpty else { return }
        for child in children { live.addSubview(child) }
        guard let presenter else { return }
        presenter.parkingChanged()
        // A batch configures its controls after its ops (`Presenter.apply`). Shown outside one (a popover opening),
        // the controls passed over while they were out are configured now.
        if !presenter.isApplying, presenter.controls.awaits { presenter.controls.sync() }
    }

    /// Its parked children resolve colours by the appearance they would inherit from it: a holder made before the
    /// node was in a window, or before the appearance changed, takes the node's.
    func followParkedAppearance() {
        guard let parked, window != nil, parked.appearance?.name != effectiveAppearance.name else { return }
        parked.appearance = effectiveAppearance
    }

    /// A view mounted in the window under a hidden node whose children are
    /// held out (a closed popover a host puts back under its authored parent,
    /// which is hidden): that node takes it out with the rest.
    func parkUnderHiddenAncestor() {
        guard let above = superview, above.isHiddenOrHasHiddenAncestor else { return }
        var at: NSView? = above
        while let view = at {
            if let node = view as? NodeView, node.isHidden {
                if node.parked != nil { node.presenter?.parking.owe(node, now: true) }
                return
            }
            at = view.superview
        }
    }
}

extension Presenter {
    /// A subtree a host shows elsewhere (an open popover in the top layer, a dialog in its sheet, a block in its
    /// pane) goes back among its authored parent's children when it closes, and a `children` op knows it by them.
    /// Those children changed views: its home is where they are now.
    func rehome(from old: NSView, to new: NSView) {
        menus.rehome(from: old, to: new)
        dialogs.rehome(from: old, to: new)
        blocks.rehome(from: old, to: new)
    }
}

/// The hidden nodes whose children are still in the window, and how long each has been.
final class Parking {
    /// The turns that changed the window's views a hidden node's children stay in it for. Each costs
    /// 1 to 2 µs a node it holds; taking them out and back, with the layout and the commit that
    /// follow, about 40: between 17 and 39 turns to break even.
    nonisolated(unsafe) static var patience = 16
    private unowned let presenter: Presenter
    private var waiting: [ObjectIdentifier: (node: NodeView, turns: Int)] = [:]
    /// Those that park at the next settle: never on screen, or their other children out already.
    private let unseen = NSHashTable<NodeView>.weakObjects()
    /// Views entered or left the window, or a node was hidden or shown, since the last settle.
    var changed = false
    private var observing = false, parkingSoon = false
    init(_ presenter: Presenter) { self.presenter = presenter }

    /// `now`: it has not been on screen, or its other children are out already, so it parks at the next settle.
    func owe(_ node: NodeView, now: Bool = false) {
        if now { unseen.add(node) } else { waiting[ObjectIdentifier(node)] = (node, waiting[ObjectIdentifier(node)]?.turns ?? 0) }
        changed = true
        observe()
    }
    func shown(_ node: NodeView) {
        waiting.removeValue(forKey: ObjectIdentifier(node))
        changed = true
        observe()
    }
    /// Outside a batch (a popover closed by Escape) the turn's end settles.
    private func observe() {
        guard !presenter.isApplying, !observing else { return }
        observing = true
        let observer = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.beforeWaiting.rawValue, false, -1_000_002) { [weak self] _, _ in
            self?.observing = false
            self?.settle()
        }
        CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
    }

    /// The end of a batch or of a turn. A turn that changed the window's
    /// views ages every node waiting; one hidden long enough parks once this
    /// turn's frame has gone, so no input waits for it.
    func settle() {
        presenter.navigation.settle()
        if unseen.count > 0 {
            let nodes = unseen.allObjects
            unseen.removeAllObjects()
            for node in nodes where node.isHidden && node.presenter === presenter && !node.park() { waiting[ObjectIdentifier(node)] = (node, Self.patience) }
        }
        guard changed, !waiting.isEmpty else { changed = false; return }
        changed = false
        var due = false
        for (key, entry) in waiting {
            guard entry.node.isHidden, entry.node.presenter === presenter else { waiting.removeValue(forKey: key); continue }
            waiting[key]?.turns = entry.turns + 1
            if entry.turns + 1 > Self.patience { due = true }
        }
        guard due, !parkingSoon else { return }
        parkingSoon = true
        DispatchQueue.main.async { [weak self] in self?.parkDue() }
    }

    private func parkDue() {
        parkingSoon = false
        guard !presenter.isApplying else { changed = true; return } // the batch's own settle asks again
        for (key, entry) in waiting where entry.turns > Self.patience {
            if !entry.node.isHidden || entry.node.presenter !== presenter || entry.node.park() { waiting.removeValue(forKey: key) }
        }
    }

    func reset() { waiting.removeAll(); unseen.removeAllObjects(); changed = false }
}
#endif
