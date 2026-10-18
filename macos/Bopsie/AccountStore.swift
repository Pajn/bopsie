import Foundation
import Security

/// Who this Mac is signed in as. The token is not part of it: it lives in the Keychain.
struct Account: Codable, Equatable {
    var serverURL: String
    var username: String
    var playerName: String
}

/// Keeps the account and the player's settings in user defaults, and the token in the Keychain.
struct AccountStore {
    private let defaults = UserDefaults.standard
    private let service = Bundle.main.bundleIdentifier ?? "Bopsie"

    private enum Key {
        static let account = "account"
        static let clientID = "clientID"
        static let volume = "volume"
    }

    func load() -> Account? {
        guard let data = defaults.data(forKey: Key.account) else { return nil }
        return try? JSONDecoder().decode(Account.self, from: data)
    }

    func save(_ account: Account, token: String) throws {
        try saveToken(token)
        defaults.set(try JSONEncoder().encode(account), forKey: Key.account)
    }

    func clear() {
        defaults.removeObject(forKey: Key.account)
        deleteToken()
    }

    /// Identifies this player to the server, which uses it as the player and queue id. Made once
    /// and kept across sign-ins, so the server keeps seeing the same player.
    var clientID: String {
        if let id = defaults.string(forKey: Key.clientID) { return id }
        let id = UUID().uuidString.lowercased()
        defaults.set(id, forKey: Key.clientID)
        return id
    }

    var volume: UInt8 {
        get {
            defaults.object(forKey: Key.volume) == nil
                ? 100 : UInt8(clamping: defaults.integer(forKey: Key.volume))
        }
        nonmutating set { defaults.set(Int(newValue), forKey: Key.volume) }
    }

    // MARK: Keychain

    private var tokenQuery: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: "token",
        ]
    }

    func token() -> String? {
        var query = tokenQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data
        else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private func saveToken(_ token: String) throws {
        deleteToken()
        var item = tokenQuery
        item[kSecValueData as String] = Data(token.utf8)
        item[kSecAttrLabel as String] = "Bopsie sign-in"
        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeychainError(status: status)
        }
    }

    private func deleteToken() {
        SecItemDelete(tokenQuery as CFDictionary)
    }
}

struct KeychainError: LocalizedError {
    let status: OSStatus

    var errorDescription: String? {
        let message = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
        return "Could not keep the sign-in in the Keychain: \(message)"
    }
}
