import AppKit
import SwiftUI

/// Screenshot-Modus für die README (`scripts/screenshots.sh`): Beispieldaten
/// statt echter, kein Rust-Kern, also keine Lampe, kein Slack, kein Nextcloud, kein MQTT.
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

    /// `--scroll nextcloud|network|wled|strip|mqtt`: Einstellungen bis zu Nextcloud, zu Govee und Nanoleaf, zu WLED, zum Lichtstreifen bzw. zu MQTT scrollen
    static var scroll: String? { isActive ? argument("--scroll") : nil }

    /// `--stats week`: Statistik in der Ansicht „Wochen“ öffnen
    static var stats: String? { isActive ? argument("--stats") : nil }

    static let suiteName = "net.staude.micled.screenshots"

    /// Eigene, beim Start geleerte Einstellungen
    static func defaults() -> UserDefaults {
        UserDefaults.standard.removePersistentDomain(forName: suiteName)
        let d = UserDefaults(suiteName: suiteName) ?? .standard
        d.set(["ff0000", "00ff00", "ffff00", "0000ff", "ffffff"], forKey: "favorites")
        d.set([["key": "com.plaud.mac", "hex": "ff8800"]], forKey: "appColors")
        d.set(true, forKey: "slackEnabled")
        d.set(true, forKey: "nextcloudStatusEnabled")
        d.set("https://cloud.example.org", forKey: "nextcloudServer")
        d.set("anna", forKey: "nextcloudUser")
        d.set(true, forKey: "mqttEnabled")
        d.set("homeassistant.local", forKey: "mqttHost")
        d.set("micled", forKey: "mqttUser")
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
        d.set(["call": true, "timer": false, "focus": false], forKey: "hueStates")
        d.set(true, forKey: "goveeEnabled")
        d.set([
            ["id": UUID().uuidString, "host": "192.168.1.31", "name": "H6609"],
            ["id": UUID().uuidString, "host": "192.168.1.32", "name": ""],
        ], forKey: "goveeDevices")
        d.set(true, forKey: "nanoleafEnabled")
        d.set([["id": UUID().uuidString, "host": "192.168.1.40:16021", "name": "Canvas 5B3C"]], forKey: "nanoleafDevices")
        d.set(true, forKey: "wledEnabled")
        d.set([["id": UUID().uuidString, "host": "192.168.1.45", "name": "Schreibtisch"]], forKey: "wledDevices")
        d.set(true, forKey: "robobloqEnabled")
        return d
    }

    /// Beispiel-Räume und -Lampen für die Hue-Auswahl
    static let hueChoices = [
        HueTarget(kind: .room, id: "buero", name: "Büro"),
        HueTarget(kind: .room, id: "flur", name: "Flur"),
        HueTarget(kind: .light, id: "tuer", name: "Flur – Tür"),
        HueTarget(kind: .light, id: "steh", name: "Stehlampe"),
    ]

    /// Beispiel-Ergebnis der Govee-Suche (eines schon eingetragen)
    static let goveeFound = [
        FoundDevice(host: "192.168.1.31", name: "H6609"),
        FoundDevice(host: "192.168.1.33", name: "H6008"),
    ]

    /// Beispiel-Ergebnis der WLED-Suche (eines schon eingetragen)
    static let wledFound = [
        FoundDevice(host: "192.168.1.45", name: "Schreibtisch"),
        FoundDevice(host: "192.168.1.47", name: "Regal"),
    ]

    /// Feste Zeit auf Sendung: Wochenende frei, werktags 40–220 min
    static func seconds(forDay start: Date) -> UInt64 {
        if Calendar.current.isDateInWeekend(start) { return 0 }
        let day = Calendar.current.ordinality(of: .day, in: .era, for: start) ?? 0
        let minutes = [95, 140, 60, 220, 130, 40, 175, 110][day % 8]
        return UInt64(minutes * 60)
    }

    /// Feste Wochensummen: Auf Sendung 4–8 h, Fokus 5–10 h, laufende Woche bis Mittwoch
    static func weeklyStats(weeks: Int) -> WeeklyStats {
        var cal = Calendar(identifier: .iso8601)
        cal.timeZone = .current
        let monday = cal.dateInterval(of: .weekOfYear, for: Date())?.start ?? Date()
        let live = [6.5, 5.0, 7.2, 4.1, 6.0, 7.8, 5.3, 4.6, 6.9, 5.8, 7.1, 5.1]
        let focus = [8.0, 6.5, 9.1, 5.2, 7.4, 9.6, 6.1, 7.7, 8.3, 7.0, 7.1, 9.1]
        let calls: [UInt32] = [15, 12, 18, 9, 14, 19, 13, 11, 16, 14, 17, 14]
        func week(_ back: Int, _ i: Int, share: Double = 1) -> WeekStats {
            let start = cal.date(byAdding: .weekOfYear, value: -back, to: monday) ?? monday
            return WeekStats(
                start: UInt64(start.timeIntervalSince1970),
                year: Int32(cal.component(.yearForWeekOfYear, from: start)),
                week: UInt32(cal.component(.weekOfYear, from: start)),
                liveSeconds: UInt64(live[i % live.count] * share * 3600),
                calls: UInt32(Double(calls[i % calls.count]) * share),
                focusSeconds: UInt64(focus[i % focus.count] * share * 3600)
            )
        }
        let list = (0..<weeks).map { i in week(weeks - 1 - i, i) }
        let last = weeks - 1
        return WeeklyStats(weeks: list, lastWeekToDate: week(1, last - 1, share: 0.75))
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
