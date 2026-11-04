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

struct MicLEDShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: TurnOffIntent(),
            phrases: ["\(.applicationName) ausschalten", "Lampe aus mit \(.applicationName)"],
            shortTitle: "Lampe aus",
            systemImageName: "lightbulb.slash"
        )
        AppShortcut(
            intent: SetAutomaticIntent(),
            phrases: ["\(.applicationName) Automatik umschalten"],
            shortTitle: "Automatik",
            systemImageName: "mic.badge.plus"
        )
    }
}
