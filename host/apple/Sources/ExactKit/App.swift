#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import CExact
import Foundation
import QuartzCore

/// The one Exact app this process links (LLP 1031 D1, D11).
public final class ExactApp {
    public static let shared = ExactApp()
    /// Where an image source, a declared font, or a deck page resolves:
    /// `EXACT_ASSETS`, else the bundle (iOS) or the working directory
    /// (macOS) — the way a page resolves against its URL.
    public var assetRoot: URL {
        didSet {
            if resolver != nil && !resolver.isComplete { resolver = AssetResolver(root: assetRoot) }
        }
    }
    /// The complete generation last accepted by every live session. A new
    /// connection can recognize it without restarting carried app state.
    private var devGeneration: String?
    private var devProgram: String?
    /// The plan last applied across the app, also used by newly created sessions.
    private(set) var lastPlan: Data?
    private(set) var lastModule: ExactModule?
    private(set) package var resolver: AssetResolver!
    private var transaction = false
    /// A retryable image preparation; the current sessions remain live.
    public private(set) var generationPending = false; private(set) var requestsHeld = false
    private var notifications: [() -> Void] = []
    func deliver(_ body: @escaping () -> Void) {
        if transaction { notifications.append(body) } else { body() }
    }
    /// Retained for the app lifetime; embedded-only apps supply none.
    public var lifecycle: ExactAppLifecycle?
    private(set) var selectedToken: UInt64 = 0
    private var sessionRefs: [WeakSession] = []
    let notificationProcessID = UUID().uuidString
    let sharedStore = SharedStoreRefresh()
    /// Every live session, in creation order.
    public var sessions: [ExactSession] { sessionRefs.compactMap(\.session) }
    /// The one dev connection (D11): the app URL `dev.mjs` prints, resolved
    /// and subscribed once; every `{seq}` applies to every session.
    private(set) var connection: PlanURL?
    private init() {
        WidgetContainer.configure()
        #if canImport(UIKit)
        let fallback = Bundle.main.bundlePath
        #else
        let fallback =
            Bundle.main.bundleURL.pathExtension == "app"
            ? (Bundle.main.resourcePath ?? Bundle.main.bundlePath) : FileManager.default.currentDirectoryPath
        #endif
        assetRoot = URL(fileURLWithPath: ExactEnv.environment["EXACT_ASSETS"] ?? fallback, isDirectory: true)
        resolver = AssetResolver(root: assetRoot)
        observeSharedStore()
    }
    /// The immutable Rust executor policy carried by this binary's bake.
    public var rustPolicy: (mode: String, target: String) {
        let bytes = Runtime.bakedCompat()
        let json = (try? JSONSerialization.jsonObject(with: bytes) as? [String: Any]) ?? [:]
        let inputs = json["inputs"] as? [String: Any] ?? [:]
        return (inputs["rustMode"] as? String ?? "off", json["target"] as? String ?? "")
    }
    /// Install a composition's launch generation before creating sessions.
    public func installInitial(_ generation: ExactGeneration) {
        precondition(sessions.isEmpty, "install the initial generation before creating sessions")
        resolver = generation.assets
        lastPlan = generation.plan
        lastModule = generation.module
        selectedToken = generation.token
    }

    func fallBackFromInitial(reason: String) -> Bool {
        guard selectedToken != 0, !sessions.contains(where: \.booted) else { return false }
        lifecycle?.initialGenerationRefused(self, token: selectedToken, reason: reason)
        resolver = AssetResolver(root: assetRoot)
        lastPlan = nil
        lastModule = nil
        selectedToken = 0
        return true
    }

    func firstPixel(_ token: UInt64) {
        lifecycle?.firstPixel(self, token: token)
        WidgetContainer.publishAfterFirstPixel(resolver)
    }

    /// Refresh generic delivery facts after a composition-owned event.
    public func refreshDelivery() {
        for session in sessions { session.apply(session.runtime.deliverySync()) }
    }
    func handleCommand(_ name: String) -> Bool { lifecycle?.handleCommand(name, app: self) ?? false }

    /// A session of this app: one runtime, unbooted until `boot` or its view's first layout.
    /// Creates platform views: on iOS call from the application/scene/controller
    /// lifecycle, not top-level code before UIApplicationMain (LLP 1012's timing reduction).
    public func makeSession(delegate: ExactSessionDelegate? = nil, label: String = "") -> ExactSession {
        let s = ExactSession(app: self, label: label.isEmpty ? "session-\(sessionRefs.count + 1)" : label)
        s.delegate = delegate
        sessionRefs.append(WeakSession(s))
        sessionRefs.removeAll { $0.session == nil }
        return s
    }

    func forget(_ session: ExactSession) {
        sessionRefs.removeAll { $0.session == nil || $0.session === session }
    }

    /// Connect to the app URL (LLP 1023 D1–D3): the envelope resolved, the
    /// plan fetched and verified, applied to every session, and the
    /// server's events subscribed so an edit re-fetches. Replaces a prior
    /// connection.
    public func connect(_ url: String) {
        connection?.close()
        connection = PlanURL.open(
            url, program: devProgram,
            acceptProgram: { [weak self] program in
                guard let self else { return false }
                if let old = self.devProgram { return old == program }
                self.devProgram = program
                return true
            }, current: { [weak self] in self?.devGeneration }, plan: { [weak self] in self?.sessions.first { $0.booted && $0.state != .destroyed }?.runtime.planDigest() },
            waiting: { [weak self] in self?.generationPending == true }, held: { [weak self] in self?.requestsHeld == true }, design: { [weak self] in self?.design($0) ?? "closed" },
            channel: { [weak self] in self?.channel($0) },
            apply: { [weak self] candidate, label, letGo in
                guard let self else { return false }
                let resolver = candidate.assets
                return self.applyTogether(candidate.plan, label: label, resolver: resolver, token: 0, identity: candidate.identity, module: candidate.module, letGo: letGo, commit: { true })
            })
        connection?.onDevStatus = { [weak self] in self?.showDevStatus($0) }  // the old one's went at its close
    }

