// @ref LLP 1069.001 D5 — `input type="checkbox"` is AppKit's checkbox
// (`NSButton`), and `switch` an `NSSwitch`, projected into the node's box as
// a tablist's segmented control is. Contract owns the value: the control
// flips at once, reports, and shows the committed `checked` after the action
// (D4). `appearance: none` draws nothing native.
#if os(macOS)
import AppKit

extension NSControl {
    var ariaUnavailableFromNode: Bool {
        var view: NSView? = self
        while let current = view {
            if let node = current as? NodeView { return node.ariaUnavailable }
            view = current.superview
        }
        return false
    }
}

class AriaButtonCellMac: NSButtonCell {
    override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        if attribute == .enabled, (controlView as? NSControl)?.ariaUnavailableFromNode == true { return false }
        return super.accessibilityAttributeValue(attribute)
    }
    // An assistive app's press clicks once AppKit's request has returned (`NativeButtonCellMac`).
    override func accessibilityPerformPress() -> Bool {
        guard isEnabled else { return false }
        DispatchQueue.main.async { _ = super.accessibilityPerformPress() }
        return true
    }
}

final class CheckboxMac: NSButton {
    override class var cellClass: AnyClass? { get { AriaButtonCellMac.self } set {} }
    override func mouseDown(with event: NSEvent) { if !contextClicked(event) { super.mouseDown(with: event) } }
    override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        if attribute == .enabled, ariaUnavailableFromNode { return false }
        return super.accessibilityAttributeValue(attribute)
    }
}

final class SwitchMac: NSSwitch {
    override func mouseDown(with event: NSEvent) { if !contextClicked(event) { super.mouseDown(with: event) } }
    // An assistive app's press flips it once AppKit's request has returned (`NativeButtonCellMac`).
    override func accessibilityPerformPress() -> Bool {
        guard isEnabled else { return false }
        DispatchQueue.main.async { _ = super.accessibilityPerformPress() }
        return true
    }
    override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        if attribute == .enabled, ariaUnavailableFromNode { return false }
        return super.accessibilityAttributeValue(attribute)
    }
}

final class PopupCellMac: NSPopUpButtonCell {
    override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        if attribute == .enabled, (controlView as? NSControl)?.ariaUnavailableFromNode == true { return false }
        return super.accessibilityAttributeValue(attribute)
    }
}

final class SliderCellMac: NSSliderCell {
    override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        if attribute == .enabled, (controlView as? NSControl)?.ariaUnavailableFromNode == true { return false }
        return super.accessibilityAttributeValue(attribute)
    }
}

