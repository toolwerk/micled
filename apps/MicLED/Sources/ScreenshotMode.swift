import AppKit
import SwiftUI

/// Screenshot-Modus für die README (`scripts/screenshots.sh`): Beispieldaten
/// statt echter, kein Rust-Kern, also keine Lampe, kein Slack, kein Nextcloud.
/// Nur in Debug-Builds; im Release ist der Modus immer aus.
enum ScreenshotMode {
    static var isActive: Bool {
        #if DEBUG
        CommandLine.arguments.contains("--screenshots")
        #else
        false
        #endif
    }

    private static func argument(_ name: String) -> String? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    /// `--appearance light|dark`
    static var appearance: NSAppearance? {
        argument("--appearance").map { NSAppearance(named: $0 == "dark" ? .darkAqua : .aqua)! }
    }

    /// `--window menu|stats|settings`: nur dieses Fenster öffnen, damit es aktiv
    /// (und nicht ausgegraut) aufgenommen wird
    static var window: String { argument("--window") ?? "menu" }

    /// `--scroll network|strip`: Einstellungen bis zu Govee und Nanoleaf bzw. zum Lichtstreifen scrollen
    static var scroll: String? { isActive ? argument("--scroll") : nil }

    static let suiteName = "net.staude.micled.screenshots"

    /// Eigene, beim Start geleerte Einstellungen
    static func defaults() -> UserDefaults {
        UserDefaults.standard.removePersistentDomain(forName: suiteName)
        let d = UserDefaults(suiteName: suiteName) ?? .standard
        d.set(["ff0000", "00ff00", "ffff00", "0000ff", "ffffff"], forKey: "favorites")
        d.set([["key": "com.plaud.mac", "hex": "ff8800"]], forKey: "appColors")
        d.set(true, forKey: "slackEnabled")
        d.set(true, forKey: "dimEnabled")
        d.set(true, forKey: "remindersEnabled")
        d.set(true, forKey: "pixooEnabled")
        d.set("192.168.1.50", forKey: "pixooHost")
        d.set(true, forKey: "awtrixEnabled")
        d.set("192.168.1.60", forKey: "awtrixHost")
        d.set(true, forKey: "hueEnabled")
        d.set("192.168.1.20", forKey: "hueHost")
        d.set("screenshot", forKey: "hueFingerprint")
        d.set("room", forKey: "hueTargetKind")
        d.set("flur", forKey: "hueTargetId")
        d.set(true, forKey: "goveeEnabled")
        d.set([
            ["id": UUID().uuidString, "host": "192.168.1.31", "name": "H6609"],
            ["id": UUID().uuidString, "host": "192.168.1.32", "name": ""],
        ], forKey: "goveeDevices")
        d.set(true, forKey: "nanoleafEnabled")
        d.set([["id": UUID().uuidString, "host": "192.168.1.40:16021", "name": "Canvas 5B3C"]], forKey: "nanoleafDevices")
        d.set(true, forKey: "robobloqEnabled")
        return d
    }

    /// Beispiel-Räume und -Lampen für die Hue-Auswahl
    static let hueChoices = [
        HueTarget(kind: .room, id: "buero", name: String(localized: "Büro")),
        HueTarget(kind: .room, id: "flur", name: String(localized: "Flur")),
        HueTarget(kind: .light, id: "tuer", name: String(localized: "Flur – Tür")),
        HueTarget(kind: .light, id: "steh", name: String(localized: "Stehlampe")),
    ]

    /// Beispiel-Ergebnis der Govee-Suche (eines schon eingetragen)
    static let goveeFound = [
        FoundDevice(host: "192.168.1.31", name: "H6609"),
        FoundDevice(host: "192.168.1.33", name: "H6008"),
    ]

    /// Feste Zeit auf Sendung: Wochenende frei, werktags 40–220 min
    static func seconds(forDay start: Date) -> UInt64 {
        if Calendar.current.isDateInWeekend(start) { return 0 }
        let day = Calendar.current.ordinality(of: .day, in: .era, for: start) ?? 0
        let minutes = [95, 140, 60, 220, 130, 40, 175, 110][day % 8]
        return UInt64(minutes * 60)
    }

    /// Das gewählte Fenster öffnen (aus dem Symbol heraus, das die Aktionen kennt)
    @MainActor
    static func openWindows(openWindow: OpenWindowAction, openSettings: OpenSettingsAction) {
        guard isActive else { return }
        if let appearance { NSApp.appearance = appearance }
        NSApp.activate()
        switch window {
        case "stats": openWindow(id: "stats")
        case "settings": openSettings()
        default: openWindow(id: "menu")
        }
        // aktiv werden, auch wenn das Terminal vorne ist, damit Schalter nicht grau erscheinen
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            NSApp.activate(ignoringOtherApps: true)
            NSApp.windows.first { $0.isVisible && $0.canBecomeKey }?.makeKeyAndOrderFront(nil)
        }
    }
}

extension View {
    /// Im Screenshot-Modus wie ein aktives Fenster zeichnen; macOS lässt die
    /// aus dem Terminal gestartete App nicht nach vorn, Schalter wären sonst grau.
    @ViewBuilder
    func screenshotActive() -> some View {
        if ScreenshotMode.isActive {
            environment(\.controlActiveState, .key)
        } else {
            self
        }
    }
}
