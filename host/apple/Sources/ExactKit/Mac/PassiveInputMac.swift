#if os(macOS)
import AppKit
import ExactCore

enum PassiveInput {
    // MARK: Pre-echo

    /// A key echoed in this turn: its path's entries and its `beforeinput`, then what the turn deferred after it.
    struct Echo {
        let token: Int
        weak var node: NodeView?
        var entries: [Runtime.Dispatch]
        /// `runtime.deferred.count` when the key began: the events after it are the key's own.
        let mark: Int
        var before: (value: String, selection: NSRange)?
    }
    nonisolated(unsafe) static var next = 0
    /// Rollback state of echoes whose batches have not landed, by token.
    struct Rollback {
        weak var node: NodeView?
        let before: (value: String, selection: NSRange)
        let after: String
    }

    /// Why `press` at `target` stays synchronous, or nil when it may be echoed.
    static func refusal(_ target: NodeView?, _ press: KeyPress, presenter: Presenter) -> String? {
        guard let node = target, let editor = node.textArea, editor.window?.firstResponder === editor else { return "not a focused text area" }
        if press.composing || editor.hasMarkedText() { return "composition" }
        let (name, command) = PassiveKeyPolicy.keyName(press.chord)
        if command { return "command chord" }
        if !PassiveKeyPolicy.printable(name) { return "not printable: \(name)" }
        if editor.selectedRange().length != 0 { return "selection" }
        if presenter.session?.runtime.deferred.isEmpty != true { return "earlier events pending" }
        if !presenter.echoRollbacks.isEmpty || presenter.passiveEcho != nil { return "echo in flight" }
        if !PassiveKeyPolicy.declaredPassive(node.props["dataset"]) { return "not declared passive" }
        if !editor.isEditable || node.disabled { return "not editable" }
        return nil
    }
}

extension Presenter {
    /// The key path is not sent now; the session sends it with the key's own events at the turn's end.
    func beginPassiveEcho(_ target: NodeView?, _ press: KeyPress, _ entries: [(UInt32, String, Bool)]) -> Bool {
        guard !press.up, let session else { return false }
        guard PassiveInput.refusal(target, press, presenter: self) == nil else { return false }
        guard let target else { return false }
        PassiveInput.next += 1
        passiveEcho = PassiveInput.Echo(
            token: PassiveInput.next, node: target, entries: entries.map { Runtime.Dispatch($0.0, $0.2 ? 44 : 6, $0.1, path: true, echo: true) },
            mark: session.runtime.deferred.count, before: nil)
        session.owePassiveFlush()
        return true
    }
    /// The echoed key's `beforeinput`, recorded instead of asked; true when it was recorded.
    func recordPassiveBeforeInput(_ node: NodeView, _ heard: Bool, _ payload: String, value: String, selection: NSRange) -> Bool {
        guard var echo = passiveEcho, echo.node === node, echo.before == nil else { return false }
        if heard { echo.entries.append(Runtime.Dispatch(node.id, 53, payload, echo: true)) }
        echo.before = (value, selection)
        passiveEcho = echo
        return true
    }
}

extension ExactSession {
    func owePassiveFlush() { owePassiveDeferredFlush() }

    /// The turn's deferred events, and an echoed key's, to the owner without main waiting.
    func flushPassiveInput() {
        var events = runtime.takeDeferred()
        var token = -1
        if var echo = presenter.passiveEcho {
            presenter.passiveEcho = nil
            let mark = min(echo.mark, events.count)
            var own = Array(events[mark...])
            for i in own.indices { own[i].echo = true }
            events = Array(events[..<mark]) + echo.entries + own
            if let node = echo.node, let before = echo.before {
                token = echo.token
                presenter.echoRollbacks[token] = PassiveInput.Rollback(node: node, before: before, after: node.fieldText)
            }
            echo.entries = []
        }
        guard !events.isEmpty else { return }
        noticesInFlight += 1
        let captured = generation
        runtime.dispatchManyAsync(events, now: now()) { [weak self] batch in
            var batch = batch
            batch.passiveEcho = token
            self?.publish(.notice, batch, captured)
        }
    }

    /// A passive-input batch landed (`landFill`): a prevented echo is rolled back. Its `preventDefault` and
    /// `stopPropagation` are not the dispatch it may land inside.
    func landPassiveInput(_ batch: Batch, prevented outer: Bool, stopped outerStopped: Bool) {
        let prevented = batch.ops.contains { $0.op == .command && ($0.payload["name"] as? String) == "preventDefault" }
        presenter.defaultPrevented = outer
        presenter.propagationStopped = outerStopped
        guard batch.passiveEcho > 0, let echo = presenter.echoRollbacks.removeValue(forKey: batch.passiveEcho) else { return }
        guard prevented, let node = echo.node, presenter.views[node.id] === node,
            let editor = node.textArea, node.fieldText == echo.after
        else { return }
        node.writeValue(echo.before.value, into: editor)
        presenter.fieldSelections.quietly { editor.setSelectedRange(echo.before.selection) }
    }
}
#endif
