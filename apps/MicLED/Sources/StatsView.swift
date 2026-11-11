import AppKit
import Charts
import SwiftUI

/// Fenster „Statistik“: Zeit auf Sendung je Tag, Wochen mit Fokuszeit und CSV-Export.
struct StatsView: View {
    @Environment(AppModel.self) private var model
    @State private var range: Period = ScreenshotMode.stats == "week" ? .weeks : .days7
    @State private var rows: [Day] = []
    @State private var weekly: WeeklyStats?

    enum Period: Hashable {
        case days7, days30, weeks
        var days: Int { self == .days30 ? 30 : 7 }
    }

    struct Day: Identifiable {
        let date: Date
        let seconds: UInt64
        var id: Date { date }
        var minutes: Double { Double(seconds) / 60 }
    }

    /// Ein Balken der Wochenansicht
    struct WeekBar: Identifiable {
        let label: String
        let kind: String
        let seconds: UInt64
        var id: String { "\(label)-\(kind)" }
        var hours: Double { Double(seconds) / 3600 }
    }

    /// Angezeigte Kalenderwochen
    static let weekCount: UInt32 = 12
    static let live = "Auf Sendung"
    static let focus = "Fokus"

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Picker("Zeitraum", selection: $range) {
                    Text("7 Tage").tag(Period.days7)
                    Text("30 Tage").tag(Period.days30)
                    Text("Wochen").tag(Period.weeks)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 260)
                Spacer()
                Button("Als CSV exportieren …", action: export)
            }

            if range == .weeks {
                weeksView
            } else {
                daysView
            }
        }
        .padding(20)
        .frame(minWidth: 560, minHeight: 360)
        .onAppear(perform: load)
        .onChange(of: range) { load() }
    }

    // MARK: Tage

    @ViewBuilder
    private var daysView: some View {
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
            summary("Summe", MenuView.duration(total))
            summary("Ø je Arbeitstag", MenuView.duration(perWorkday))
            summary("Längster Tag", MenuView.duration(rows.map(\.seconds).max() ?? 0))
        }
    }

    private var total: UInt64 { rows.map(\.seconds).reduce(0, +) }

    private var perWorkday: UInt64 {
        let workdays = rows.filter { !Calendar.current.isDateInWeekend($0.date) }
        guard !workdays.isEmpty else { return 0 }
        return workdays.map(\.seconds).reduce(0, +) / UInt64(workdays.count)
    }

    // MARK: Wochen

    @ViewBuilder
    private var weeksView: some View {
        Chart(weekBars) { bar in
            BarMark(
                x: .value("Woche", bar.label),
                y: .value("Stunden", bar.hours)
            )
            .foregroundStyle(by: .value("Art", bar.kind))
            .position(by: .value("Art", bar.kind))
        }
        .chartForegroundStyleScale([Self.live: Color.red, Self.focus: Color.orange])
        .chartXAxis {
            AxisMarks { value in
                AxisValueLabel {
                    if let label = value.as(String.self) {
                        VStack(spacing: 1) {
                            Text(label)
                            Text("\(callsByLabel[label] ?? 0) Calls").foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
        .chartYAxisLabel("Stunden")
        .chartLegend(position: .top, alignment: .trailing)
        .frame(minHeight: 220)

        if let now = weekly?.weeks.last, let before = weekly?.lastWeekToDate {
            VStack(alignment: .leading, spacing: 6) {
                Text("Diese Woche (KW \(now.week))").font(.headline)
                HStack(alignment: .top, spacing: 24) {
                    summary(Self.live, MenuView.duration(now.liveSeconds),
                            change: Self.change(now.liveSeconds, before.liveSeconds))
                    summary("Calls", "\(now.calls)",
                            change: Self.signed(Int(now.calls) - Int(before.calls)))
                    summary(Self.focus, MenuView.duration(now.focusSeconds),
                            change: Self.change(now.focusSeconds, before.focusSeconds))
                }
                Text("ggü. Vorwoche bis jetzt").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var weekBars: [WeekBar] {
        (weekly?.weeks ?? []).flatMap { w in
            [
                WeekBar(label: "KW \(w.week)", kind: Self.live, seconds: w.liveSeconds),
                WeekBar(label: "KW \(w.week)", kind: Self.focus, seconds: w.focusSeconds),
            ]
        }
    }

    /// Zahl der Calls je Beschriftung „KW <n>“
    private var callsByLabel: [String: UInt32] {
        Dictionary((weekly?.weeks ?? []).map { ("KW \($0.week)", $0.calls) }, uniquingKeysWith: { a, _ in a })
    }

    /// Differenz zweier Dauern mit Vorzeichen, z.B. „+1 h 5 min“
    static func change(_ now: UInt64, _ before: UInt64) -> String {
        now >= before ? "+" + MenuView.duration(now - before) : "−" + MenuView.duration(before - now)
    }

    /// Zahl mit Vorzeichen, z.B. „+2“, „−3“, „±0“
    static func signed(_ n: Int) -> String {
        n > 0 ? "+\(n)" : n < 0 ? "−\(-n)" : "±0"
    }

    // MARK: Gemeinsam

    private func summary(_ title: String, _ value: String, change: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.title3.monospacedDigit())
            if let change {
                Text(change).font(.callout.monospacedDigit()).foregroundStyle(.secondary)
            }
        }
    }

    private func load() {
        if range == .weeks {
            weekly = model.weeklyStats(weeks: Self.weekCount)
            return
        }
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        let starts = (0..<range.days).reversed().compactMap { cal.date(byAdding: .day, value: -$0, to: today) }
        guard let end = cal.date(byAdding: .day, value: 1, to: today) else { return }
        let bounds = (starts + [end]).map { UInt64($0.timeIntervalSince1970) }
        let seconds = model.liveSecondsPerDay(bounds: bounds)
        rows = zip(starts, seconds).map { Day(date: $0, seconds: $1) }
    }

    private func export() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.commaSeparatedText]
        let choice = ExportChoice(panel: panel, weeks: range == .weeks)
        panel.accessoryView = choice.view
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let csv = choice.weeks ? model.weeksCsv() : model.callsCsv()
            try csv.write(to: url, atomically: true, encoding: .utf8)
        } catch {
            NSAlert(error: error).runModal()
        }
    }
}

/// Auswahl „Inhalt“ im Speichern-Dialog: alle Calls oder Summen je Woche
@MainActor
private final class ExportChoice: NSObject {
    private weak var panel: NSSavePanel?
    private let popup = NSPopUpButton()
    let view: NSView

    var weeks: Bool { popup.indexOfSelectedItem == 1 }

    init(panel: NSSavePanel, weeks: Bool) {
        self.panel = panel
        popup.addItems(withTitles: ["Alle Calls", "Summen je Woche"])
        popup.selectItem(at: weeks ? 1 : 0)
        let label = NSTextField(labelWithString: "Inhalt:")
        let stack = NSStackView(views: [label, popup])
        stack.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        stack.frame.size = stack.fittingSize
        view = stack
        super.init()
        popup.target = self
        popup.action = #selector(changed)
        changed()
    }

    @objc private func changed() {
        panel?.nameFieldStringValue = weeks ? "MicLED-Wochen.csv" : "MicLED-Calls.csv"
    }
}