    public func disconnect() {
        connection?.close()
        connection = nil
    }

    /// The connection's page URL, when connected (what a deck's `//` source resolves against).
    public var connectedPage: URL? { connection?.page }
    /// The connection's status line for a dev menu, or nil.
    public var connectionStatus: String? {
        connection.map { "\($0.page)\($0.devStatus.map { " (\($0.said))" } ?? ($0.terminal != nil ? " (rebuild the host)" : generationPending ? " (preparing Rust update)" : " (live)"))" }
    }
    /// Re-resolve the connection (the menu's Reload; clears a `{rebuilt}` stop).
    public func reloadConnection() { connection?.reload() }

    /// A plan is one app generation: every session accepts before any swaps.
    @discardableResult
    public func apply(_ bytes: Data, label: String = "plan") -> Bool {
        applyTogether(bytes, label: label, resolver: resolver, token: 0, commit: { true })
    }

    /// Every session accepts before the durable commit. Its callback runs on
    /// main while the runtime owner holds replies; it cannot reenter a runtime.
    @discardableResult
    public func applyGeneration(_ candidate: ExactGeneration, label: String = "generation", commit: () -> Bool) -> Bool {
        applyTogether(candidate.plan, label: label, resolver: candidate.assets, token: candidate.token, module: candidate.module, commit: commit)
    }

    private func applyTogether(_ bytes: Data, label: String, resolver candidateResolver: AssetResolver, token: UInt64, identity: String? = nil, module: ExactModule? = nil, letGo: Bool = false, commit: () -> Bool) -> Bool
    {
        generationPending = false; requestsHeld = false
        guard !transaction else { return false }
        let module = module ?? lastModule
        let participants = sessions.filter { $0.state != .destroyed }
        // Validate a staged plan without starting a hidden runner's requests.
        if participants.isEmpty {
            let validator = ExactSession(app: self, label: "")
            defer { validator.destroy() }
            guard validator.prepare(bytes, resolver: candidateResolver, token: token, module: module, size: CGSize(width: 1, height: 1), letGo: letGo) != nil else { return false }
            validator.runtime.discardPlan()
        }
        guard
            let prepared = prepareGeneration(
                participants,
                prepare: { session in
                    session.prepare(bytes, resolver: candidateResolver, token: token, module: module, letGo: letGo)
                })
        else {
            generationPending = participants.contains { $0.modulePending || $0.requestsPending }
            requestsHeld = participants.contains { $0.requestsPending }
            for session in participants { session.runtime.discardPlan() }
            return false
        }
        let surfaces = SurfacesLink.installed
        let shaderSources = surfaces?.shadersLoaded == true ? candidateResolver.shaderSources() : nil
        let shadersAccepted = shaderSources.map { surfaces?.acceptsShaders($0) == true } ?? true
        guard candidateResolver.refusal == nil, shadersAccepted else {
            for (session, _) in prepared { session.runtime.discardPlan() }
            return false
        }
        for (session, candidate) in prepared { session.configureCommit(candidate) }
        transaction = true
        let result = Runtime.commitTogether(prepared.map { $0.0.runtime }, commit: commit)
        guard let committed = result.batches else {
            transaction = false; generationPending = result.pending; requestsHeld = result.requests
            for (session, _) in prepared { session.cancelCommit() }
            return false
        }
        let batches = zip(prepared, committed).map { pair, batch in (pair.0, pair.0.commit(pair.1, batch: batch)) }
        resolver = candidateResolver
        #if EXACT_WIDGET_GALLERY
        NotificationCenter.default.post(name: Notification.Name("ExactWidgetGeneration"), object: self)
        #endif
        devGeneration = identity
        lastPlan = bytes
        lastModule = module
        selectedToken = token
        if let shaderSources { surfaces?.replaceShaders(shaderSources) }
        for (session, _) in batches { session.awaitReplacementTree() }  // each report waits for its own tree: one tree may retint the window; each runtime held till its own reset ends (ResetGate)
        MainActor.assumeIsolated { HeldState.swap() }  // the hold let go unsaid before the old tree goes, real input none till after (HeldState.swift)
        for (session, batch) in batches { session.presentCommitted(batch, label: label) }
        for session in participants { session.apply(session.runtime.deliverySync()) }
        MainActor.assumeIsolated { HeldState.swapping = false }
        rehold()
        transaction = false
        let callbacks = notifications
        notifications = []
        for callback in callbacks { callback() }
        return true
    }

    /// A local asset as pinned bytes, verified before any native consumer uses it.
    func assetBytes(_ name: String) -> Data? { resolver.bytes(name) }
    func bundledAsset(_ name: String) -> URL? { resolver.bundledURL(name) }

    /// Path-only native consumers receive a private materialization of those
    /// same pinned bytes; removed names never reach the embedded directory.
    public func resolveAsset(_ name: String) -> URL? { resolver.url(name) }

    /// Explicit session replacement invalidates the connection's claim that
    /// every session still runs its last accepted complete generation.
    func invalidateDevGeneration() { devGeneration = nil }

}
