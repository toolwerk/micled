import EventKit
import Foundation

/// Liest die laufenden Termine und die der nächsten Stunde (ohne Titel) und
/// gibt sie an den Kern weiter. Wann die Lampe blinkt und welches Ende Pixoo
/// und AWTRIX zeigen, entscheidet der Kern.
@MainActor
final class CalendarWatcher {
    private let store = EKEventStore()
    private let push: ([CalendarEvent]) -> Void
    private let denied: () -> Void
    private var timer: Timer?
    private var changeObserver: NSObjectProtocol?

    init(push: @escaping ([CalendarEvent]) -> Void, denied: @escaping () -> Void) {
        self.push = push
        self.denied = denied
    }

    func start() {
        Task { [weak self] in
            guard let store = self?.store else { return }
            let granted = (try? await store.requestFullAccessToEvents()) ?? false
            self?.begin(granted: granted)
        }
    }

    private func begin(granted: Bool) {
        guard granted else {
            denied()
            return
        }
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
        changeObserver = NotificationCenter.default.addObserver(
            forName: .EKEventStoreChanged, object: store, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        if let changeObserver { NotificationCenter.default.removeObserver(changeObserver) }
        changeObserver = nil
        push([])
    }

    private func refresh() {
        let now = Date()
        let predicate = store.predicateForEvents(withStart: now, end: now.addingTimeInterval(3600), calendars: nil)
        // schließt schon begonnene Termine ein, die erst in diesem Fenster enden
        let events = store.events(matching: predicate).map { event in
            CalendarEvent(
                start: Self.unix(event.startDate),
                end: Self.unix(event.endDate),
                allDay: event.isAllDay,
                declined: Self.declined(event)
            )
        }
        push(events)
    }

    private static func unix(_ date: Date?) -> UInt64 {
        guard let date else { return 0 }
        return UInt64(max(0, date.timeIntervalSince1970))
    }

    private static func declined(_ event: EKEvent) -> Bool {
        event.status == .canceled
            || event.attendees?.first(where: \.isCurrentUser)?.participantStatus == .declined
    }
}
