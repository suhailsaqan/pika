import SwiftUI

struct NewChatView: View {
    let manager: AppManager
    @State private var npubInput = ""
    @State private var isLoading = false

    var body: some View {
        let peer = npubInput.trimmingCharacters(in: .whitespacesAndNewlines)
        let isValidPeer = PeerKeyValidator.isValidPeer(peer)

        VStack(spacing: 12) {
            TextField("Peer npub", text: $npubInput)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .textFieldStyle(.roundedBorder)
                .disabled(isLoading)
                .accessibilityIdentifier(TestIds.newChatPeerNpub)

            if !peer.isEmpty && !isValidPeer {
                Text("Enter a valid npub1… or 64-char hex pubkey.")
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            Button {
                isLoading = true
                manager.dispatch(.createChat(peerNpub: peer))
            } label: {
                if isLoading {
                    HStack(spacing: 8) {
                        ProgressView()
                            .tint(.white)
                        Text("Creating…")
                    }
                } else {
                    Text("Start Chat")
                }
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier(TestIds.newChatStart)
            .disabled(!isValidPeer || isLoading)

            Spacer()
        }
        .padding(16)
        .navigationTitle("New Chat")
        // Reset loading on error (errors produce toasts).
        .onChange(of: manager.state.toast) { _, new in
            if new != nil { isLoading = false }
        }
    }
}
