import SwiftUI

/// Nimmt `micled://`-Adressen entgegen (Raycast, Stream Deck, Kurzbefehle).
final class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls { AppModel.shared.handle(url: url) }
    }

    /// Kern sauber anhalten: eigener Slack-Status wird noch zurückgesetzt
    func applicationWillTerminate(_ notification: Notification) {
        MainActor.assumeIsolated { AppModel.shared.shutdown() }
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
            StatusLabel()
                .environment(model)
        }
        .menuBarExtraStyle(.window)

        Window("Statistik", id: "stats") {
            StatsView()
                .environment(model)
                .screenshotActive()
        }
        .windowResizability(.contentSize)

        // Nur im Screenshot-Modus geöffnet: das Menü als normales Fenster
        Window("MicLED-Menü", id: "menu") {
            MenuView()
                .environment(model)
                .screenshotActive()
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)

        Settings {
            SettingsView()
                .environment(model)
                .screenshotActive()
        }
    }
}
