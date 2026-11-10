#if os(macOS)
import AppKit

/// An epoch bumped by every frame or subview change of a node, and each node's extent at an epoch.
enum HitExtent {
    nonisolated(unsafe) static var epoch: UInt64 = 0
    static func changed() { epoch &+= 1 }

    /// The union of `view`'s bounds and every descendant's frame, in `view`'s own coordinates; nil when some part
    /// of it is hit where its frame is not (a 3D plane, a canvas's placed children).
    static func of(_ view: NSView) -> NSRect? {
        if let node = view as? NodeView {
            if node.hitExtentEpoch == epoch { return node.hitExtent }
            let e = compute(view)
            node.hitExtent = e; node.hitExtentEpoch = epoch
            return e
        }
        return compute(view)
    }
    private static func compute(_ view: NSView) -> NSRect? {
        if let node = view as? NodeView, node.plane != nil || node.overlay != nil || node.placement != nil || node.placementHidden { return nil }
        var r = view.bounds
        // A scroll view and its clip view take no hit outside their frames.
        if view is NSScrollView || view is NSClipView { return r }
        for sub in view.subviews {
            guard let e = of(sub) else { return nil }
            r = r.union(view.convert(e, from: sub))
        }
        return r
    }
}
#endif
