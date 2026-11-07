import Foundation
import Security

/// Geheimnisse im Schlüsselbund statt in den UserDefaults
/// (App-Passwort für Nextcloud, Slack-Token, App-Schlüssel der Hue Bridge)
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

    private static func query(_ item: Item) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: item.service,
         kSecAttrAccount as String: item.rawValue]
    }

    static func read(_ item: Item) -> String {
        var q = query(item)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    static func write(_ item: Item, _ value: String) {
        if value.isEmpty {
            SecItemDelete(query(item) as CFDictionary)
            return
        }
        let data = Data(value.utf8)
        let status = SecItemUpdate(query(item) as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var q = query(item)
            q[kSecValueData as String] = data
            SecItemAdd(q as CFDictionary, nil)
        }
    }
}
