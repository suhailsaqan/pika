import SwiftUI

struct ChatView: View {
    let chatId: String
    let state: ChatScreenState
    let onSendMessage: @MainActor (String) -> Void
    @State private var messageText = ""
    @State private var scrollPosition: String?

    private static let bottomAnchorId = "chat-bottom-anchor"
    private let scrollButtonBottomPadding: CGFloat = 12

    var body: some View {
        if let chat = state.chat, chat.chatId == chatId {
            ScrollViewReader { proxy in
                let scrollView = ScrollView {
                    VStack(spacing: 0) {
                        LazyVStack(spacing: 8) {
                            ForEach(chat.messages, id: \.id) { msg in
                                MessageRow(message: msg)
                                    .id(msg.id)
                            }
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 10)

                        BottomAnchor()
                            .id(Self.bottomAnchorId)
                    }
                    .scrollTargetLayout()
                }
                .scrollTargetBehavior(.viewAligned)
                .overlay(alignment: .bottomTrailing) {
                    if showScrollButton(for: chat) {
                        Button {
                            scrollToBottom(proxy, animated: true, chat: chat)
                        } label: {
                            Image(systemName: "arrow.down")
                                .font(.footnote.weight(.semibold))
                                .padding(10)
                        }
                        .foregroundStyle(.primary)
                        .background(.ultraThinMaterial, in: Circle())
                        .overlay(Circle().strokeBorder(.quaternary, lineWidth: 0.5))
                        .padding(.trailing, 16)
                        .padding(.bottom, scrollButtonBottomPadding)
                        .accessibilityLabel("Scroll to bottom")
                    }
                }
                .modifier(FloatingInputBarModifier(content: { messageInputBar(chat: chat) }))

                if #available(iOS 18.0, *) {
                    scrollView
                        .scrollPosition(id: $scrollPosition, anchor: .bottom)
                        .defaultScrollAnchor(.bottom, for: .initialOffset)
                } else {
                    scrollView
                        .scrollPosition(id: $scrollPosition)
                        .defaultScrollAnchor(.bottom)
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

    private func sendMessage() {
        let trimmed = messageText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        onSendMessage(trimmed)
        messageText = ""
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy, animated: Bool, chat: ChatViewState) {
        if #available(iOS 18.0, *) {
            scrollPosition = Self.bottomAnchorId
        } else {
            scrollPosition = chat.messages.last?.id ?? Self.bottomAnchorId
        }
        let action = {
            proxy.scrollTo(Self.bottomAnchorId, anchor: .bottom)
        }
        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                action()
            }
        } else {
            action()
        }
    }

    private func showScrollButton(for chat: ChatViewState) -> Bool {
        guard let position = scrollPosition else { return false }
        if #available(iOS 18.0, *) {
            return position != Self.bottomAnchorId
        }
        guard let lastId = chat.messages.last?.id else { return false }
        return position != lastId
    }


    @ViewBuilder
    private func messageInputBar(chat: ChatViewState) -> some View {
        HStack(spacing: 10) {
            TextField("Message", text: $messageText)
                .onSubmit { sendMessage() }
                .accessibilityIdentifier(TestIds.chatMessageInput)

            Button(action: { sendMessage() }) {
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
        content
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.ultraThinMaterial, in: Capsule())
            .padding(12)
    }
}

private struct FloatingInputBarModifier<Bar: View>: ViewModifier {
    @ViewBuilder var content: Bar

    func body(content view: Content) -> some View {
        view.safeAreaInset(edge: .bottom) {
            VStack(spacing: 0) {
                Divider()
                content
            }
        }
    }
}

private struct BottomAnchor: View {
    var body: some View {
        Color.clear.frame(height: 1)
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

#if DEBUG
#Preview("Chat") {
    NavigationStack {
        ChatView(
            chatId: "chat-1",
            state: ChatScreenState(chat: PreviewAppState.chatDetail.currentChat),
            onSendMessage: { _ in }
        )
    }
}

#Preview("Chat - Failed") {
    NavigationStack {
        ChatView(
            chatId: "chat-1",
            state: ChatScreenState(chat: PreviewAppState.chatDetailFailed.currentChat),
            onSendMessage: { _ in }
        )
    }
}

#Preview("Chat - Empty") {
    NavigationStack {
        ChatView(
            chatId: "chat-empty",
            state: ChatScreenState(chat: PreviewAppState.chatDetailEmpty.currentChat),
            onSendMessage: { _ in }
        )
    }
}

#Preview("Chat - Long Thread") {
    NavigationStack {
        ChatView(
            chatId: "chat-long",
            state: ChatScreenState(chat: PreviewAppState.chatDetailLongThread.currentChat),
            onSendMessage: { _ in }
        )
    }
}
#endif
