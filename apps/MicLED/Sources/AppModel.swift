import AppKit
import Foundation
import Observation
import ServiceManagement

/// Bindeglied zwischen Oberfläche und Rust-Kern. Hält die Einstellungen
/// (UserDefaults) und den zuletzt gemeldeten Status.
@MainActor
@Observable
final class AppModel {
    // MARK: Einstellungen

    var enabled: Bool { didSet { persist() } }
    var useWebhook: Bool { didSet { persist() } }
    var luxaforId: String { didSet { persist() } }
    var micOnHex: String { didSet { persist() } }
    var micOffHex: String { didSet { persist() } }
    /// true: nach dem Mikrofon zur vorherigen Farbe, false: immer `micOffHex`
    var restorePrevious: Bool { didSet { persist() } }
    var releaseDelaySeconds: Double { didSet { persist() } }
    var ignored: [String] { didSet { persist() } }
    var favorites: [String] { didSet { persist(pushConfig: false) } }

    // MARK: Status aus dem Kern

    private(set) var micUsers: [MicUser] = []
    private(set) var overriding = false
    private(set) var shownHex: String?
    private(set) var baseHex: String?
    private(set) var error: String?

    var micActive: Bool { !micUsers.isEmpty }

    private var controller: MicLedController?
    private let defaults = UserDefaults.standard

    static let defaultFavorites = ["00ff00", "ffff00", "ff0000", "0000ff", "ffffff"]

    init() {
        let d = UserDefaults.standard
        d.register(defaults: [
            Key.enabled: true,
            Key.useWebhook: false,
            Key.luxaforId: "",
            Key.micOn: "ff0000",
            Key.micOff: "00ff00",
            Key.restorePrevious: true,
            Key.releaseDelay: 3.0,
            Key.ignored: defaultIgnored(),
            Key.favorites: AppModel.defaultFavorites,
        ])
        enabled = d.bool(forKey: Key.enabled)
        useWebhook = d.bool(forKey: Key.useWebhook)
        luxaforId = d.string(forKey: Key.luxaforId) ?? ""
        micOnHex = d.string(forKey: Key.micOn) ?? "ff0000"
        micOffHex = d.string(forKey: Key.micOff) ?? "00ff00"
        restorePrevious = d.bool(forKey: Key.restorePrevious)
        releaseDelaySeconds = d.double(forKey: Key.releaseDelay)
        ignored = d.stringArray(forKey: Key.ignored) ?? defaultIgnored()
        favorites = d.stringArray(forKey: Key.favorites) ?? AppModel.defaultFavorites

        let observer = Observer { [weak self] status in
            Task { @MainActor in self?.apply(status) }
        }
        let controller = MicLedController(config: config, observer: observer)
        self.controller = controller
        apply(controller.status())
    }

    // MARK: Aktionen

    /// Farbe direkt setzen. Sie gilt sofort und ist danach die Farbe,
    /// zu der nach dem Mikrofon zurückgekehrt wird.
    func setColor(_ hex: String) {
        controller?.setColor(hex: hex)
    }

    func turnOff() {
        controller?.turnOff()
    }

    func ignore(_ user: MicUser) {
        let key = user.bundleId.isEmpty ? user.name : user.bundleId
        if !ignored.contains(key) { ignored.append(key) }
    }

    func currentUsers() -> [MicUser] {
        currentMicUsers()
    }

    /// Anzeigename: App-Name, sonst Prozessname
    func displayName(_ user: MicUser) -> String {
        NSRunningApplication(processIdentifier: user.pid)?.localizedName ?? user.name
    }

    var launchAtLogin: Bool {
        get { SMAppService.mainApp.status == .enabled }
        set {
            do {
                if newValue { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            } catch {
                self.error = "Anmeldeobjekt: \(error.localizedDescription)"
            }
        }
    }

    // MARK: Intern

    private var config: MicLedConfig {
        MicLedConfig(
            enabled: enabled,
            transport: useWebhook ? .webhook : .usb,
            luxaforId: luxaforId,
            micOnColor: micOnHex,
            micOffColor: micOffHex,
            afterMic: restorePrevious ? .restore : .fixed,
            releaseDelayMs: UInt32(max(0, releaseDelaySeconds) * 1000),
            ignored: ignored
        )
    }

    private func persist(pushConfig: Bool = true) {
        defaults.set(enabled, forKey: Key.enabled)
        defaults.set(useWebhook, forKey: Key.useWebhook)
        defaults.set(luxaforId, forKey: Key.luxaforId)
        defaults.set(micOnHex, forKey: Key.micOn)
        defaults.set(micOffHex, forKey: Key.micOff)
        defaults.set(restorePrevious, forKey: Key.restorePrevious)
        defaults.set(releaseDelaySeconds, forKey: Key.releaseDelay)
        defaults.set(ignored, forKey: Key.ignored)
        defaults.set(favorites, forKey: Key.favorites)
        if pushConfig { controller?.updateConfig(config: config) }
    }

    private func apply(_ s: MicLedStatus) {
        micUsers = s.micUsers
        overriding = s.overriding
        shownHex = s.shownColor
        baseHex = s.baseColor
        error = s.error
    }

    private enum Key {
        static let enabled = "enabled"
        static let useWebhook = "useWebhook"
        static let luxaforId = "luxaforId"
        static let micOn = "micOnColor"
        static let micOff = "micOffColor"
        static let restorePrevious = "restorePrevious"
        static let releaseDelay = "releaseDelaySeconds"
        static let ignored = "ignored"
        static let favorites = "favorites"
    }
}

/// Nimmt Statusmeldungen aus dem Rust-Thread entgegen.
final class Observer: StatusObserver {
    private let handler: @Sendable (MicLedStatus) -> Void

    init(_ handler: @escaping @Sendable (MicLedStatus) -> Void) {
        self.handler = handler
    }

    func statusChanged(status: MicLedStatus) {
        handler(status)
    }
}
