import AppKit
import SwiftUI

/// Kontextmenü für das Menüleisten-Symbol: Rechtsklick oder Ctrl-Klick zeigt
/// „Einstellungen …“ und „MicLED beenden“, ein Linksklick öffnet weiter das
/// Menüfenster von `MenuBarExtra`.
///
/// `MenuBarExtra` hat keinen eigenen Rechtsklick. Deshalb fängt ein lokaler
/// Ereignis-Monitor Klicks auf das Fenster des Symbols ab, bevor SwiftUI sie sieht.
@MainActor
final class StatusContextMenu: NSObject {
    static let shared = StatusContextMenu()

    /// Kommt aus der SwiftUI-Umgebung des Symbols (`openSettings`)
    var openSettings: (() -> Void)?
    private var monitor: Any?

    func install() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.rightMouseDown, .leftMouseDown]) { event in
            nonisolated(unsafe) let e = event
            let handled = MainActor.assumeIsolated { StatusContextMenu.shared.handle(e) }
            return handled ? nil : event
        }
    }

    /// `true`, wenn das Ereignis das Kontextmenü geöffnet hat
    private func handle(_ event: NSEvent) -> Bool {
        let secondary = event.type == .rightMouseDown
            || (event.type == .leftMouseDown && event.modifierFlags.contains(.control))
        guard secondary, let window = event.window, Self.isStatusItemWindow(window),
              let view = window.contentView else { return false }
        let menu = NSMenu()
        menu.addItem(withTitle: "Einstellungen …", action: #selector(settings), keyEquivalent: ",").target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: "MicLED beenden", action: #selector(quit), keyEquivalent: "q").target = self
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: view.bounds.minY - 4), in: view)
        return true
    }

    private static func isStatusItemWindow(_ window: NSWindow) -> Bool {
        NSStringFromClass(type(of: window)).contains("StatusBarWindow")
    }

    @objc private func settings() {
        NSApp.activate()
        openSettings?()
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}

/// Symbol in der Menüleiste; reicht `openSettings` an das Kontextmenü weiter
struct StatusLabel: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        Image(nsImage: StatusIcon.image(hex: model.shownHex, micActive: model.micActive, cameraOn: model.cameraOn))
            .onAppear {
                let open = openSettings
                StatusContextMenu.shared.openSettings = { open() }
                StatusContextMenu.shared.install()
            }
    }
}
