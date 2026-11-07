import Foundation
import Security

/// Geheimnisse im Schlüsselbund statt in den UserDefaults
/// (App-Passwort für Nextcloud, Slack-Token, App-Schlüssel der Hue Bridge,
/// Schlüssel der Nanoleaf-Lampen)
enum Keychain {
    enum Item: String {
        case nextcloud = "app-password"
        case slack = "slack-token"
        case hue = "hue-app-key"

        var service: String {
            switch self {
            case .nextcloud: "net.staude.micled.nextcloud"
            case .slack: "net.staude.micled.slack"
            case .hue: "net.staude.micled.hue"
            }
        }
    }

    /// Dienst der Nanoleaf-Schlüssel, Account ist die ID des Eintrags
    private static let nanoleafService = "net.staude.micled.nanoleaf"

    private static func query(_ item: Item) -> [String: Any] {
        query(service: item.service, account: item.rawValue)
    }

    private static func query(service: String, account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func read(_ item: Item) -> String {
        read(query(item))
    }

    static func write(_ item: Item, _ value: String) {
        write(query(item), value)
    }

    /// Schlüssel der Nanoleaf-Lampe mit dieser ID, leer wenn nicht gekoppelt
    static func nanoleafKey(_ id: UUID) -> String {
        read(query(service: nanoleafService, account: id.uuidString))
    }

    /// Leer löscht den Schlüssel
    static func setNanoleafKey(_ id: UUID, _ key: String) {
        write(query(service: nanoleafService, account: id.uuidString), key)
    }

    private static func read(_ query: [String: Any]) -> String {
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    private static func write(_ query: [String: Any], _ value: String) {
        if value.isEmpty {
            SecItemDelete(query as CFDictionary)
            return
        }
        let data = Data(value.utf8)
        let status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var q = query
            q[kSecValueData as String] = data
            SecItemAdd(q as CFDictionary, nil)
        }
    }
}
