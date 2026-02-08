import SwiftUI

struct LoginView: View {
    let manager: AppManager
    @State private var nsecInput = ""
    @State private var isLoading = false

    var body: some View {
        VStack(spacing: 16) {
            Text("Pika")
                .font(.largeTitle.weight(.semibold))

            Button {
                isLoading = true
                manager.dispatch(.createAccount)
            } label: {
                if isLoading {
                    ProgressView()
                        .tint(.white)
                } else {
                    Text("Create Account")
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(isLoading)
            .accessibilityIdentifier(TestIds.loginCreateAccount)

            Divider().padding(.vertical, 8)

            TextField("nsec (mock)", text: $nsecInput)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .textFieldStyle(.roundedBorder)
                .disabled(isLoading)
                .accessibilityIdentifier(TestIds.loginNsecInput)

            Button {
                isLoading = true
                manager.login(nsec: nsecInput)
            } label: {
                if isLoading {
                    ProgressView()
                } else {
                    Text("Login")
                }
            }
            .buttonStyle(.bordered)
            .disabled(isLoading)
            .accessibilityIdentifier(TestIds.loginSubmit)
        }
        .padding(20)
        // Reset loading on error (errors produce toasts).
        .onChange(of: manager.state.toast) { _, new in
            if new != nil { isLoading = false }
        }
    }
}
