import SwiftUI

@main
struct PikaApp: App {
    @State private var manager = AppManager()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView(manager: manager)
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active {
                        manager.onForeground()
                    }
                }
                .onOpenURL { url in
                    NSLog("[PikaApp] onOpenURL: \(url.absoluteString)")
                    manager.onOpenURL(url)
                }
        }
    }
}
