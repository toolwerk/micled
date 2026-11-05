import AppKit
import Foundation
import Observation
import ServiceManagement

/// Bindeglied zwischen Oberfläche und Rust-Kern. Hält die Einstellungen
/// (UserDefaults) und den zuletzt gemeldeten Status.
@MainActor
@Observable
final class AppModel {
    /// Eine Instanz für Menü, Einstellungen, URL-Befehle und Kurzbefehle
    static let shared = AppModel()

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
    /// Lampe aus bei Ruhezustand, Bildschirm aus und Sperre
    var offWhenAway: Bool { didSet { persist() } }
    var cameraEnabled: Bool { didSet { persist() } }
    var cameraHex: String { didSet { persist() } }
    var onAirUrl: String { didSet { persist() } }
    var remindersEnabled: Bool { didSet { persist(); updateCalendar() } }
    var reminderHex: String { didSet { persist() } }
    var reminderLeadMinutes: Double { didSet { persist() } }
    /// Termine aus den Kalendern von macOS (EventKit)
    var useSystemCalendar: Bool { didSet { persist(); updateCalendar() } }
    /// Termine zusätzlich direkt aus Nextcloud
    var nextcloudEnabled: Bool { didSet { persist() } }
    var nextcloudServer: String { didSet { persist() } }
    var nextcloudUser: String { didSet { persist() } }
    /// Liegt im Schlüsselbund
    var nextcloudPassword: String { didSet { Keychain.setPassword(nextcloudPassword); persist() } }
    var favorites: [String] { didSet { persist(pushConfig: false) } }
    /// Eigene Farbe je Programm, erster passender Eintrag gewinnt
    var appColors: [AppColor] { didSet { persist() } }
    var timerHex: String { didSet { persist() } }
    /// Fokus von macOS auf der Lampe zeigen
    var focusEnabled: Bool { didSet { persist(); updateFocus() } }
    var focusHex: String { didSet { persist() } }

    // MARK: Status aus dem Kern

    private(set) var micUsers: [MicUser] = []
    private(set) var cameraOn = false
    private(set) var overriding = false
    private(set) var live = false
    /// Ende des Konzentrations-Timers
    private(set) var timerUntil: Date?
    private(set) var focusOn = false
    /// Fokus lässt sich nicht direkt lesen, nur über Kurzbefehle
    private(set) var focusUnavailable = false
    private(set) var shownHex: String?
    private(set) var baseHex: String?
    private(set) var error: String?

    var micActive: Bool { !micUsers.isEmpty }

    private var controller: MicLedController?
    private var awayObservers: [NSObjectProtocol] = []
    private var calendar: CalendarWatcher?
    private var focusWatcher: FocusWatcher?
    private let defaults = UserDefaults.standard

    static let defaultFavorites = ["00ff00", "ffff00", "ff0000", "0000ff", "ffffff"]

