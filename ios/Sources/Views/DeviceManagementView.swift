import SwiftUI

struct DeviceManagementView: View {
    let devices: [DeviceInfo]
    let pendingDevices: [DeviceInfo]
    let autoAddDevices: Bool
    let onToggleAutoAdd: (Bool) -> Void
    let onAddDevice: (String) -> Void
    let onAcceptDevice: (String) -> Void
    let onRejectDevice: (String) -> Void
    let onAcceptAll: () -> Void
    let onRejectAll: () -> Void
    let onRefresh: () -> Void

    var body: some View {
        List {
            Section {
                Toggle("Auto-add new devices", isOn: Binding(
                    get: { autoAddDevices },
                    set: { onToggleAutoAdd($0) }
                ))
            } footer: {
                Text("When enabled, existing devices will automatically detect and invite new devices to all groups.")
            }

            if !pendingDevices.isEmpty {
                Section {
                    HStack {
                        Text("Pending Devices")
                            .font(.headline)
                        Spacer()
                        Button("Accept All") { onAcceptAll() }
                            .font(.caption)
                            .buttonStyle(.borderedProminent)
                        Button("Reject All") { onRejectAll() }
                            .font(.caption)
                            .buttonStyle(.bordered)
                            .tint(.red)
                    }

                    ForEach(pendingDevices, id: \.fingerprint) { device in
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(deviceDisplayName(device))
                                    .font(.body)
                                Text("Published: \(formattedDate(device.publishedAt))")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button { onAcceptDevice(device.fingerprint) } label: {
                                Image(systemName: "checkmark.circle.fill")
                                    .foregroundStyle(.green)
                            }
                            .buttonStyle(.plain)
                            Button { onRejectDevice(device.fingerprint) } label: {
                                Image(systemName: "xmark.circle.fill")
                                    .foregroundStyle(.red)
                            }
                            .buttonStyle(.plain)
                        }
                        .padding(.vertical, 4)
                    }
                }
            }

            Section("My Devices") {
                if devices.isEmpty {
                    Text("No devices found")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(devices, id: \.fingerprint) { device in
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text(deviceDisplayName(device))
                                        .font(.body)
                                    if device.isCurrentDevice {
                                        Text("This device")
                                            .font(.caption)
                                            .foregroundStyle(.white)
                                            .padding(.horizontal, 6)
                                            .padding(.vertical, 2)
                                            .background(.blue, in: Capsule())
                                    }
                                }
                                Text("Published: \(formattedDate(device.publishedAt))")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                        }
                        .padding(.vertical, 4)
                    }
                }
            }
        }
        .navigationTitle("Devices")
        .onAppear { onRefresh() }
    }

    private func deviceDisplayName(_ device: DeviceInfo) -> String {
        return "Device \(device.fingerprint)"
    }

    private func formattedDate(_ timestamp: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }
}