final class ControlHost: NSObject {
    unowned let presenter: Presenter
    var controls: [UInt32: NSControl] = [:]
    /// Which control each node shows (`ControlKinds`), to remake it when that changes.
    var kinds: [UInt32: String] = [:]
    /// The size last reported per control, so each is published once.
    private var reported: [UInt32: CGSize] = [:]
    /// A select's menu as last built, so a batch that leaves it alone does not rebuild it.
    var menus: [UInt32: SelectMenu] = [:]
    /// A range's last reported value while it moves, so each is sent once.
    var lastRange: [UInt32: String] = [:]
    /// Each native button's face as the runner last gave it. A face is the
    /// button's viewless contents, which change only in a batch that says so
    /// (`Batch.controls`), and its own props, which change only in a batch
    /// that touches it: `Presenter.apply` drops it then, and the next read
    /// asks the runner. Every other batch keeps it, so a commit that changes
    /// no button waits on the owner thread for none, as on iOS.
    private var faces: [UInt32: ButtonFace] = [:]
    func face(_ id: UInt32) -> ButtonFace {
        if let face = faces[id] { return face }
        if let many = presenter.buttonFaces {
            var ids = [id]
            // With it, every other face a `sync` will ask for: not those of controls out of the window, which it passes over.
            for (other, kind) in kinds where kind == "button" && other != id && faces[other] == nil && controls[other] != nil && !away.contains(other) { ids.append(other) }
            for (other, face) in zip(ids, many(ids)) { faces[other] = face }
            return faces[id] ?? ButtonFace()
        }
        let face = presenter.buttonFace?(id) ?? ButtonFace()
        faces[id] = face
        return face
    }
    /// Asks for the face of `id` again at its next read.
    func faceChanged(_ id: UInt32) { faces.removeValue(forKey: id) }
    /// Asks for every face again at its next read, and configures every control again at the next `sync`.
    func facesChanged() { faces.removeAll(); everyControlTouched = true }
    /// The nodes the batch being applied has an op for (`Presenter.apply`), and those of them whose op can change
    /// what a node under it inherits: a style, props, a new place in the tree. `sync` configures a control again
    /// when its node is one of the first or a node above it one of the second (what a control shows is its node's
    /// own state and what it inherits), when the batch says a button's contents changed (`facesChanged`), and the
    /// first time. Any other control is as its last `sync` left it, so a batch costs the controls it touches, not
    /// every native control mounted in the window: a dialog mounted beside a page reconfigures none of the page's.
    private var touched = Set<UInt32>(), restyled = Set<UInt32>()
    private var everyControlTouched = true
    private var configured = Set<UInt32>()
    func touch(_ id: UInt32, restyles: Bool) { touched.insert(id); if restyles { restyled.insert(id) } }
    /// The control nodes as `sync` last listed them, while the chrome index and the views stand as they were.
    private var listed: (revision: Int, views: Int, owners: [NodeView], live: Set<UInt32>)?
    /// The controls a `sync` passed over because their node was in no window (its page is hidden and out of the
    /// window, `ParkingMac.swift`): each is configured at the first `sync` that finds it in one. A batch then
    /// costs the controls on screen that it touches, not every native control mounted.
    private var away = Set<UInt32>()
    /// Whether a control is waiting to be configured when its node is back in a window.
    var awaits: Bool { !away.isEmpty }
    /// A natural size per control class and control size: AppKit's `intrinsicContentSize` runs an Auto Layout
    /// pass in each control, and a page's hundred switches all answer the same.
    private var naturalSizes: [NaturalKey: CGSize] = [:]
    private struct NaturalKey: Hashable { let type: ObjectIdentifier, size: UInt }
    private func kept(_ control: NSControl) -> CGSize {
        guard control is NSSwitch || control is CheckboxMac || control is RadioButtonMac else { return naturalSize(control) }
        let key = NaturalKey(type: ObjectIdentifier(Swift.type(of: control)), size: control.controlSize.rawValue)
        if let size = naturalSizes[key] { return size }
        let size = naturalSize(control)
        naturalSizes[key] = size
        return size
    }
    /// A range's bound value last written into it: written again only when
    /// it changes (LLP 1069.001 D4, amended 2026-10-04).
    var appliedRange: [UInt32: String] = [:]
    /// A radio's group from the kernel (`exact_radio_group`; x2apps survey #2).
    var radioGroup: ((UInt32) -> RadioGroup)?
    /// Each `progress`'s spinner (ProgressMac.swift): a view, not a
    /// control, so beside `controls`; and those turning, which AppKit does
    /// not say.
    var spinners: [UInt32: NSProgressIndicator] = [:]
    var animating: Set<UInt32> = []
    /// Each determinate `progress`'s bar (LLP 1116 D8), beside them.
    var bars: [UInt32: NSProgressIndicator] = [:]

    init(_ presenter: Presenter) { self.presenter = presenter }

    func isOn(_ control: NSControl) -> Bool {
        ((control as? NSSwitch)?.state ?? (control as? NSButton)?.state) == .on
    }

    func setOn(_ control: NSControl, _ on: Bool) {
        let state: NSControl.StateValue = on ? .on : .off
        if let s = control as? NSSwitch, s.state != state { s.state = state }
        if let b = control as? NSButton, b.state != state { b.state = state }
    }

    /// The control the node shows, made (or remade, when its kind changes).
    private func control(for node: NodeView) -> NSControl {
        let kind = ControlKinds.kind(node.props)
        if let existing = controls[node.id], kinds[node.id] == kind { return existing }
        controls.removeValue(forKey: node.id)?.removeFromSuperview()
        menus.removeValue(forKey: node.id)
        appliedRange.removeValue(forKey: node.id)
        let made: NSControl
        switch kind {
        case "switch": made = SwitchMac()
        case "button": made = makeNativeButton(node) // LLP 1069.011
        case "checkbox":
            let box = CheckboxMac(checkboxWithTitle: "", target: nil, action: nil)
            box.imagePosition = .imageOnly
            made = box
        case "radio": made = RadioButtonMac() // x2apps survey #2
        default: made = makeValueControl(kind)
        }
        made.tag = Int(node.id)
        made.target = self
        made.action = kind == "button" ? #selector(nativePressed(_:)) : kind == "radio" ? #selector(radioClicked(_:))
            : kind == "switch" || kind == "checkbox" ? #selector(changed(_:)) : #selector(valueChanged(_:))
        controls[node.id] = made
        kinds[node.id] = kind
        return made
    }

