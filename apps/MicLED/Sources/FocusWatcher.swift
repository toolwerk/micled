import Foundation
import Intents

/// Fragt ab, ob ein Fokus von macOS an ist, und meldet Wechsel an den Kern.
/// macOS verrät nur „an/aus“, nicht welcher Fokus läuft. Ohne Freigabe
/// bleibt der Weg über eine Kurzbefehle-Automation (`micled://focus/on`).
@MainActor
final class FocusWatcher {
    private let push: (Bool) -> Void
    private let unavailable: () -> Void
    private var timer: Timer?
    private var last: Bool?
    /// Abfragen ohne Antwort; bleibt `isFocused` leer, gilt der Fokus als nicht lesbar
    private var misses = 0

    init(push: @escaping (Bool) -> Void, unavailable: @escaping () -> Void) {
        self.push = push
        self.unavailable = unavailable
    }

    func start() {
        let center = INFocusStatusCenter.default
        if center.authorizationStatus == .authorized {
            begin()
            return
        }
        center.requestAuthorization { status in
            Task { @MainActor [weak self] in
                if status == .authorized { self?.begin() } else { self?.unavailable() }
            }
        }
    }

    private func begin() {
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
    }

    private func refresh() {
        guard let focused = INFocusStatusCenter.default.focusStatus.isFocused else {
            misses += 1
            if misses == 3 { unavailable() }
            return
        }
        misses = 0
        guard focused != last else { return }
        last = focused
        push(focused)
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}
