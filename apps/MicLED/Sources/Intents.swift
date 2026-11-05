import AppIntents

/// Aktionen für die Kurzbefehle-App, z. B. in einer Fokus-Automation
/// („Fokus Arbeiten an → Lampenfarbe gelb“).

enum MicLEDIntentError: Error, CustomLocalizedStringResourceConvertible {
    case badColor(String)

    var localizedStringResource: LocalizedStringResource {
        switch self {
        case .badColor(let text): "„\(text)“ ist keine Farbe. Erlaubt sind z. B. ff8800, #f80 oder rot."
        }
    }
}

struct SetColorIntent: AppIntent {
    static let title: LocalizedStringResource = "Lampenfarbe setzen"
    static let description = IntentDescription("Setzt die Luxafor-Lampe auf eine Farbe, z. B. ff8800, #f80 oder rot.")

    @Parameter(title: "Farbe")
    var color: String

    static var parameterSummary: some ParameterSummary {
        Summary("Lampe auf \(\.$color) setzen")
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        guard let hex = parseColor(text: color) else { throw MicLEDIntentError.badColor(color) }
        AppModel.shared.setColor(hex)
        return .result()
    }
}

struct TurnOffIntent: AppIntent {
    static let title: LocalizedStringResource = "Lampe ausschalten"

    @MainActor
    func perform() async throws -> some IntentResult {
        AppModel.shared.turnOff()
        return .result()
    }
}

enum AutomaticMode: String, AppEnum {
    case on, off, toggle

    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Automatik"
    static let caseDisplayRepresentations: [AutomaticMode: DisplayRepresentation] = [
        .on: "Ein",
        .off: "Aus",
        .toggle: "Umschalten",
    ]
}

struct SetAutomaticIntent: AppIntent {
    static let title: LocalizedStringResource = "Mikrofon-Automatik schalten"

    @Parameter(title: "Automatik", default: .toggle)
    var mode: AutomaticMode

    static var parameterSummary: some ParameterSummary {
        Summary("Automatik \(\.$mode)")
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        let model = AppModel.shared
        switch mode {
        case .on: model.enabled = true
        case .off: model.enabled = false
        case .toggle: model.enabled.toggle()
        }
        return .result()
    }
}

struct StartTimerIntent: AppIntent {
    static let title: LocalizedStringResource = "Konzentration starten"
    static let description = IntentDescription("Zeigt die Timerfarbe für die gewählte Zeit, am Ende blinkt die Lampe.")

    @Parameter(title: "Minuten", default: 25, inclusiveRange: (1, 600))
    var minutes: Int

    static var parameterSummary: some ParameterSummary {
        Summary("Konzentration für \(\.$minutes) Minuten")
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        AppModel.shared.startTimer(minutes: minutes)
        return .result()
    }
}

struct StopTimerIntent: AppIntent {
    static let title: LocalizedStringResource = "Konzentration beenden"

    @MainActor
    func perform() async throws -> some IntentResult {
        AppModel.shared.stopTimer()
        return .result()
    }
}

struct SetFocusIntent: AppIntent {
    static let title: LocalizedStringResource = "Fokus an MicLED melden"
    static let description = IntentDescription("Für eine Fokus-Automation: zeigt die Fokusfarbe, solange der Fokus an ist.")

    @Parameter(title: "Fokus an", default: true)
    var on: Bool

    @MainActor
    func perform() async throws -> some IntentResult {
        AppModel.shared.handle(url: URL(string: on ? "micled://focus/on" : "micled://focus/off")!)
        return .result()
    }
}

struct MicLEDShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: TurnOffIntent(),
            phrases: ["\(.applicationName) ausschalten", "Lampe aus mit \(.applicationName)"],
            shortTitle: "Lampe aus",
            systemImageName: "lightbulb.slash"
        )
        AppShortcut(
            intent: StartTimerIntent(),
            phrases: ["Konzentration mit \(.applicationName) starten"],
            shortTitle: "Konzentration",
            systemImageName: "timer"
        )
        AppShortcut(
            intent: SetAutomaticIntent(),
            phrases: ["\(.applicationName) Automatik umschalten"],
            shortTitle: "Automatik",
            systemImageName: "mic.badge.plus"
        )
    }
}
