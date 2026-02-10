import SwiftUI

struct ChatView: View {
    let manager: AppManager
    let chatId: String
    @State private var messageText = ""

    var body: some View {
        if let chat = manager.state.currentChat, chat.chatId == chatId {
            VStack(spacing: 0) {
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
                }

                Divider()

                HStack(spacing: 10) {
                    TextField("Message", text: $messageText, axis: .vertical)
                        .lineLimit(1...4)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier(TestIds.chatMessageInput)

                    Button("Send") {
                        manager.dispatch(.sendMessage(chatId: chat.chatId, content: messageText))
                        messageText = ""
                    }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier(TestIds.chatSend)
                }
                .padding(12)
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
