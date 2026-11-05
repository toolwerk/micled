import Foundation
import Security

/// App-Passwort für Nextcloud im Schlüsselbund (nicht in den UserDefaults)
enum Keychain {
    private static let service = "net.staude.micled.nextcloud"
    private static let account = "app-password"

    private static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func password() -> String {
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    static func setPassword(_ value: String) {
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
