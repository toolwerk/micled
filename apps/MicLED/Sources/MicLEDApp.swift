import SwiftUI

/// Nimmt `micled://`-Adressen entgegen (Raycast, Stream Deck, Kurzbefehle).
final class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls { AppModel.shared.handle(url: url) }
    }
}

@main
struct MicLEDApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model = AppModel.shared

    var body: some Scene {
        MenuBarExtra {
            MenuView()
                .environment(model)
        } label: {
            Image(nsImage: StatusIcon.image(hex: model.shownHex, micActive: model.micActive, cameraOn: model.cameraOn))
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView()
                .environment(model)
        }
    }
}
