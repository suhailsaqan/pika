import SwiftUI

struct ChatView: View {
    let manager: AppManager
    let chatId: String
    @State private var messageText = ""

    var body: some View {
        if let chat = manager.state.currentChat, chat.chatId == chatId {
            VStack(spacing: 8) {
                callControls(chat: chat)

                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 8) {
                            ForEach(chat.messages, id: \.id) { msg in
                                MessageRow(message: msg)
                            }
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 10)
                    }
                    .modifier(FloatingInputBarModifier(content: { messageInputBar(chat: chat) }))
                }
            }
            .navigationTitle(chat.peerName ?? chat.peerNpub)
            .navigationBarTitleDisplayMode(.inline)
        } else {
            VStack(spacing: 10) {
                ProgressView()
                Text("Loading chat…")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func sendMessage(chat: ChatViewState) {
        let trimmed = messageText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        manager.dispatch(.sendMessage(chatId: chat.chatId, content: trimmed))
        messageText = ""
    }

    private func isLiveStatus(_ status: CallStatus) -> Bool {
        switch status {
        case .offering, .ringing, .connecting, .active:
            return true
        case .ended:
            return false
        }
    }

    @ViewBuilder
    private func callControls(chat: ChatViewState) -> some View {
        let activeCall = manager.state.activeCall
        let callForChat = activeCall?.chatId == chat.chatId ? activeCall : nil
        let hasLiveCallElsewhere = activeCall.map { $0.chatId != chat.chatId && isLiveStatus($0.status) } ?? false

        if let call = callForChat {
            VStack(alignment: .leading, spacing: 8) {
                Text(callStatusText(call.status))
                    .font(.subheadline.weight(.semibold))

                if let debug = call.debug {
                    Text("tx \(debug.txFrames)  rx \(debug.rxFrames)  drop \(debug.rxDropped)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                HStack(spacing: 8) {
                    switch call.status {
                    case .ringing:
                        Button("Accept") {
                            manager.dispatch(.acceptCall(chatId: chat.chatId))
                        }
                        .accessibilityIdentifier(TestIds.chatCallAccept)
                        Button("Reject", role: .destructive) {
                            manager.dispatch(.rejectCall(chatId: chat.chatId))
                        }
                        .accessibilityIdentifier(TestIds.chatCallReject)
                    case .offering, .connecting, .active:
                        Button(call.isMuted ? "Unmute" : "Mute") {
                            manager.dispatch(.toggleMute)
                        }
                        .accessibilityIdentifier(TestIds.chatCallMute)
                        Button("End", role: .destructive) {
                            manager.dispatch(.endCall)
                        }
                        .accessibilityIdentifier(TestIds.chatCallEnd)
                    case .ended:
                        Button("Start Again") {
                            manager.dispatch(.startCall(chatId: chat.chatId))
                        }
                        .accessibilityIdentifier(TestIds.chatCallStart)
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
        } else {
            HStack {
                Button("Start Call") {
                    manager.dispatch(.startCall(chatId: chat.chatId))
                }
                .disabled(hasLiveCallElsewhere)
                .accessibilityIdentifier(TestIds.chatCallStart)
                if hasLiveCallElsewhere {
                    Text("Another call is active")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
        }
    }

    private func callStatusText(_ status: CallStatus) -> String {
        switch status {
        case .offering:
            return "Calling…"
        case .ringing:
            return "Incoming call"
        case .connecting:
            return "Connecting…"
        case .active:
            return "Call active"
        case let .ended(reason):
            return "Call ended: \(reason)"
        }
    }

    @ViewBuilder
    private func messageInputBar(chat: ChatViewState) -> some View {
        HStack(spacing: 10) {
            TextField("Message", text: $messageText)
                .onSubmit { sendMessage(chat: chat) }
                .accessibilityIdentifier(TestIds.chatMessageInput)

            Button(action: { sendMessage(chat: chat) }) {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.title2)
            }
            .disabled(messageText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .accessibilityIdentifier(TestIds.chatSend)
        }
        .modifier(GlassInputModifier())
    }
}

private struct GlassInputModifier: ViewModifier {
    func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .glassEffect(.regular.interactive(), in: .capsule)
                .padding(12)
        } else {
            content
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(.ultraThinMaterial, in: Capsule())
                .padding(12)
        }
    }
}

private struct FloatingInputBarModifier<Bar: View>: ViewModifier {
    @ViewBuilder var content: Bar

    func body(content view: Content) -> some View {
        if #available(iOS 26.0, *) {
            view.safeAreaBar(edge: .bottom) { content }
        } else {
            view.safeAreaInset(edge: .bottom) {
                VStack(spacing: 0) {
                    Divider()
                    content
                }
            }
        }
    }
}

private struct MessageRow: View {
    let message: ChatMessage

    var body: some View {
        HStack {
            if message.isMine { Spacer(minLength: 0) }
            VStack(alignment: message.isMine ? .trailing : .leading, spacing: 3) {
                Text(message.content)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(message.isMine ? Color.blue : Color.gray.opacity(0.2))
                    .foregroundStyle(message.isMine ? Color.white : Color.primary)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .contextMenu {
                        Button {
                            UIPasteboard.general.string = message.content
                        } label: {
                            Label("Copy", systemImage: "doc.on.doc")
                        }
                    }

                if message.isMine {
                    Text(deliveryText(message.delivery))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            if !message.isMine { Spacer(minLength: 0) }
        }
    }

    private func deliveryText(_ d: MessageDeliveryState) -> String {
        switch d {
        case .pending: return "Pending"
        case .sent: return "Sent"
        case .failed(let reason): return "Failed: \(reason)"
        }
    }
}
