import EventKit
import Foundation

/// Liest die Startzeiten der Termine der nächsten Stunde und gibt sie an den
/// Kern weiter. Wann die Lampe blinkt, entscheidet der Kern.
@MainActor
final class CalendarWatcher {
    private let store = EKEventStore()
    private let push: ([UInt64]) -> Void
    private let denied: () -> Void
    private var timer: Timer?
    private var changeObserver: NSObjectProtocol?

    init(push: @escaping ([UInt64]) -> Void, denied: @escaping () -> Void) {
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
        let starts = store.events(matching: predicate)
            .filter { !$0.isAllDay && $0.startDate > now && !Self.declined($0) }
            .map { UInt64($0.startDate.timeIntervalSince1970) }
        push(starts)
    }

    private static func declined(_ event: EKEvent) -> Bool {
        event.attendees?.first(where: \.isCurrentUser)?.participantStatus == .declined
    }
}