    private init() {
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
            Key.offWhenAway: true,
            Key.cameraEnabled: true,
            Key.cameraColor: "ff00ff",
            Key.onAirUrl: "",
            Key.remindersEnabled: false,
            Key.reminderColor: "ffff00",
            Key.reminderLead: 2.0,
            Key.useSystemCalendar: true,
            Key.nextcloudEnabled: false,
            Key.nextcloudServer: "",
            Key.nextcloudUser: "",
            Key.favorites: AppModel.defaultFavorites,
            Key.appColors: [[String: String]](),
            Key.timerColor: "8000ff",
            Key.focusEnabled: false,
            Key.focusColor: "ff8800",
        ])
        enabled = d.bool(forKey: Key.enabled)
        useWebhook = d.bool(forKey: Key.useWebhook)
        luxaforId = d.string(forKey: Key.luxaforId) ?? ""
        micOnHex = d.string(forKey: Key.micOn) ?? "ff0000"
        micOffHex = d.string(forKey: Key.micOff) ?? "00ff00"
        restorePrevious = d.bool(forKey: Key.restorePrevious)
        releaseDelaySeconds = d.double(forKey: Key.releaseDelay)
        ignored = d.stringArray(forKey: Key.ignored) ?? defaultIgnored()
        offWhenAway = d.bool(forKey: Key.offWhenAway)
        cameraEnabled = d.bool(forKey: Key.cameraEnabled)
        cameraHex = d.string(forKey: Key.cameraColor) ?? "ff00ff"
        onAirUrl = d.string(forKey: Key.onAirUrl) ?? ""
        remindersEnabled = d.bool(forKey: Key.remindersEnabled)
        reminderHex = d.string(forKey: Key.reminderColor) ?? "ffff00"
        reminderLeadMinutes = d.double(forKey: Key.reminderLead)
        useSystemCalendar = d.bool(forKey: Key.useSystemCalendar)
        nextcloudEnabled = d.bool(forKey: Key.nextcloudEnabled)
        nextcloudServer = d.string(forKey: Key.nextcloudServer) ?? ""
        nextcloudUser = d.string(forKey: Key.nextcloudUser) ?? ""
        nextcloudPassword = Keychain.password()
        favorites = d.stringArray(forKey: Key.favorites) ?? AppModel.defaultFavorites
        timerHex = d.string(forKey: Key.timerColor) ?? "8000ff"
        focusEnabled = d.bool(forKey: Key.focusEnabled)
        focusHex = d.string(forKey: Key.focusColor) ?? "ff8800"
        appColors = (d.array(forKey: Key.appColors) as? [[String: String]] ?? []).compactMap { entry in
            guard let key = entry["key"], let hex = entry["hex"] else { return nil }
            return AppColor(key: key, hex: hex)
        }

        let observer = Observer { [weak self] status in
            Task { @MainActor in self?.apply(status) }
        }
        let controller = MicLedController(config: config, observer: observer)
        self.controller = controller
        apply(controller.status())
        observeAway()
        updateCalendar()
        updateFocus()
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

    /// Konzentrations-Timer starten, 0 beendet ihn
    func startTimer(minutes: Int) {
        controller?.startTimer(minutes: UInt32(max(0, minutes)))
    }

    func stopTimer() {
        controller?.startTimer(minutes: 0)
    }

    static let timerChoices = [15, 25, 50, 90]

    /// `micled://color/ff8800`, `micled://off`, `micled://auto/on|off|toggle`
    func handle(url: URL) {
        switch parseUrlCommand(url: url.absoluteString) {
        case .setColor(let hex): setColor(hex)
        case .off: turnOff()
        case .automatic(let on): enabled = on ?? !enabled
        case .timer(let minutes): startTimer(minutes: Int(minutes))
        case .focus(let on): controller?.setFocus(on: on)
        case nil: error = "Unbekannter Befehl: \(url.absoluteString)"
        }
    }

    /// Zugang zu Nextcloud prüfen, liefert eine Meldung für die Einstellungen
    func checkNextcloud() async -> String {
        let (server, user, password) = (nextcloudServer, nextcloudUser, nextcloudPassword)
        let result = await Task.detached {
            nextcloudCheck(server: server, user: user, password: password)
        }.value
        if let error = result.error { return error }
        return result.calendars == 1 ? "Verbunden, 1 Kalender" : "Verbunden, \(result.calendars) Kalender"
    }

    /// Sekunden auf Sendung seit Mitternacht
    func liveSecondsToday() -> UInt64 {
        let midnight = Calendar.current.startOfDay(for: Date())
        return controller?.liveSecondsSince(since: UInt64(midnight.timeIntervalSince1970)) ?? 0
    }

    /// Programm in die Liste „Farbe je Programm“ aufnehmen (Start mit der Mikrofarbe)
    func assignColor(_ user: MicUser) {
        let key = user.bundleId.isEmpty ? user.name : user.bundleId
        if !appColors.contains(where: { $0.key == key }) { appColors.append(AppColor(key: key, hex: micOnHex)) }
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
            ignored: ignored,
            offWhenAway: offWhenAway,
            cameraEnabled: cameraEnabled,
            cameraColor: cameraHex,
            onAirUrl: onAirUrl,
            reminderColor: reminderHex,
            reminderLeadS: UInt32(max(0, reminderLeadMinutes) * 60),
            nextcloudServer: nextcloudActive ? nextcloudServer : "",
            nextcloudUser: nextcloudActive ? nextcloudUser : "",
            nextcloudPassword: nextcloudActive ? nextcloudPassword : "",
            appColors: appColors,
            timerColor: timerHex,
            focusEnabled: focusEnabled,
            focusColor: focusHex
        )
    }

    private var nextcloudActive: Bool {
        remindersEnabled && nextcloudEnabled
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
        defaults.set(offWhenAway, forKey: Key.offWhenAway)
        defaults.set(cameraEnabled, forKey: Key.cameraEnabled)
        defaults.set(cameraHex, forKey: Key.cameraColor)
        defaults.set(onAirUrl, forKey: Key.onAirUrl)
        defaults.set(remindersEnabled, forKey: Key.remindersEnabled)
        defaults.set(reminderHex, forKey: Key.reminderColor)
        defaults.set(reminderLeadMinutes, forKey: Key.reminderLead)
        defaults.set(useSystemCalendar, forKey: Key.useSystemCalendar)
        defaults.set(nextcloudEnabled, forKey: Key.nextcloudEnabled)
        defaults.set(nextcloudServer, forKey: Key.nextcloudServer)
        defaults.set(nextcloudUser, forKey: Key.nextcloudUser)
        defaults.set(favorites, forKey: Key.favorites)
        defaults.set(timerHex, forKey: Key.timerColor)
        defaults.set(focusEnabled, forKey: Key.focusEnabled)
        defaults.set(focusHex, forKey: Key.focusColor)
        defaults.set(appColors.map { ["key": $0.key, "hex": $0.hex] }, forKey: Key.appColors)
        if pushConfig { controller?.updateConfig(config: config) }
    }

    /// Ruhezustand, Bildschirm aus und Sperre an den Kern melden.
    /// Was daraus für die Lampe folgt, entscheidet der Kern.
    private func observeAway() {
        let ws = NSWorkspace.shared.notificationCenter
        let dist = DistributedNotificationCenter.default()
        let events: [(NotificationCenter, Notification.Name, AwayReason, Bool)] = [
            (ws, NSWorkspace.willSleepNotification, .sleep, true),
            (ws, NSWorkspace.didWakeNotification, .sleep, false),
            (ws, NSWorkspace.screensDidSleepNotification, .displaySleep, true),
            (ws, NSWorkspace.screensDidWakeNotification, .displaySleep, false),
            (dist, Notification.Name("com.apple.screenIsLocked"), .locked, true),
            (dist, Notification.Name("com.apple.screenIsUnlocked"), .locked, false),
        ]
        awayObservers = events.map { center, name, reason, away in
            center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.controller?.setAway(reason: reason, away: away)
                }
            }
        }
    }

    /// Kalender von macOS nur lesen, wenn Vorwarnung und Quelle eingeschaltet sind
    private func updateCalendar() {
        guard remindersEnabled, useSystemCalendar else {
            calendar?.stop()
            calendar = nil
            return
        }
        guard calendar == nil else { return }
        let watcher = CalendarWatcher { [weak self] starts in
            self?.controller?.setUpcoming(starts: starts)
        } denied: { [weak self] in
            self?.error = "Kein Zugriff auf den Kalender. Systemeinstellungen › Datenschutz › Kalender"
        }
        calendar = watcher
        watcher.start()
    }

    /// Fokus nur abfragen, wenn er angezeigt werden soll
    private func updateFocus() {
        guard focusEnabled else {
            focusWatcher?.stop()
            focusWatcher = nil
            return
        }
        guard focusWatcher == nil else { return }
        let watcher = FocusWatcher { [weak self] on in
            self?.controller?.setFocus(on: on)
        } unavailable: { [weak self] in
            self?.focusUnavailable = true
        }
        focusWatcher = watcher
        watcher.start()
    }

    private func apply(_ s: MicLedStatus) {
        micUsers = s.micUsers
        cameraOn = s.camera
        overriding = s.overriding
        live = s.live
        timerUntil = s.timerUntil.map { Date(timeIntervalSince1970: TimeInterval($0)) }
        focusOn = s.focus
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
        static let offWhenAway = "offWhenAway"
        static let cameraEnabled = "cameraEnabled"
        static let cameraColor = "cameraColor"
        static let onAirUrl = "onAirUrl"
        static let remindersEnabled = "remindersEnabled"
        static let reminderColor = "reminderColor"
        static let reminderLead = "reminderLeadMinutes"
        static let useSystemCalendar = "useSystemCalendar"
        static let nextcloudEnabled = "nextcloudEnabled"
        static let nextcloudServer = "nextcloudServer"
        static let nextcloudUser = "nextcloudUser"
        static let favorites = "favorites"
        static let appColors = "appColors"
        static let timerColor = "timerColor"
        static let focusEnabled = "focusEnabled"
        static let focusColor = "focusColor"
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
