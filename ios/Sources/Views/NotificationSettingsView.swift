import SwiftUI
import UserNotifications

struct NotificationSettingsView: View {
    @State private var permissionStatus: UNAuthorizationStatus?
    @State private var didReregister = false
    var body: some View {
        List {
            permissionSection
            serverSection
            deviceInfoSection
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Notifications")
        .task {
            await refreshPermissionStatus()
        }
    }

    @ViewBuilder
    private var permissionSection: some View {
        Section {
            HStack {
                Text("Permission")
                Spacer()
                Text(permissionLabel)
                    .foregroundStyle(permissionColor)
            }

            if permissionStatus == .denied {
                Button("Open Settings") {
                    if let url = URL(string: UIApplication.openSettingsURLString) {
                        UIApplication.shared.open(url)
                    }
                }
            }
        } header: {
            Text("Push Notifications")
        } footer: {
            if permissionStatus == .denied {
                Text("Notifications are disabled. Tap \"Open Settings\" to enable them.")
            }
        }
    }

    @ViewBuilder
    private var serverSection: some View {
        Section {
            HStack {
                Text("Server URL")
                Spacer()
                Text(notificationUrl)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }

            Button {
                PushNotificationManager.shared.reregister()
                didReregister = true
            } label: {
                HStack {
                    Text("Re-register")
                    Spacer()
                    if didReregister {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                    }
                }
            }
            .disabled(didReregister)
        } header: {
            Text("Notification Server")
        } footer: {
            Text("Re-register the device and re-subscribe to all your chats.")
        }
    }

    @ViewBuilder
    private var deviceInfoSection: some View {
        Section {
            HStack {
                Text("APNs Token")
                Spacer()
                Text(PushNotificationManager.shared.apnsToken ?? "Not registered")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        } header: {
            Text("Debug Info")
        }
    }

    private var permissionLabel: String {
        switch permissionStatus {
        case .authorized: return "Enabled"
        case .denied: return "Disabled"
        case .provisional: return "Provisional"
        case .ephemeral: return "Ephemeral"
        case .notDetermined, .none: return "Not Requested"
        @unknown default: return "Unknown"
        }
    }

    private var permissionColor: Color {
        switch permissionStatus {
        case .authorized, .provisional, .ephemeral: return .green
        case .denied: return .red
        case .notDetermined, .none: return .secondary
        @unknown default: return .secondary
        }
    }

    private func refreshPermissionStatus() async {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        permissionStatus = settings.authorizationStatus
    }

    private var notificationUrl: String {
        let appGroup = Bundle.main.infoDictionary?["PikaAppGroup"] as? String ?? "group.org.pikachat.pika"
        if let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup) {
            let configUrl = container.appendingPathComponent("Library/Application Support/pika_config.json")
            if let data = try? Data(contentsOf: configUrl),
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let url = json["notification_url"] as? String, !url.isEmpty {
                return url
            }
        }
        if let envUrl = ProcessInfo.processInfo.environment["PIKA_NOTIFICATION_URL"], !envUrl.isEmpty {
            return envUrl
        }
        return "https://test.notifs.benthecarman.com"
    }
}

#if DEBUG
#Preview {
    NavigationStack {
        NotificationSettingsView()
    }
}
#endif
