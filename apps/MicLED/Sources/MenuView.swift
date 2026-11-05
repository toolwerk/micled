import SwiftUI

/// Fenster, das sich beim Klick auf das Menüleisten-Symbol öffnet.
struct MenuView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openSettings) private var openSettings
    @Environment(\.openWindow) private var openWindow
    @State private var custom = Color.orange

    var body: some View {
        @Bindable var model = model

        VStack(alignment: .leading, spacing: 12) {
            header

            if let error = model.error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Divider()

            Text("Farbe setzen")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack(spacing: 8) {
                ForEach(model.favorites, id: \.self) { hex in
                    Swatch(hex: hex, selected: model.shownHex == hex) { model.setColor(hex) }
                }
                Swatch(hex: "000000", selected: isOff(model.shownHex), symbol: "power") { model.turnOff() }
                ColorPicker("Eigene Farbe", selection: $custom, supportsOpacity: false)
                    .labelsHidden()
                    .onChange(of: custom) { _, new in model.setColor(new.hex) }
            }

            Divider()

            Toggle("Bei Mikrofon automatisch umschalten", isOn: $model.enabled)
                .toggleStyle(.switch)
                .controlSize(.small)
            HStack(spacing: 6) {
                Text("Mikro an")
                Circle().fill(Color(hex: model.overriding ? (model.shownHex ?? model.micOnHex) : model.micOnHex))
                    .frame(width: 10, height: 10)
                Text("· danach")
                if model.restorePrevious {
                    Text("vorherige Farbe")
                } else {
                    Circle().fill(Color(hex: model.micOffHex)).frame(width: 10, height: 10)
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)

            Divider()

            TimelineView(.periodic(from: .now, by: 15)) { context in
                HStack(spacing: 6) {
                    Image(systemName: "timer").foregroundStyle(.secondary)
                    if let until = model.timerUntil, until > context.date {
                        Text("Konzentration noch \(Self.duration(UInt64(until.timeIntervalSince(context.date)) + 59))")
                        Spacer()
                        Button("Beenden") { model.stopTimer() }
                    } else {
                        Text("Konzentration")
                        Spacer()
                        ForEach(AppModel.timerChoices, id: \.self) { m in
                            Button("\(m)") { model.startTimer(minutes: m) }
                        }
                        Text("min").foregroundStyle(.secondary)
                    }
                }
                .font(.caption)
                .buttonStyle(.borderless)
            }

            TimelineView(.periodic(from: .now, by: 30)) { _ in
                HStack {
                    Label("Heute \(Self.duration(model.liveSecondsToday())) auf Sendung", systemImage: "clock")
                    Spacer()
                    Button("Statistik") {
                        NSApp.activate()
                        openWindow(id: "stats")
                    }
                    .buttonStyle(.borderless)
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }

            Divider()

            HStack {
                Button("Einstellungen …") {
                    NSApp.activate()
                    openSettings()
                }
                Spacer()
                Button("Beenden") { NSApp.terminate(nil) }
            }
            .buttonStyle(.borderless)
        }
        .padding(14)
        .frame(width: 300)
    }

    private var header: some View {
        HStack(spacing: 10) {
            Image(nsImage: StatusIcon.image(hex: model.shownHex, micActive: model.micActive, cameraOn: model.cameraOn))
                .resizable()
                .frame(width: 28, height: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.cameraOn ? "Kamera aktiv" : model.micActive ? "Mikrofon aktiv" : "Mikrofon aus")
                    .font(.headline)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
    }

    /// „2 h 40 min“, „12 min“, „0 min“
    static func duration(_ seconds: UInt64) -> String {
        let minutes = seconds / 60
        return minutes >= 60 ? "\(minutes / 60) h \(minutes % 60) min" : "\(minutes) min"
    }

    private var subtitle: String {
        if model.micActive {
            return model.micUsers.map(model.displayName).joined(separator: ", ")
        }
        if model.cameraOn { return "Kamera in Benutzung" }
        guard let hex = model.shownHex else { return "Lampenfarbe noch unbekannt" }
        return isOff(hex) ? "Lampe aus" : "Lampe #\(hex)"
    }
}

/// Farbfeld zum direkten Umschalten
struct Swatch: View {
    let hex: String
    var selected = false
    var symbol: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Circle()
                    .fill(isOff(hex) ? Color.secondary.opacity(0.25) : Color(hex: hex))
                Circle()
                    .strokeBorder(selected ? Color.primary : Color.black.opacity(0.2), lineWidth: selected ? 2 : 1)
                if let symbol {
                    Image(systemName: symbol).font(.system(size: 10, weight: .bold))
                }
            }
            .frame(width: 24, height: 24)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .help(isOff(hex) ? "Aus" : "#\(hex)")
    }
}
