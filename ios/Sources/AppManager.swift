import CallKit
import CryptoKit
import Foundation
import Observation
import os

protocol AppCore: AnyObject, Sendable {
    func dispatch(action: AppAction)
    func listenForUpdates(reconciler: AppReconciler)
    func state() -> AppState
}

extension FfiApp: AppCore {}

protocol NsecStore: AnyObject {
    func getNsec() -> String?
    func setNsec(_ nsec: String)
    func clearNsec()
}

extension KeychainNsecStore: NsecStore {}

@MainActor
@Observable
final class AppManager: AppReconciler {
    private let core: AppCore
    var state: AppState
    private var lastRevApplied: UInt64
    private let nsecStore: NsecStore
    /// True while we're waiting for a stored session to be restored by Rust.
    var isRestoringSession: Bool = false
    private let callAudioSession = CallAudioSessionCoordinator()
    @ObservationIgnored
    private var callKitCoordinator: CallKitCoordinator?
    var callTimelineEventsByChatId: [String: [CallTimelineEvent]] = [:]
    private var loggedCallTimelineKeys: Set<String> = []

    init(core: AppCore, nsecStore: NsecStore) {
        self.core = core
        self.nsecStore = nsecStore

        let initial = core.state()
        self.state = initial
        self.lastRevApplied = initial.rev
        callAudioSession.apply(activeCall: initial.activeCall)
        if let initialCall = initial.activeCall {
            callKit().sync(
                previous: nil,
                current: initialCall,
                displayName: callDisplayName(for: initialCall, in: initial)
            )
        }

        core.listenForUpdates(reconciler: self)

        if let nsec = nsecStore.getNsec(), !nsec.isEmpty {
            isRestoringSession = true
            core.dispatch(action: .restoreSession(nsec: nsec))
        }
    }

