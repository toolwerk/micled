import AppKit
import Sparkle

/// Sparkle für die App, die mit Developer ID von GitHub kommt. Lebt in Swift,
/// nicht im Rust-Kern, weil Sparkle ein AppKit-Framework mit eigenen Fenstern ist.
///
/// Sparkle prüft beim Start und danach alle 24 Stunden (`SUScheduledCheckInterval`)
/// den Appcast am neuesten Release und fragt vor der Installation nach.
/// Debug-Builds und Builds mit dem Platzhalter-Schlüssel bleiben still: Ein
/// Entwickler-Build darf sich nie durch ein Release ersetzen.
@Observable
@MainActor
final class UpdaterController {
    static let shared = UpdaterController()

    private let controller: SPUStandardUpdaterController?

    var automaticallyChecksForUpdates: Bool {
        didSet {
            guard let updater = controller?.updater,
                  updater.automaticallyChecksForUpdates != automaticallyChecksForUpdates else { return }
            updater.automaticallyChecksForUpdates = automaticallyChecksForUpdates
        }
    }

    private init(bundle: Bundle = .main) {
        controller = Self.canStart(in: bundle)
            ? SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
            : nil
        automaticallyChecksForUpdates = controller?.updater.automaticallyChecksForUpdates ?? ScreenshotMode.isActive
    }

    /// Im Screenshot-Modus wie in der echten App, damit die Bilder nichts Ausgegrautes zeigen
    var isAvailable: Bool { controller != nil || ScreenshotMode.isActive }

    /// „0.2.0 (7)“
    var versionText: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(version) (\(build))"
    }

    func checkForUpdates() {
        guard let controller else { return }
        NSApp.activate()   // MicLED hat kein Dock-Symbol, sonst bleibt das Fenster hinten
        controller.checkForUpdates(nil)
    }

    private static func canStart(in bundle: Bundle) -> Bool {
        #if DEBUG
        return false
        #else
        guard bundle.bundlePath.hasSuffix(".app"),
              bundle.object(forInfoDictionaryKey: "SUFeedURL") != nil,
              let key = bundle.object(forInfoDictionaryKey: "SUPublicEDKey") as? String,
              !key.isEmpty, !key.hasPrefix("__") else { return false }
        return true
        #endif
    }
}
