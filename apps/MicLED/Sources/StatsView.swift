import AppKit
import Charts
import SwiftUI

/// Fenster „Statistik“: Zeit auf Sendung je Tag und CSV-Export.
struct StatsView: View {
    @Environment(AppModel.self) private var model
    @State private var days = 7
    @State private var rows: [Day] = []

    struct Day: Identifiable {
        let date: Date
        let seconds: UInt64
        var id: Date { date }
        var minutes: Double { Double(seconds) / 60 }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Picker("Zeitraum", selection: $days) {
                    Text("7 Tage").tag(7)
                    Text("30 Tage").tag(30)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 180)
                Spacer()
                Button("Als CSV exportieren …", action: export)
            }

            Chart(rows) { day in
                BarMark(
                    x: .value("Tag", day.date, unit: .day),
                    y: .value("Minuten", day.minutes)
                )
                .foregroundStyle(Color.red.gradient)
            }
            .chartYAxisLabel("Minuten")
            .frame(minHeight: 220)

            HStack(spacing: 24) {
                summary("Summe", total)
                summary("Ø je Arbeitstag", perWorkday)
                summary("Längster Tag", rows.map(\.seconds).max() ?? 0)
            }
        }
        .padding(20)
        .frame(minWidth: 520, minHeight: 360)
        .onAppear(perform: load)
        .onChange(of: days) { load() }
    }

    private var total: UInt64 { rows.map(\.seconds).reduce(0, +) }

    private var perWorkday: UInt64 {
        let workdays = rows.filter { !Calendar.current.isDateInWeekend($0.date) }
        guard !workdays.isEmpty else { return 0 }
        return workdays.map(\.seconds).reduce(0, +) / UInt64(workdays.count)
    }

    private func summary(_ title: String, _ seconds: UInt64) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(MenuView.duration(seconds)).font(.title3.monospacedDigit())
        }
    }

    private func load() {
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        let starts = (0..<days).reversed().compactMap { cal.date(byAdding: .day, value: -$0, to: today) }
        guard let end = cal.date(byAdding: .day, value: 1, to: today) else { return }
        let bounds = (starts + [end]).map { UInt64($0.timeIntervalSince1970) }
        let seconds = model.liveSecondsPerDay(bounds: bounds)
        rows = zip(starts, seconds).map { Day(date: $0, seconds: $1) }
    }

    private func export() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "MicLED-Calls.csv"
        panel.allowedContentTypes = [.commaSeparatedText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try model.callsCsv().write(to: url, atomically: true, encoding: .utf8)
        } catch {
            NSAlert(error: error).runModal()
        }
    }
}
