import AppKit
import Foundation
import Intents
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
    var lampKind: LampKind { didSet { persist() } }
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
    var nextcloudPassword: String { didSet { Keychain.write(.nextcloud, nextcloudPassword); persist() } }
    /// Slack-Status bei „Auf Sendung“
    var slackEnabled: Bool { didSet { persist() } }
    /// Liegt im Schlüsselbund
    var slackToken: String { didSet { Keychain.write(.slack, slackToken); persist() } }
    var slackText: String { didSet { persist() } }
    var slackEmoji: String { didSet { persist() } }
    /// Status auf einem Pixoo 64 im eigenen Netz zeigen
    var pixooEnabled: Bool { didSet { persist() } }
    var pixooHost: String { didSet { persist() } }
    /// Status auf einer Pixeluhr mit AWTRIX 3 im eigenen Netz zeigen
    var awtrixEnabled: Bool { didSet { persist() } }
    var awtrixHost: String { didSet { persist() } }
    /// Status auf einer Hue-Lampe oder in einem Raum über die Hue Bridge zeigen
    var hueEnabled: Bool { didSet { persist() } }
    /// Eine neue Adresse löscht Kopplung und Auswahl
    var hueHost: String {
        didSet {
            if hueHost != oldValue {
                batch {
                    hueKey = ""
                    hueFingerprint = ""
                    hueTargetId = ""
                }
                hueChoices = []
                hueChoicesError = nil
            }
            persist()
        }
    }
    /// App-Schlüssel vom Koppeln, liegt im Schlüsselbund
    var hueKey: String { didSet { Keychain.write(.hue, hueKey); persist() } }
    /// SHA-256 des Zertifikats der Bridge vom Koppeln
    var hueFingerprint: String { didSet { persist() } }
    var hueTargetKind: HueTargetKind { didSet { persist() } }
    var hueTargetId: String { didSet { persist() } }
    /// Status auf Govee-Lampen im eigenen Netz zeigen
    var goveeEnabled: Bool { didSet { persist() } }
    var goveeDevices: [NetLight] { didSet { persist() } }
    /// Status auf gekoppelten Nanoleaf-Lampen zeigen
    var nanoleafEnabled: Bool { didSet { persist() } }
    /// Entfernen oder eine neue Adresse löscht den Schlüssel
    var nanoleafDevices: [NetLight] {
        didSet {
            for old in oldValue where nanoleafDevices.first(where: { $0.id == old.id })?.host != old.host {
                nanoleafKeys[old.id] = nil
                if !ScreenshotMode.isActive { Keychain.setNanoleafKey(old.id, "") }
            }
            persist()
        }
    }
    /// Status auf einem USB-Lichtstreifen mit Robobloq-Controller zeigen
    var robobloqEnabled: Bool { didSet { persist() } }
    /// Schlüssel vom Koppeln je Eintrag, liegen im Schlüsselbund
    private(set) var nanoleafKeys: [UUID: String] = [:]
    var favorites: [String] { didSet { persist(pushConfig: false) } }
    /// Eigene Farbe je Programm, erster passender Eintrag gewinnt
    var appColors: [AppColor] { didSet { persist() } }
    var timerHex: String { didSet { persist() } }
    /// Fokus von macOS auf der Lampe zeigen
    var focusEnabled: Bool { didSet { persist(); updateFocus() } }
    var focusHex: String { didSet { persist() } }
    /// Abends dimmen, Zeiten in Minuten nach Mitternacht
    var dimEnabled: Bool { didSet { persist() } }
    var dimFromMinutes: Int { didSet { persist() } }
    var dimToMinutes: Int { didSet { persist() } }
    var dimPercent: Double { didSet { persist() } }

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
    /// Räume und Lampen der gekoppelten Bridge (nur im Speicher)
    private(set) var hueChoices: [HueTarget] = []
    private(set) var hueChoicesError: String?
    /// Ergebnis der letzten Suche (nur im Speicher)
    private(set) var goveeFound: [FoundDevice] = []
    private(set) var nanoleafFound: [FoundDevice] = []

    var micActive: Bool { !micUsers.isEmpty }

    private var controller: MicLedController?
    private var awayObservers: [NSObjectProtocol] = []
    private var calendar: CalendarWatcher?
    private var focusWatcher: FocusWatcher?
    private let defaults: UserDefaults

    static let defaultFavorites = ["00ff00", "ffff00", "ff0000", "0000ff", "ffffff"]

    private init() {
        let d = ScreenshotMode.isActive ? ScreenshotMode.defaults() : UserDefaults.standard
        defaults = d
        d.register(defaults: [
            Key.enabled: true,
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
            Key.slackEnabled: false,
            Key.slackText: "Im Call",
            Key.slackEmoji: ":red_circle:",
            Key.pixooEnabled: false,
            Key.pixooHost: "",
            Key.awtrixEnabled: false,
            Key.awtrixHost: "",
            Key.hueEnabled: false,
            Key.hueHost: "",
            Key.hueFingerprint: "",
            Key.hueTargetKind: "light",
            Key.hueTargetId: "",
            Key.goveeEnabled: false,
            Key.goveeDevices: [[String: String]](),
            Key.nanoleafEnabled: false,
            Key.nanoleafDevices: [[String: String]](),
            Key.robobloqEnabled: false,
            Key.dimEnabled: false,
            Key.dimFrom: 18 * 60,
            Key.dimTo: 7 * 60,
            Key.dimPercent: 50.0,
        ])
        enabled = d.bool(forKey: Key.enabled)
        // ältere Versionen kannten nur USB oder Webhook
        lampKind = d.string(forKey: Key.lampKind).flatMap(LampKind.init(rawValue:))
            ?? (d.bool(forKey: Key.useWebhook) ? .webhook : .usb)
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
        nextcloudPassword = ScreenshotMode.isActive ? "" : Keychain.read(.nextcloud)
        slackEnabled = d.bool(forKey: Key.slackEnabled)
        slackToken = ScreenshotMode.isActive ? "" : Keychain.read(.slack)
        slackText = d.string(forKey: Key.slackText) ?? "Im Call"
        slackEmoji = d.string(forKey: Key.slackEmoji) ?? ":red_circle:"
        pixooEnabled = d.bool(forKey: Key.pixooEnabled)
        pixooHost = d.string(forKey: Key.pixooHost) ?? ""
        awtrixEnabled = d.bool(forKey: Key.awtrixEnabled)
        awtrixHost = d.string(forKey: Key.awtrixHost) ?? ""
        hueEnabled = d.bool(forKey: Key.hueEnabled)
        hueHost = d.string(forKey: Key.hueHost) ?? ""
        hueKey = ScreenshotMode.isActive ? "screenshot" : Keychain.read(.hue)
        hueFingerprint = d.string(forKey: Key.hueFingerprint) ?? ""
        hueTargetKind = d.string(forKey: Key.hueTargetKind) == "room" ? .room : .light
        hueTargetId = d.string(forKey: Key.hueTargetId) ?? ""
        goveeEnabled = d.bool(forKey: Key.goveeEnabled)
        goveeDevices = NetLight.list(d.array(forKey: Key.goveeDevices))
        nanoleafEnabled = d.bool(forKey: Key.nanoleafEnabled)
        nanoleafDevices = NetLight.list(d.array(forKey: Key.nanoleafDevices))
        robobloqEnabled = d.bool(forKey: Key.robobloqEnabled)
        favorites = d.stringArray(forKey: Key.favorites) ?? AppModel.defaultFavorites
        timerHex = d.string(forKey: Key.timerColor) ?? "8000ff"
        focusEnabled = d.bool(forKey: Key.focusEnabled)
        focusHex = d.string(forKey: Key.focusColor) ?? "ff8800"
        dimEnabled = d.bool(forKey: Key.dimEnabled)
        dimFromMinutes = d.integer(forKey: Key.dimFrom)
        dimToMinutes = d.integer(forKey: Key.dimTo)
        dimPercent = d.double(forKey: Key.dimPercent)
        appColors = (d.array(forKey: Key.appColors) as? [[String: String]] ?? []).compactMap { entry in
            guard let key = entry["key"], let hex = entry["hex"] else { return nil }
            return AppColor(key: key, hex: hex)
        }
        nanoleafKeys = Dictionary(uniqueKeysWithValues: nanoleafDevices.map { light in
            (light.id, ScreenshotMode.isActive ? "screenshot" : Keychain.nanoleafKey(light.id))
        })

        if ScreenshotMode.isActive {
            micUsers = [MicUser(pid: 0, bundleId: "us.zoom.xos", name: "Zoom")]
            overriding = true
            live = true
            shownHex = "ff0000"
            baseHex = "0000ff"
            hueChoices = ScreenshotMode.hueChoices
            goveeFound = ScreenshotMode.goveeFound
            return
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

    /// Beim Beenden der App: Kern anhalten (wartet kurz auf Slack)
    func shutdown() {
        controller?.stop()
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

    /// Slack-Token prüfen, liefert eine Meldung für die Einstellungen
    func checkSlack() async -> String {
        let token = slackToken
        let result = await Task.detached { slackCheck(token: token) }.value
        return result.error ?? "Verbunden als \(result.name ?? "?")"
    }

    /// Pixoo 64 prüfen, liefert eine Meldung für die Einstellungen
    func checkPixoo() async -> String {
        let host = pixooHost
        let result = await Task.detached { pixooCheck(host: host) }.value
        return result.error ?? "Pixoo erreichbar"
    }

    /// AWTRIX-Uhr prüfen, liefert eine Meldung für die Einstellungen
    func checkAwtrix() async -> String {
        let host = awtrixHost
        let result = await Task.detached { awtrixCheck(host: host) }.value
        return result.error ?? "AWTRIX erreichbar"
    }

    /// Mit der Bridge gekoppelt (Schlüssel und Zertifikat bekannt)
    var huePaired: Bool { !hueKey.isEmpty && !hueFingerprint.isEmpty }

    /// Gewählte Lampe bzw. gewählter Raum für die Auswahl, leer = keine Auswahl
    var hueSelection: String {
        get { hueTargetId.isEmpty ? "" : "\(hueTargetKind == .room ? "room" : "light"):\(hueTargetId)" }
        set {
            let parts = newValue.split(separator: ":", maxSplits: 1).map(String.init)
            guard parts.count == 2 else { hueTargetId = ""; return }
            batch {
                hueTargetKind = parts[0] == "room" ? .room : .light
                hueTargetId = parts[1]
            }
        }
    }

    /// Mit der Hue Bridge koppeln (Knopf vorher drücken), liefert eine Meldung
    /// für die Einstellungen
    func pairHue() async -> String {
        let host = hueHost
        let result = await Task.detached { huePair(host: host) }.value
        if let error = result.error { return error }
        // Adresse während des Koppelns geändert: Ergebnis gehört zur alten Bridge
        guard host == hueHost else { return "Adresse geändert, bitte erneut koppeln" }
        batch {
            hueKey = result.key
            hueFingerprint = result.fingerprint
        }
        await loadHueTargets()
        return "Gekoppelt ✓"
    }

    /// Räume und Lampen der gekoppelten Bridge laden
    func loadHueTargets() async {
        guard huePaired, !ScreenshotMode.isActive else { return }
        let (host, key, fingerprint) = (hueHost, hueKey, hueFingerprint)
        let result = await Task.detached { hueTargets(host: host, key: key, fingerprint: fingerprint) }.value
        guard host == hueHost else { return }
        hueChoices = result.items
        hueChoicesError = result.error
    }

    /// Govee-Lampen im eigenen Netz suchen, liefert eine Meldung für die Einstellungen
    func searchGovee() async -> String {
        let result = await Task.detached { goveeDiscover() }.value
        goveeFound = result.items
        if let error = result.error { return error }
        return result.items.isEmpty ? "Nichts gefunden" : "\(result.items.count) gefunden"
    }

    /// Nanoleaf-Lampen per Bonjour suchen, liefert eine Meldung für die Einstellungen
    func searchNanoleaf() async -> String {
        let result = await Task.detached { nanoleafDiscover() }.value
        nanoleafFound = result.items
        if let error = result.error { return error }
        return result.items.isEmpty ? "Nichts gefunden" : "\(result.items.count) gefunden"
    }

    /// Mit der Nanoleaf-Lampe koppeln (Einschaltknopf vorher 5–7 Sekunden
    /// halten), liefert eine Meldung für die Einstellungen
    func pairNanoleaf(_ id: UUID) async -> String {
        guard let host = nanoleafDevices.first(where: { $0.id == id })?.host else { return "" }
        let result = await Task.detached { nanoleafPair(host: host) }.value
        if let error = result.error { return error }
        // Adresse während des Koppelns geändert oder Eintrag entfernt
        guard nanoleafDevices.first(where: { $0.id == id })?.host == host else {
            return "Adresse geändert, bitte erneut koppeln"
        }
        nanoleafKeys[id] = result.key
        Keychain.setNanoleafKey(id, result.key)
        persist()
        return "Gekoppelt ✓"
    }

    /// Lichtstreifen per USB testen: Geräteinfo lesen und zwei Sekunden rot,
    /// danach zeigt er wieder den Zustand von MicLED. Liefert eine Meldung für
    /// die Einstellungen.
    func checkRobobloq() async -> String {
        let controller = controller
        let result = await Task.detached { controller?.testStrip() ?? robobloqCheck() }.value
        return result.error ?? "Gefunden: \(result.leds) LEDs, Firmware \(result.firmware) ✓"
    }

    func nanoleafPaired(_ id: UUID) -> Bool {
        !(nanoleafKeys[id] ?? "").isEmpty
    }

    /// Gefundenes Gerät übernehmen
    func add(_ found: FoundDevice, toNanoleaf: Bool) {
        let light = NetLight(id: UUID(), host: found.host, name: found.name)
        if toNanoleaf {
            if !NetLight.contains(nanoleafDevices, found.host, nanoleaf: true) { nanoleafDevices.append(light) }
        } else if !NetLight.contains(goveeDevices, found.host, nanoleaf: false) {
            goveeDevices.append(light)
        }
    }

    /// Sekunden auf Sendung je Tag zwischen aufsteigenden Tagesgrenzen
    func liveSecondsPerDay(bounds: [UInt64]) -> [UInt64] {
        if ScreenshotMode.isActive {
            return bounds.dropLast().map { ScreenshotMode.seconds(forDay: Date(timeIntervalSince1970: TimeInterval($0))) }
        }
        return controller?.liveSecondsPerDay(bounds: bounds) ?? []
    }

    func callsCsv() -> String {
        controller?.callsCsv() ?? "start,end,seconds\n"
    }

    /// Summen der letzten `weeks` Kalenderwochen und die Vorwoche bis jetzt
    func weeklyStats(weeks: UInt32) -> WeeklyStats? {
        if ScreenshotMode.isActive { return ScreenshotMode.weeklyStats(weeks: Int(weeks)) }
        return controller?.weeklyStats(weeks: weeks)
    }

    func weeksCsv() -> String {
        controller?.weeksCsv() ?? "week,start,live_seconds,calls,focus_seconds\n"
    }

    /// Sekunden auf Sendung seit Mitternacht
    func liveSecondsToday() -> UInt64 {
        if ScreenshotMode.isActive { return 2 * 3600 + 10 * 60 }
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
            transport: lampKind.transport,
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
            focusColor: focusHex,
            dimEnabled: dimEnabled,
            dimFromMin: UInt16(clamping: dimFromMinutes),
            dimToMin: UInt16(clamping: dimToMinutes),
            dimPercent: UInt8(clamping: Int(dimPercent)),
            slackToken: slackEnabled ? slackToken : "",
            slackText: slackText,
            slackEmoji: slackEmoji,
            pixooHost: pixooEnabled ? pixooHost : "",
            awtrixHost: awtrixEnabled ? awtrixHost : "",
            hueHost: hueEnabled ? hueHost : "",
            hueKey: hueKey,
            hueFingerprint: hueFingerprint,
            hueTargetKind: hueTargetKind,
            hueTargetId: hueTargetId,
            goveeHosts: goveeEnabled ? goveeDevices.map(\.host) : [],
            nanoleaf: nanoleafEnabled
                ? nanoleafDevices.map { NanoleafDevice(host: $0.host, key: nanoleafKeys[$0.id] ?? "") }
                : [],
            robobloq: robobloqEnabled
        )
    }

    private var nextcloudActive: Bool {
        remindersEnabled && nextcloudEnabled
    }

    /// Unterdrückt `persist()` während `batch`, damit der Kern keine halb
    /// geänderte Kombination (z. B. neuer Schlüssel, alter Fingerabdruck) sieht
    private var batching = false

    /// Mehrere Einstellungen ändern und einmal speichern
    private func batch(_ change: () -> Void) {
        batching = true
        change()
        batching = false
        persist()
    }

    private func persist(pushConfig: Bool = true) {
        if batching { return }
        defaults.set(enabled, forKey: Key.enabled)
        defaults.set(lampKind.rawValue, forKey: Key.lampKind)
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
        defaults.set(slackEnabled, forKey: Key.slackEnabled)
        defaults.set(slackText, forKey: Key.slackText)
        defaults.set(slackEmoji, forKey: Key.slackEmoji)
        defaults.set(pixooEnabled, forKey: Key.pixooEnabled)
        defaults.set(pixooHost, forKey: Key.pixooHost)
        defaults.set(awtrixEnabled, forKey: Key.awtrixEnabled)
        defaults.set(awtrixHost, forKey: Key.awtrixHost)
        defaults.set(hueEnabled, forKey: Key.hueEnabled)
        defaults.set(hueHost, forKey: Key.hueHost)
        defaults.set(hueFingerprint, forKey: Key.hueFingerprint)
        defaults.set(hueTargetKind == .room ? "room" : "light", forKey: Key.hueTargetKind)
        defaults.set(hueTargetId, forKey: Key.hueTargetId)
        defaults.set(goveeEnabled, forKey: Key.goveeEnabled)
        defaults.set(goveeDevices.map(\.stored), forKey: Key.goveeDevices)
        defaults.set(nanoleafEnabled, forKey: Key.nanoleafEnabled)
        defaults.set(nanoleafDevices.map(\.stored), forKey: Key.nanoleafDevices)
        defaults.set(robobloqEnabled, forKey: Key.robobloqEnabled)
        defaults.set(dimEnabled, forKey: Key.dimEnabled)
        defaults.set(dimFromMinutes, forKey: Key.dimFrom)
        defaults.set(dimToMinutes, forKey: Key.dimTo)
        defaults.set(dimPercent, forKey: Key.dimPercent)
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

    /// Fokus abfragen, wenn er angezeigt werden soll oder die Freigabe schon
    /// besteht (für die Fokuszeit in der Statistik). Ohne Anzeige wird nicht
    /// nach der Freigabe gefragt.
    private func updateFocus() {
        guard focusEnabled || INFocusStatusCenter.default.authorizationStatus == .authorized else {
            focusWatcher?.stop()
            focusWatcher = nil
            // sonst zählte ein beim Abschalten laufender Fokus in der Statistik weiter
            controller?.setFocus(on: false)
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
        static let lampKind = "lampKind"
        /// Nur noch zum Übernehmen alter Einstellungen
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
        static let slackEnabled = "slackEnabled"
        static let slackText = "slackText"
        static let slackEmoji = "slackEmoji"
        static let pixooEnabled = "pixooEnabled"
        static let pixooHost = "pixooHost"
        static let awtrixEnabled = "awtrixEnabled"
        static let awtrixHost = "awtrixHost"
        static let hueEnabled = "hueEnabled"
        static let hueHost = "hueHost"
        static let hueFingerprint = "hueFingerprint"
        static let hueTargetKind = "hueTargetKind"
        static let hueTargetId = "hueTargetId"
        static let goveeEnabled = "goveeEnabled"
        static let goveeDevices = "goveeDevices"
        static let nanoleafEnabled = "nanoleafEnabled"
        static let nanoleafDevices = "nanoleafDevices"
        static let robobloqEnabled = "robobloqEnabled"
        static let dimEnabled = "dimEnabled"
        static let dimFrom = "dimFromMinutes"
        static let dimTo = "dimToMinutes"
        static let dimPercent = "dimPercent"
    }
}

/// Govee- oder Nanoleaf-Lampe im eigenen Netz
struct NetLight: Identifiable, Equatable {
    let id: UUID
    var host: String
    /// Modell oder Name aus der Suche, leer bei einer Adresse von Hand
    var name: String

    /// Eintrag für die UserDefaults
    var stored: [String: String] {
        ["id": id.uuidString, "host": host, "name": name]
    }

    static func list(_ stored: [Any]?) -> [NetLight] {
        (stored as? [[String: String]] ?? []).compactMap { entry in
            guard let id = entry["id"].flatMap(UUID.init(uuidString:)), let host = entry["host"] else { return nil }
            return NetLight(id: id, host: host, name: entry["name"] ?? "")
        }
    }

    /// Adresse wie im Kern bereinigt (ohne `http://` und `/`, Nanoleaf mit
    /// Port 16021, Govee ohne Port), zum Vergleichen
    static func normalized(_ host: String, nanoleaf: Bool) -> String {
        var h = host.trimmingCharacters(in: .whitespacesAndNewlines)
        for prefix in ["http://", "https://"] where h.hasPrefix(prefix) { h.removeFirst(prefix.count) }
        while h.hasSuffix("/") { h.removeLast() }
        h = h.trimmingCharacters(in: .whitespacesAndNewlines)
        if let colon = h.lastIndex(of: ":") {
            let port = h[h.index(after: colon)...]
            if !nanoleaf, !port.isEmpty, port.allSatisfy(\.isASCII), port.allSatisfy(\.isNumber) {
                h = String(h[..<colon])
            }
        } else if nanoleaf, !h.isEmpty {
            h += ":16021"
        }
        return h
    }

    /// Steht die Adresse schon in der Liste?
    static func contains(_ lights: [NetLight], _ host: String, nanoleaf: Bool) -> Bool {
        let h = normalized(host, nanoleaf: nanoleaf)
        return lights.contains { normalized($0.host, nanoleaf: nanoleaf) == h }
    }

    /// Adresse ohne den Standard-Port von Nanoleaf
    var shownHost: String {
        host.hasSuffix(":16021") ? String(host.dropLast(6)) : host
    }
}

/// Wie die Lampe angesteuert wird
enum LampKind: String, CaseIterable {
    case usb, webhook, noLamp

    var transport: TransportKind {
        switch self {
        case .usb: .usb
        case .webhook: .webhook
        case .noLamp: .noLamp
        }
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
