// Agent and canvas pictures follow the same sibling order as input.
// Copy the layer tree, never reorder the live views (Tab and AX tree order).
#if os(macOS)
import AppKit

extension Capture {
    /// AppKit attaches new backing layers at the window display boundary.
    /// Do this once per upload, before Capture.bitmap changes visibility.
    package static func prepareBackingLayers(of view: NSView) {
        view.layoutSubtreeIfNeeded()
        if view.layer == nil, (view as? NSTextView)?.isFieldEditor != true { view.wantsLayer = true }
        func unattached(_ v: NSView) -> Bool {
            // The installed editor inherits backing from its field. Opting it
            // into backing explicitly can interrupt AppKit's editing session.
            if (v as? NSTextView)?.isFieldEditor == true { return false }
            guard let layer = v.layer else { return true }
            if v.superview != nil, layer.superlayer == nil { return true }
            return v.subviews.contains(where: unattached)
        }
        if unattached(view) { view.window?.displayIfNeeded() }
    }

    static func paintOrderBitmap(of view: NSView, scale: CGFloat? = nil, prepared: Bool = false) -> NSBitmapImageRep? {
        if !prepared { prepareBackingLayers(of: view) }
        view.displayIfNeeded()
        let scale = scale ?? view.pixelScale ?? 2
        guard let root = view.layer else { return nil }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let tree = paintOrderLayer(of: view, root: root)
        defer { releasePaintCopy(tree) }
        guard let image = picture(of: tree, size: view.bounds.size, scale: scale) else { return nil }
        let rep = NSBitmapImageRep(cgImage: image)
        rep.size = view.bounds.size
        return rep
    }

    /// Detached copies still participate in the current transaction. Drop
    /// their borrowed backing stores before commit so AppKit retains its ink.
    static func releasePaintCopy(_ tree: CALayer) {
        tree.contents = nil
        if let mask = tree.mask { releasePaintCopy(mask) }
        for child in tree.sublayers ?? [] { releasePaintCopy(child) }
    }

    /// A layer tree drawn into a bitmap of `size` points, its first row at
    /// the top, as AppKit's `cacheDisplay` has it.
    static func picture(of tree: CALayer, size: CGSize, scale: CGFloat) -> CGImage? {
        let width = Int((size.width * scale).rounded()), height = Int((size.height * scale).rounded())
        guard width > 0, height > 0,
              let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                                  bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        // CA renders the root in bottom-up Quartz space.
        ctx.translateBy(x: 0, y: CGFloat(height))
        ctx.scaleBy(x: scale, y: -scale)
        tree.render(in: ctx)
        return ctx.makeImage()
    }
}

extension Capture {
    /// `body` with each session's guest pictures (its iframes' and native views'
    /// snapshots) in `web` for their nodes to draw, and those live views hidden,
    /// which a capture cannot draw; put back after.
    static func withGuests<T>(of sessions: [ExactSession], _ body: () -> T) -> T {
        var hidden: [(NSView, Bool)] = []
        for session in sessions {
            let views = session.presenter.views
            for (id, image) in session.webviews.snapshots().merging(session.natives.snapshots(), uniquingKeysWith: { web, _ in web }) {
                if let node = views[id] { web[ObjectIdentifier(node)] = image }
            }
            hidden += (views.values.compactMap(\.web) + session.natives.snapshotViews).map { ($0, $0.isHidden) }
        }
        hidden.forEach { $0.0.isHidden = true }
        defer { hidden.forEach { $0.0.isHidden = $0.1 }; web = [:] }
        return body()
    }
}

extension Agent {
    /// Whether `window` is covered and holds a canvas a GPU module draws: a covered
    /// window's Metal layers present no frames (`GpuMac.visible`), so the window
    /// server's picture of it holds none of what they draw.
    /// The sessions whose nodes are in `view`: a host's window holds several.
    static func sessions(in view: NSView) -> [ExactSession] {
        var found: [ObjectIdentifier: ExactSession] = [:], order: [ExactSession] = []
        func walk(_ v: NSView) {
            if let session = (v as? NodeView)?.presenter?.session, found.updateValue(session, forKey: ObjectIdentifier(session)) == nil { order.append(session) }
            v.subviews.forEach(walk)
        }
        walk(view)
        return order
    }