    convenience init() {
        let fm = FileManager.default
        let dataDirUrl = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dataDir = dataDirUrl.path
        let nsecStore = KeychainNsecStore()

        // UI tests need a clean slate and a way to inject relay overrides without relying on
        // external scripts.
        let env = ProcessInfo.processInfo.environment
        let uiTestReset = env["PIKA_UI_TEST_RESET"] == "1"
        if uiTestReset {
            nsecStore.clearNsec()
            try? fm.removeItem(at: dataDirUrl)
        }
        try? fm.createDirectory(at: dataDirUrl, withIntermediateDirectories: true)

        // Optional relay override (matches `tools/run-ios` environment variables).
        let relays = (env["PIKA_RELAY_URLS"] ?? env["PIKA_RELAY_URL"])?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let kpRelays = (env["PIKA_KEY_PACKAGE_RELAY_URLS"] ?? env["PIKA_KP_RELAY_URLS"])?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let callMoqUrl = (env["PIKA_CALL_MOQ_URL"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let callBroadcastPrefix = (env["PIKA_CALL_BROADCAST_PREFIX"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let moqProbeOnStart = (env["PIKA_MOQ_PROBE_ON_START"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        ensureDefaultConfig(
            dataDirUrl: dataDirUrl,
            uiTestReset: uiTestReset,
            relays: relays,
            kpRelays: kpRelays,
            callMoqUrl: callMoqUrl,
            callBroadcastPrefix: callBroadcastPrefix,
            moqProbeOnStart: moqProbeOnStart
        )

        let core = FfiApp(dataDir: dataDir)
        self.init(core: core, nsecStore: nsecStore)
    }

    nonisolated func reconcile(update: AppUpdate) {
        Task { @MainActor [weak self] in
            self?.apply(update: update)
        }
    }

    func apply(update: AppUpdate) {
        let updateRev = update.rev

        // Side-effect updates must not be lost: `AccountCreated` carries an `nsec` that isn't in
        // AppState snapshots (by design). Store it even if the update is stale w.r.t. rev.
        if case .accountCreated(_, let nsec, _, _) = update {
            let existing = nsecStore.getNsec() ?? ""
            if existing.isEmpty && !nsec.isEmpty {
                nsecStore.setNsec(nsec)
            }
        }

        // The stream is full-state snapshots; drop anything stale.
        if updateRev <= lastRevApplied { return }

        lastRevApplied = updateRev
        switch update {
        case .fullState(let s):
            let previousState = state
            state = s
            callAudioSession.apply(activeCall: s.activeCall)
            if previousState.activeCall != nil || s.activeCall != nil {
                callKit().sync(
                    previous: previousState.activeCall,
                    current: s.activeCall,
                    displayName: s.activeCall.map { callDisplayName(for: $0, in: s) }
                )
            }
            recordCallTimelineTransition(from: previousState.activeCall, to: s.activeCall)
            if previousState.auth != .loggedOut, s.auth == .loggedOut {
                callTimelineEventsByChatId = [:]
                loggedCallTimelineKeys = []
            }
            if isRestoringSession {
                // Clear once we've transitioned away from login (success) or if
                // the router settles on login (restore failed / nsec invalid).
                if s.auth != .loggedOut || s.router.defaultScreen != .login {
                    isRestoringSession = false
                }
            }
        case .accountCreated(_, let nsec, _, _):
            // Required by spec-v2: native stores nsec; Rust never persists it.
            if !nsec.isEmpty {
                nsecStore.setNsec(nsec)
            }
            state.rev = updateRev
            callAudioSession.apply(activeCall: state.activeCall)
        }
    }

    func dispatch(_ action: AppAction) {
        core.dispatch(action: action)
    }

    private func callKit() -> CallKitCoordinator {
        if let existing = callKitCoordinator {
            return existing
        }

        let coordinator = CallKitCoordinator(
            actions: .init(
                startCall: { [weak self] chatId in
                    Task { @MainActor in
                        self?.dispatch(.startCall(chatId: chatId))
                    }
                },
                acceptCall: { [weak self] chatId in
                    Task { @MainActor in
                        self?.dispatch(.acceptCall(chatId: chatId))
                    }
                },
                rejectCall: { [weak self] chatId in
                    Task { @MainActor in
                        self?.dispatch(.rejectCall(chatId: chatId))
                    }
                },
                endCall: { [weak self] in
                    Task { @MainActor in
                        self?.dispatch(.endCall)
                    }
                }
            )
        )
        callKitCoordinator = coordinator
        return coordinator
    }

    func startCall(chatId: String) {
        callKit().requestStartCall(
            chatId: chatId,
            handleValue: callHandleValue(chatId: chatId)
        ) { [weak self] in
            Task { @MainActor in
                self?.dispatch(.startCall(chatId: chatId))
            }
        }
    }

    func acceptCall(chatId: String) {
        guard let activeCall = state.activeCall, activeCall.chatId == chatId else {
            dispatch(.acceptCall(chatId: chatId))
            return
        }
        callKit().requestAnswer(call: activeCall) { [weak self] in
            Task { @MainActor in
                self?.dispatch(.acceptCall(chatId: chatId))
            }
        }
    }

    func rejectCall(chatId: String) {
        guard let activeCall = state.activeCall, activeCall.chatId == chatId else {
            dispatch(.rejectCall(chatId: chatId))
            return
        }
        callKit().requestEnd(call: activeCall) { [weak self] in
            Task { @MainActor in
                self?.dispatch(.rejectCall(chatId: chatId))
            }
        }
    }

    func endCall() {
        guard let activeCall = state.activeCall else {
            dispatch(.endCall)
            return
        }
        callKit().requestEnd(call: activeCall) { [weak self] in
            Task { @MainActor in
                self?.dispatch(.endCall)
            }
        }
    }

    func login(nsec: String) {
        if !nsec.isEmpty {
            nsecStore.setNsec(nsec)
        }
        dispatch(.login(nsec: nsec))
    }

    func logout() {
        nsecStore.clearNsec()
        dispatch(.logout)
    }

    func onForeground() {
        // Foreground is a lifecycle action; Rust owns state changes and side effects.
        dispatch(.foregrounded)
    }

    func refreshMyProfile() {
        dispatch(.refreshMyProfile)
    }

    func saveMyProfile(name: String, about: String) {
        dispatch(.saveMyProfile(name: name, about: about))
    }

    func uploadMyProfileImage(data: Data, mimeType: String) {
        guard !data.isEmpty else { return }
        dispatch(
            .uploadMyProfileImage(
                imageBase64: data.base64EncodedString(),
                mimeType: mimeType
            )
        )
    }

    func getNsec() -> String? {
        nsecStore.getNsec()
    }

    private func recordCallTimelineTransition(from old: CallState?, to new: CallState?) {
        guard let new else { return }

        if new.status.isLive {
            appendCallTimelineEventIfNeeded(
                key: "\(new.callId):started",
                chatId: new.chatId,
                text: "Call started"
            )
            return
        }

        guard case let .ended(reason) = new.status else { return }
        let previousStatus = old?.callId == new.callId ? old?.status : nil
        appendCallTimelineEventIfNeeded(
            key: "\(new.callId):ended",
            chatId: new.chatId,
            text: callEndedTimelineText(
                reason: reason,
                previousStatus: previousStatus,
                startedAt: new.startedAt
            )
        )
    }

    private func appendCallTimelineEventIfNeeded(key: String, chatId: String, text: String) {
        guard loggedCallTimelineKeys.insert(key).inserted else { return }
        var events = callTimelineEventsByChatId[chatId] ?? []
        events.append(CallTimelineEvent(id: key, chatId: chatId, text: text))
        if events.count > 20 {
            events.removeFirst(events.count - 20)
        }
        callTimelineEventsByChatId[chatId] = events
    }

    private func callDisplayName(for call: CallState, in appState: AppState) -> String {
        if let currentChat = appState.currentChat, currentChat.chatId == call.chatId {
            if currentChat.isGroup {
                return currentChat.groupName ?? "Group"
            }
            if let peer = currentChat.members.first {
                return peer.name ?? shortNpub(peer.npub)
            }
        }

        if let summary = appState.chatList.first(where: { $0.chatId == call.chatId }) {
            if summary.isGroup {
                return summary.groupName ?? "Group"
            }
            if let peer = summary.members.first {
                return peer.name ?? shortNpub(peer.npub)
            }
        }

        return shortNpub(call.peerNpub)
    }

    private func callHandleValue(chatId: String) -> String {
        if let currentChat = state.currentChat, currentChat.chatId == chatId {
            if currentChat.isGroup {
                return currentChat.groupName ?? "Group"
            }
            if let peer = currentChat.members.first {
                return peer.name ?? peer.npub
            }
        }

        if let summary = state.chatList.first(where: { $0.chatId == chatId }) {
            if summary.isGroup {
                return summary.groupName ?? "Group"
            }
            if let peer = summary.members.first {
                return peer.name ?? peer.npub
            }
        }

        return chatId
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count > 16 else { return npub }
        return "\(npub.prefix(8))...\(npub.suffix(4))"
    }
}

private extension AppUpdate {
    var rev: UInt64 {
        switch self {
        case .fullState(let s): return s.rev
        case .accountCreated(let rev, _, _, _): return rev
        }
    }
}

private func ensureDefaultConfig(
    dataDirUrl: URL,
    uiTestReset: Bool,
    relays: String,
    kpRelays: String,
    callMoqUrl: String,
    callBroadcastPrefix: String,
    moqProbeOnStart: String
) {
    // Ensure call config exists even when no env overrides are set (call runtime requires `call_moq_url`).
    // If the file already exists, only fill missing keys to avoid clobbering user/tooling overrides.
    //
    // Important: do NOT write `disable_network` here. Tests rely on `PIKA_DISABLE_NETWORK=1`
    // taking effect when the config file omits `disable_network`.
    let defaultMoqUrl = "https://us-east.moq.logos.surf/anon"
    let defaultBroadcastPrefix = "pika/calls"

    let wantsOverride = uiTestReset
        || !relays.isEmpty
        || !kpRelays.isEmpty
        || !callMoqUrl.isEmpty
        || !callBroadcastPrefix.isEmpty
        || moqProbeOnStart == "1"

    let path = dataDirUrl.appendingPathComponent("pika_config.json")
    var obj: [String: Any] = [:]
    if let data = try? Data(contentsOf: path),
       let decoded = try? JSONSerialization.jsonObject(with: data, options: []),
       let dict = decoded as? [String: Any] {
        obj = dict
    }

    func isMissingOrBlank(_ key: String) -> Bool {
        guard let raw = obj[key] else { return true }
        let v = String(describing: raw).trimmingCharacters(in: .whitespacesAndNewlines)
        return v.isEmpty || v == "(null)"
    }

    var changed = false

    let resolvedCallMoqUrl = callMoqUrl.isEmpty ? defaultMoqUrl : callMoqUrl
    if isMissingOrBlank("call_moq_url") {
        obj["call_moq_url"] = resolvedCallMoqUrl
        changed = true
    }

    let resolvedCallBroadcastPrefix = callBroadcastPrefix.isEmpty ? defaultBroadcastPrefix : callBroadcastPrefix
    if isMissingOrBlank("call_broadcast_prefix") {
        obj["call_broadcast_prefix"] = resolvedCallBroadcastPrefix
        changed = true
    }

    if wantsOverride {
        let relayItems = relays
            .split(separator: ",")
            .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        var kpItems = kpRelays
            .split(separator: ",")
            .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }

        if kpItems.isEmpty {
            kpItems = relayItems
        }

        if moqProbeOnStart == "1" && (obj["moq_probe_on_start"] as? Bool) != true {
            obj["moq_probe_on_start"] = true
            changed = true
        }

        if !relayItems.isEmpty {
            obj["relay_urls"] = relayItems
            obj["key_package_relay_urls"] = kpItems
            changed = true
        }
    }

    guard changed else { return }
    guard let out = try? JSONSerialization.data(withJSONObject: obj, options: []) else { return }
    try? out.write(to: path, options: .atomic)
}

private final class CallKitCoordinator: NSObject {
    struct Actions {
        let startCall: (String) -> Void
        let acceptCall: (String) -> Void
        let rejectCall: (String) -> Void
        let endCall: () -> Void
    }

    private struct PendingOutgoing {
        let chatId: String
        let createdAt: Date
        var startActionPerformed: Bool
    }

    private struct CallMeta {
        let chatId: String
        let status: CallStatus
    }

    private enum CallDirection {
        case incoming
        case outgoing
    }

    private let actions: Actions
    private let provider: CXProvider
    private let callController = CXCallController()
    private let callObserver = CXCallObserver()
    private let logger = Logger(subsystem: "com.justinmoon.pika", category: "CallKit")

    private var pendingOutgoingByUUID: [UUID: PendingOutgoing] = [:]
    private var callIdToUUID: [String: UUID] = [:]
    private var uuidToCallId: [UUID: String] = [:]
    private var callMetaByCallId: [String: CallMeta] = [:]
    private var directionByCallId: [String: CallDirection] = [:]
    private var incomingReportedCallIds: Set<String> = []
    private var endedReportedCallIds: Set<String> = []

    init(actions: Actions) {
        self.actions = actions

        let config = CXProviderConfiguration(localizedName: "Pika")
        config.supportsVideo = false
        config.supportedHandleTypes = [.generic]
        config.maximumCallsPerCallGroup = 1
        config.maximumCallGroups = 1
        config.includesCallsInRecents = false

        self.provider = CXProvider(configuration: config)
        super.init()
        provider.setDelegate(self, queue: nil)
        callObserver.setDelegate(self, queue: nil)
    }

    deinit {
        provider.invalidate()
    }

    func requestStartCall(chatId: String, handleValue: String, fallback: @escaping () -> Void) {
        onMain { [weak self] in
            self?.requestStartCallOnMain(chatId: chatId, handleValue: handleValue, fallback: fallback)
        }
    }

    func requestAnswer(call: CallState, fallback: @escaping () -> Void) {
        onMain { [weak self] in
            self?.requestAnswerOnMain(call: call, fallback: fallback)
        }
    }

    func requestEnd(call: CallState, fallback: @escaping () -> Void) {
        onMain { [weak self] in
            self?.requestEndOnMain(call: call, fallback: fallback)
        }
    }

    func sync(previous: CallState?, current: CallState?, displayName: String?) {
        onMain { [weak self] in
            self?.syncOnMain(previous: previous, current: current, displayName: displayName)
        }
    }

    private func requestStartCallOnMain(chatId: String, handleValue: String, fallback: @escaping () -> Void) {
        let uuid = UUID()
        info("requestStartCall chatId=\(chatId) uuid=\(uuid.uuidString)")
        pendingOutgoingByUUID[uuid] = PendingOutgoing(
            chatId: chatId,
            createdAt: Date(),
            startActionPerformed: false
        )

        let handle = CXHandle(type: .generic, value: normalizedHandleValue(handleValue))
        let action = CXStartCallAction(call: uuid, handle: handle)
        action.isVideo = false

        callController.request(CXTransaction(action: action)) { [weak self] error in
            guard let self else { return }
            self.onMain {
                guard let error else {
                    self.info("requestStartCall transaction accepted uuid=\(uuid.uuidString)")
                    return
                }
                self.pendingOutgoingByUUID.removeValue(forKey: uuid)
                self.logTransactionError(kind: "start", error: error)
                fallback()
            }
        }
    }

    private func requestAnswerOnMain(call: CallState, fallback: @escaping () -> Void) {
        let uuid = ensureMappedUUID(for: call, preferPendingOutgoing: false)
        callMetaByCallId[call.callId] = CallMeta(chatId: call.chatId, status: call.status)

        let action = CXAnswerCallAction(call: uuid)
        info("requestAnswer callId=\(call.callId) uuid=\(uuid.uuidString)")
        callController.request(CXTransaction(action: action)) { [weak self] error in
            guard let self else { return }
            self.onMain {
                guard let error else {
                    self.info("requestAnswer transaction accepted uuid=\(uuid.uuidString)")
                    return
                }
                self.logTransactionError(kind: "answer", error: error)
                fallback()
            }
        }
    }

    private func requestEndOnMain(call: CallState, fallback: @escaping () -> Void) {
        let uuid = ensureMappedUUID(for: call, preferPendingOutgoing: false)
        callMetaByCallId[call.callId] = CallMeta(chatId: call.chatId, status: call.status)

        let action = CXEndCallAction(call: uuid)
        info("requestEnd callId=\(call.callId) uuid=\(uuid.uuidString)")
        callController.request(CXTransaction(action: action)) { [weak self] error in
            guard let self else { return }
            self.onMain {
                guard let error else {
                    self.info("requestEnd transaction accepted uuid=\(uuid.uuidString)")
                    return
                }
                self.logTransactionError(kind: "end", error: error)
                fallback()
            }
        }
    }

    private func syncOnMain(previous: CallState?, current: CallState?, displayName: String?) {
        if let previous, let current, previous.callId != current.callId {
            reportAndRetireIfNeeded(call: previous, previousStatus: previous.status)
        }

        guard let current else {
            if let previous {
                reportAndRetireIfNeeded(call: previous, previousStatus: previous.status)
            }
            return
        }

        let direction = direction(for: current, previous: previous)
        let uuid = ensureMappedUUID(for: current, preferPendingOutgoing: direction == .outgoing)

        callMetaByCallId[current.callId] = CallMeta(chatId: current.chatId, status: current.status)

        let previousStatus: CallStatus? = previous?.callId == current.callId ? previous?.status : nil

        if case .ringing = current.status {
            info("sync status=ringing callId=\(current.callId) uuid=\(uuid.uuidString)")
            reportIncomingIfNeeded(call: current, uuid: uuid, displayName: displayName)
        }

        if direction == .outgoing, case .connecting = current.status, !isConnecting(previousStatus) {
            info("sync status=connecting callId=\(current.callId) uuid=\(uuid.uuidString)")
            provider.reportOutgoingCall(with: uuid, startedConnectingAt: Date())
        }

        if direction == .outgoing, case .active = current.status, !isActive(previousStatus) {
            info("sync status=active callId=\(current.callId) uuid=\(uuid.uuidString)")
            provider.reportOutgoingCall(with: uuid, connectedAt: Date())
        }

        if case let .ended(reason) = current.status {
            if !isEnded(previousStatus) {
                reportEndedIfNeeded(
                    callId: current.callId,
                    uuid: uuid,
                    reason: reason,
                    previousStatus: previousStatus
                )
            }
            retireMapping(for: current.callId)
        }
    }

    private func reportIncomingIfNeeded(call: CallState, uuid: UUID, displayName: String?) {
        guard incomingReportedCallIds.insert(call.callId).inserted else { return }

        let handleValue = normalizedHandleValue(displayName ?? call.peerNpub)
        let update = CXCallUpdate()
        update.remoteHandle = CXHandle(type: .generic, value: handleValue)
        update.localizedCallerName = handleValue
        update.hasVideo = false
        update.supportsDTMF = false
        update.supportsHolding = false
        update.supportsGrouping = false
        update.supportsUngrouping = false

        provider.reportNewIncomingCall(with: uuid, update: update) { [weak self] error in
            guard let self else { return }
            self.onMain {
                guard let error else {
                    self.info("reportNewIncomingCall accepted callId=\(call.callId) uuid=\(uuid.uuidString)")
                    return
                }
                self.incomingReportedCallIds.remove(call.callId)
                self.logReportIncomingError(callId: call.callId, error: error)
            }
        }
    }

    private func reportAndRetireIfNeeded(call: CallState, previousStatus: CallStatus?) {
        let uuid = ensureMappedUUID(for: call, preferPendingOutgoing: false)

        if case let .ended(reason) = call.status {
            reportEndedIfNeeded(
                callId: call.callId,
                uuid: uuid,
                reason: reason,
                previousStatus: previousStatus
            )
        } else if endedReportedCallIds.insert(call.callId).inserted {
            provider.reportCall(with: uuid, endedAt: Date(), reason: .remoteEnded)
        }

        retireMapping(for: call.callId)
    }

    private func reportEndedIfNeeded(
        callId: String,
        uuid: UUID,
        reason: String,
        previousStatus: CallStatus?
    ) {
        guard endedReportedCallIds.insert(callId).inserted else { return }
        provider.reportCall(
            with: uuid,
            endedAt: Date(),
            reason: callEndedReason(from: reason, previousStatus: previousStatus)
        )
    }

    private func ensureMappedUUID(for call: CallState, preferPendingOutgoing: Bool) -> UUID {
        if let existing = callIdToUUID[call.callId] {
            return existing
        }

        let uuid: UUID
        if preferPendingOutgoing, let pendingUUID = takePendingOutgoingUUID(chatId: call.chatId) {
            uuid = pendingUUID
        } else {
            uuid = deterministicUUID(for: call.callId)
        }

        bind(callId: call.callId, to: uuid)
        return uuid
    }

    private func bind(callId: String, to uuid: UUID) {
        if let oldUUID = callIdToUUID[callId], oldUUID != uuid {
            uuidToCallId.removeValue(forKey: oldUUID)
        }
        if let oldCallId = uuidToCallId[uuid], oldCallId != callId {
            callIdToUUID.removeValue(forKey: oldCallId)
        }
        callIdToUUID[callId] = uuid
        uuidToCallId[uuid] = callId
    }

    private func retireMapping(for callId: String) {
        if let uuid = callIdToUUID.removeValue(forKey: callId) {
            uuidToCallId.removeValue(forKey: uuid)
            pendingOutgoingByUUID.removeValue(forKey: uuid)
        }
        callMetaByCallId.removeValue(forKey: callId)
        directionByCallId.removeValue(forKey: callId)
    }

    private func takePendingOutgoingUUID(chatId: String) -> UUID? {
        let candidate = pendingOutgoingByUUID
            .filter { $0.value.chatId == chatId }
            .min { $0.value.createdAt < $1.value.createdAt }?
            .key

        guard let candidate else { return nil }
        pendingOutgoingByUUID.removeValue(forKey: candidate)
        return candidate
    }

    private func direction(for call: CallState, previous: CallState?) -> CallDirection {
        if let existing = directionByCallId[call.callId] {
            return existing
        }

        let inferred: CallDirection
        switch call.status {
        case .ringing:
            inferred = .incoming
        case .offering:
            inferred = .outgoing
        case .connecting, .active, .ended:
            if let previous, previous.callId == call.callId {
                if case .ringing = previous.status {
                    inferred = .incoming
                } else {
                    inferred = .outgoing
                }
            } else {
                inferred = .outgoing
            }
        }

        directionByCallId[call.callId] = inferred
        return inferred
    }

    private func deterministicUUID(for callId: String) -> UUID {
        if let parsed = UUID(uuidString: callId) {
            return parsed
        }

        let digest = SHA256.hash(data: Data(callId.utf8))
        var bytes = Array(digest.prefix(16))
        bytes[6] = (bytes[6] & 0x0F) | 0x50
        bytes[8] = (bytes[8] & 0x3F) | 0x80
        return UUID(uuid: (
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15]
        ))
    }

    private func callEndedReason(from reason: String, previousStatus: CallStatus?) -> CXCallEndedReason {
        switch reason {
        case "runtime_error", "auth_failed", "publish_failed", "serialize_failed", "unsupported_group":
            return .failed
        case "busy":
            return .unanswered
        case "declined":
            return .declinedElsewhere
        default:
            if isRinging(previousStatus) {
                return .unanswered
            }
            return .remoteEnded
        }
    }

    private func normalizedHandleValue(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Unknown" : String(trimmed.prefix(64))
    }

    private func onMain(_ work: @escaping () -> Void) {
        if Thread.isMainThread {
            work()
        } else {
            DispatchQueue.main.async(execute: work)
        }
    }

    private func logTransactionError(kind: String, error: Error) {
        let ns = error as NSError
        self.error(
            "requestTransaction failed kind=\(kind) domain=\(ns.domain) code=\(ns.code) desc=\(ns.localizedDescription)"
        )
    }

    private func logReportIncomingError(callId: String, error: Error) {
        let ns = error as NSError
        self.error(
            "reportNewIncomingCall failed callId=\(callId) domain=\(ns.domain) code=\(ns.code) desc=\(ns.localizedDescription)"
        )
    }

    private func info(_ message: String) {
        logger.log("\(message, privacy: .public)")
        NSLog("[CallKit] %@", message)
        writeConsoleLine("[CallKit] \(message)")
    }

    private func error(_ message: String) {
        logger.error("\(message, privacy: .public)")
        NSLog("[CallKit][ERROR] %@", message)
        writeConsoleLine("[CallKit][ERROR] \(message)")
    }

    private func writeConsoleLine(_ line: String) {
        guard let data = "\(line)\n".data(using: .utf8) else { return }
        FileHandle.standardError.write(data)
    }

    private func logObservedCalls(context: String) {
        let calls = callObserver.calls
        if calls.isEmpty {
            info("observer \(context) calls=0")
            return
        }
        for call in calls {
            info(
                "observer \(context) uuid=\(call.uuid.uuidString) outgoing=\(call.isOutgoing) connected=\(call.hasConnected) ended=\(call.hasEnded) onHold=\(call.isOnHold)"
            )
        }
    }

    private func isConnecting(_ status: CallStatus?) -> Bool {
        if case .connecting = status {
            return true
        }
        return false
    }

    private func isActive(_ status: CallStatus?) -> Bool {
        if case .active = status {
            return true
        }
        return false
    }

    private func isRinging(_ status: CallStatus?) -> Bool {
        if case .ringing = status {
            return true
        }
        return false
    }

    private func isEnded(_ status: CallStatus?) -> Bool {
        if case .ended = status {
            return true
        }
        return false
    }
}

extension CallKitCoordinator: CXProviderDelegate {
    func providerDidReset(_ provider: CXProvider) {
        error("providerDidReset")
        onMain { [weak self] in
            self?.pendingOutgoingByUUID.removeAll()
            self?.callIdToUUID.removeAll()
            self?.uuidToCallId.removeAll()
            self?.callMetaByCallId.removeAll()
            self?.directionByCallId.removeAll()
        }
    }

    func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        onMain { [weak self] in
            guard let self else {
                action.fail()
                return
            }

            guard var pending = self.pendingOutgoingByUUID[action.callUUID] else {
                self.error("provider perform start failed: unknown uuid=\(action.callUUID.uuidString)")
                action.fail()
                return
            }

            pending.startActionPerformed = true
            self.pendingOutgoingByUUID[action.callUUID] = pending
            self.info("provider perform start uuid=\(action.callUUID.uuidString) chatId=\(pending.chatId)")
            provider.reportOutgoingCall(with: action.callUUID, startedConnectingAt: Date())
            self.actions.startCall(pending.chatId)
            action.fulfill(withDateStarted: Date())
            self.logObservedCalls(context: "after-start-fulfill")
        }
    }

    func provider(_ provider: CXProvider, perform action: CXAnswerCallAction) {
        onMain { [weak self] in
            guard let self else {
                action.fail()
                return
            }

            guard let callId = self.uuidToCallId[action.callUUID],
                  let meta = self.callMetaByCallId[callId] else {
                self.error("provider perform answer failed: unknown uuid=\(action.callUUID.uuidString)")
                action.fail()
                return
            }

            self.info("provider perform answer uuid=\(action.callUUID.uuidString) callId=\(callId)")
            self.actions.acceptCall(meta.chatId)
            action.fulfill()
        }
    }

    func provider(_ provider: CXProvider, perform action: CXEndCallAction) {
        onMain { [weak self] in
            guard let self else {
                action.fail()
                return
            }

            if let callId = self.uuidToCallId[action.callUUID],
               let meta = self.callMetaByCallId[callId] {
                self.info("provider perform end uuid=\(action.callUUID.uuidString) callId=\(callId)")
                if self.isRinging(meta.status) {
                    self.actions.rejectCall(meta.chatId)
                } else {
                    self.actions.endCall()
                }
                action.fulfill()
                self.logObservedCalls(context: "after-end-fulfill")
                return
            }

            if let pending = self.pendingOutgoingByUUID.removeValue(forKey: action.callUUID) {
                self.info("provider perform end pendingOutgoing uuid=\(action.callUUID.uuidString)")
                if pending.startActionPerformed {
                    self.actions.endCall()
                }
                action.fulfill()
                self.logObservedCalls(context: "after-end-pending-fulfill")
                return
            }

            self.actions.endCall()
            action.fulfill()
            self.logObservedCalls(context: "after-end-fallback-fulfill")
        }
    }
}

extension CallKitCoordinator: CXCallObserverDelegate {
    func callObserver(_ callObserver: CXCallObserver, callChanged call: CXCall) {
        info(
            "observer changed uuid=\(call.uuid.uuidString) outgoing=\(call.isOutgoing) connected=\(call.hasConnected) ended=\(call.hasEnded) onHold=\(call.isOnHold)"
        )
    }
}
