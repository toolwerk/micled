import SwiftUI

@main
struct MicLEDApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            MenuView()
                .environment(model)
        } label: {
            Image(nsImage: StatusIcon.image(hex: model.shownHex, micActive: model.micActive))
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView()
                .environment(model)
        }
    }
}