    /// `moved`: the nodes the batch put under another parent.
    func sync(moved: Set<UInt32> = []) {
        restyled.formUnion(moved)
        let fresh = listed.map { $0.revision != presenter.chrome.revision || $0.views != presenter.views.count } ?? true
        if fresh {
            let owners = ControlKinds.indexed.flatMap { presenter.carrying($0) }.filter { $0.kind == "control" }
            listed = (presenter.chrome.revision, presenter.views.count, owners, Set(owners.map(\.id)))
        }
        let owners = listed!.owners, live = listed!.live
        // A leaving control keeps drawing until its exit ends (LLP 1069.011 D9).
        let leaving = Set(presenter.leaving.values.flatMap { $0.members.map(\.id) })
        for id in (fresh || controls.count != live.count ? Array(controls.keys) : []) where !live.contains(id) && !leaving.contains(id) {
            controls.removeValue(forKey: id)?.removeFromSuperview()
            configured.remove(id); away.remove(id)
            reported.removeValue(forKey: id)
            faces.removeValue(forKey: id)
            kinds.removeValue(forKey: id)
            menus.removeValue(forKey: id)
            lastRange.removeValue(forKey: id)
            appliedRange.removeValue(forKey: id)
        }
        var sizes: [(UInt32, CGSize?)] = []
        let every = everyControlTouched
        defer { touched.removeAll(keepingCapacity: true); restyled.removeAll(keepingCapacity: true); everyControlTouched = false }
        // Whether the batch restyled or moved this view or one above it, asked once for each view on the way up.
        var above: [ObjectIdentifier: Bool] = [:]
        func inherits(_ view: NSView?) -> Bool {
            guard let view else { return false }
            if let known = above[ObjectIdentifier(view)] { return known }
            let answer = ((view as? NodeView).map { restyled.contains($0.id) } ?? false)
                || inherits(presenter.authoredSuperview(of: view) ?? view.superview)
            above[ObjectIdentifier(view)] = answer
            return answer
        }
        // The controls this batch configures, decided before any is: a face is asked for with every other face
        // the pass will ask for (`face`). Out of the window its session is in, a control waits; a session in no
        // window is configured as before, since nothing of it is parked.
        let windowed = presenter.viewport.window != nil
        var due: [NodeView] = []
        for owner in owners {
            let made = configured.contains(owner.id) && controls[owner.id] != nil
            if made, windowed, owner.window == nil { away.insert(owner.id); continue }
            let back = away.remove(owner.id) != nil
            if !made || every || back || touched.contains(owner.id) || inherits(owner) { due.append(owner) }
        }
        for owner in due {
            configured.insert(owner.id)
            let control = control(for: owner)
            if owner.style["appearance"]?.string == "none" {
                // The author's box is the look; the node keeps its role.
                control.removeFromSuperview()
                continue
            }
            let mount = owner.controlMount
            if control.superview !== mount { mount.addSubview(control) }
            control.isHidden = owner.cssVisibilityHidden
            // `outline: none` (LLP 1104 D6): AppKit draws the control no ring.
            let ring: NSFocusRingType = owner.outlineNone ? .none : .default
            if control.focusRingType != ring { control.focusRingType = ring; control.noteFocusRingMaskChanged() }
            let accent = owner.channels("accent_color").map { TextEngine.color($0) }
            if let b = control as? NativeButtonMac {
                configureNative(b, owner, accent: accent)
            } else if control is NSSwitch || kinds[owner.id] == "checkbox" || kinds[owner.id] == "radio" {
                // @ref LLP 1115.002 D6 — a grouped form's switch is the size its
                // sheet names (`-exact-control-size`, a metric: AppKit's mini).
                if let s = control as? NSSwitch {
                    let named: [String: NSControl.ControlSize] = ["mini": .mini, "small": .small, "large": .large]
                    let size = named[owner.style["control_size"]?.string ?? ""] ?? .regular
                    // AppKit takes the new size's metrics at its next layout: now, so the size reported below is the one drawn.
                    if s.controlSize != size { s.controlSize = size; s.layoutSubtreeIfNeeded() }
                }
                if let on = owner.props["checked"].map({ $0 == "true" }) { setOn(control, on) }
                // NSSwitch takes the system accent; AppKit gives it no tint.
                (control as? NSButton)?.contentTintColor = accent
            } else {
                configureValue(control, owner, accent: accent)
            }
            if !(control is NativeButtonMac) {
                control.isEnabled = !owner.disabled
                control.setAccessibilityLabel(owner.authoredLabel)
                control.setAccessibilityIdentifier(owner.props["testId"])
            }
            // A native button is sized by its node's box and has answered its fitting size already (D11):
            // its intrinsic size, an AppKit measure on every batch, is read only for the other controls.
            let natural = control is NativeButtonMac ? .zero : kept(control)
            let box = control is NativeButtonMac ? owner.bounds : owner.contentBox()
            // A slider's track spans its box, as the web's does; a native
            // button fills it, its chrome inside (LLP 1069.011 D6); the
            // others keep their own size, centred.
            if control is NativeButtonMac {
                // The box is the button's alignment rect, as its natural size
                // is; its bezel's shadow and insets fall outside it.
                let frame = control.frame(forAlignmentRect: box)
                if control.frame != frame { control.frame = frame }
                if let button = control as? NativeButtonMac, let face = button.written?.face {
                    ButtonConfigurationMac.corners(face, to: button, size: box.size)
                }
            } else {
                let width = control is NSSlider ? box.width : natural.width
                control.frame = CGRect(x: box.midX - width / 2, y: box.midY - natural.height / 2,
                                       width: width, height: natural.height)
            }
            // Buttons already received their fitting answer before publication (D11).
            if !(control is NativeButtonMac), reported[owner.id] != natural {
                reported[owner.id] = natural
                sizes.append((owner.id, natural))
            }
        }
        // Published outside the batch being applied, as images' are.
        // A control destroyed before then reports nothing.
        if !sizes.isEmpty {
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                let live = sizes.filter { self.controls[$0.0] != nil && self.presenter.views[$0.0] != nil }
                if !live.isEmpty { self.presenter.onIntrinsic?(live) }
            }
        }
        syncProgress()
    }

    @objc private func changed(_ sender: NSControl) {
        let id = UInt32(sender.tag)
        guard let node = presenter.views[id] else { return }
        // Read first: the press's batch syncs a bound control back to `checked`.
        let on = isOn(sender)
        // A click on a checkbox is heard before its `input` and `change` (HTML
        // dispatches `click`, then fires those as its activation behaviour).
        if node.handlers.contains("press") { presenter.press(id) }
        presenter.checked(id, on)
        // The committed state is authoritative: an action that refused the
        // toggle snaps the control back (D4).
        if let committed = presenter.views[id]?.props["checked"].map({ $0 == "true" }) { setOn(sender, committed) }
    }

    /// The agent's `tap` (LLP 1069.001 D9): the control's own activation.
    func activate(_ node: NodeView) -> Bool? {
        // A native button takes the ordinary click path (LLP 1069.011 D10).
        guard let control = controls[node.id], !(control is NativeButtonMac) else { return nil }
        guard control.window != nil, control.isEnabled, !node.inert, !control.isHiddenOrHasHiddenAncestor else { return false }
        // A click at a slider's middle moves its knob there, as a click
        // does; AppKit's own tracking loop would wait for a mouse-up the
        // agent's synthesized click never hands it.
        // A date field takes the focus, as a click into it does.
        if let picker = control as? NSDatePicker { return picker.window?.makeFirstResponder(picker) ?? false }
        if let slider = control as? NSSlider {
            _ = typeRange(slider, node, String((slider.minValue + slider.maxValue) / 2))
            return true
        }
        if !["checkbox", "switch", "radio"].contains(kinds[node.id]) { return openValue(control) }
        if let b = control as? NSButton { b.performClick(nil) } else {
            setOn(control, !isOn(control))
            changed(control)
        }
        return true
    }

    func observation(_ node: NodeView) -> [String: Any]? {
        if let progress = progressObservation(node) { return progress }
        guard let control = controls[node.id] else { return nil }
        if let b = control as? NativeButtonMac { return nativeObservation(b) }
        if let value = valueObservation(control) {
            return value.merging(["size": [Agent.r2(control.frame.width), Agent.r2(control.frame.height)]]) { a, _ in a }
        }
        return ["view": control is NSSwitch ? "NSSwitch" : control is RadioButtonMac ? "NSButton(radio)" : "NSButton(checkbox)", "on": isOn(control),
                "size": [Agent.r2(control.frame.width), Agent.r2(control.frame.height)]]
    }

    func reset() {
        for control in controls.values { control.removeFromSuperview() }
        controls.removeAll()
        for spinner in spinners.values { spinner.stopAnimation(nil); spinner.removeFromSuperview() }
        spinners.removeAll()
        for bar in bars.values { bar.removeFromSuperview() }
        bars.removeAll()
        animating.removeAll()
        reported.removeAll()
        kinds.removeAll()
        menus.removeAll()
        lastRange.removeAll()
        appliedRange.removeAll()
        faces.removeAll()
        listed = nil; away.removeAll(); configured.removeAll(); naturalSizes.removeAll(); everyControlTouched = true
    }
}

#endif