    static func coveredCanvases(_ window: NSWindow) -> Bool {
        guard !window.occlusionState.contains(.visible), let content = window.contentView else { return false }
        func canvas(_ view: NSView) -> Bool {
            if let node = view as? NodeView, node.kind == "canvas", !node.isHiddenOrHasHiddenAncestor, node.canvases?.readback(view: node) != nil { return true }
            return view.subviews.contains(where: canvas)
        }
        return canvas(content)
    }

    /// `image`, the window server's picture of `window`, with its content as the
    /// capture paints it (`Capture.picture`: views in their z order, each canvas
    /// from its readback, every session's) in place of what a covered window's
    /// Metal layers left. The frame stays the window server's, its shape kept
    /// (source-atop), and so is its title bar.
    static func paintedContent(_ image: CGImage, of window: NSWindow) -> CGImage {
        guard let content = window.contentView, window.frame.width > 0,
              let picture = Capture.withGuests(of: sessions(in: content), { Capture.picture(of: content) })?.cgImage,
              let ctx = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return image }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        // Window coordinates run up from the frame's bottom left, as the context's do.
        let scale = CGFloat(image.width) / window.frame.width
        ctx.scaleBy(x: scale, y: scale)
        ctx.setBlendMode(.sourceAtop)
        ctx.draw(picture, in: content.convert(content.bounds, to: nil))
        ctx.setBlendMode(.normal)
        // The title bar is the window server's: a capture draws it as an active
        // window's, without its hairline. Through a transparent one the content
        // shows, so there only its buttons are drawn over the content.
        if let bar = window.standardWindowButton(.closeButton)?.superview, !bar.isHiddenOrHasHiddenAncestor {
            if window.titlebarAppearsTransparent {
                for kind in [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton] {
                    guard let button = window.standardWindowButton(kind), !button.isHiddenOrHasHiddenAncestor,
                          let drawn = Capture.picture(of: button)?.cgImage else { continue }
                    ctx.draw(drawn, in: button.convert(button.bounds, to: nil))
                }
            } else {
                ctx.saveGState()
                ctx.clip(to: bar.convert(bar.bounds, to: nil))
                ctx.setBlendMode(.copy)
                ctx.draw(image, in: CGRect(origin: .zero, size: window.frame.size))
                ctx.restoreGState()
            }
        }
        return ctx.makeImage() ?? image
    }
}
#endif

#if os(macOS)
import AppKit

/// A view's subtree as pixels (LLP 1014 D3).
package enum Capture {
    /// A capture is drawing: its draws are not repaints (D4 b).
    nonisolated(unsafe) static var capturing = false
    /// Guest pictures for this turn, by the node that draws each (two
    /// sessions' ids can be the same); the remote platform views are hidden
    /// while their owning nodes draw these (@ref LLP 1020 D4/D6, `withGuests`).
    nonisolated(unsafe) static var web: [ObjectIdentifier: ExactWebImage] = [:]

    /// The subtree painted at `scale`: premultiplied RGBA, rows top-down,
    /// `pixelsWide * 4` bytes per row, transparent where nothing painted.
    package static func bitmap(of view: NSView, scale: CGFloat, prepared: Bool = false) -> NSBitmapImageRep? {
        if !prepared { prepareBackingLayers(of: view) }
        // A subtree painted through its canvas composites at alpha 0; paint
        // it opaque into the bitmap regardless.
        let alpha = view.alphaValue
        view.alphaValue = 1
        // A canvas nested under this one that is painted through its own
        // surface: its picture comes by readback (its draw), not from its
        // overlay's views, so those are hidden for the duration.
        var hidden: [NSView] = []
        func hide(_ v: NSView) {
            for s in v.subviews {
                if let n = s as? NodeView, n.placement != nil, !n.isHidden { n.isHidden = true; hidden.append(n); continue }
                if let n = s as? NodeView, let o = n.overlay, o.alphaValue == 0, !o.isHidden { o.isHidden = true; hidden.append(o); continue }
                hide(s)
            }
        }
        hide(view)
        let rep = paintOrderBitmap(of: view, scale: scale, prepared: true)
        for o in hidden { o.isHidden = false }
        view.alphaValue = alpha
        return rep
    }
}

extension NSRect {
    /// The rect inside the given edges (never negative in size).
    func insetBy(left: CGFloat, top: CGFloat, right: CGFloat, bottom: CGFloat) -> NSRect {
        NSRect(x: minX + left, y: minY + top, width: max(0, width - left - right), height: max(0, height - top - bottom))
    }
}
#endif
