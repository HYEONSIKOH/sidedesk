import Foundation
import Security

/// 키체인에 이 폰의 키(비밀 32 + 공개 32)와 짝을 맺은 컴퓨터 공개 키 목록을 둔다.
/// 읽기가 실패하면 새 키를 만들지 않는다(덮어쓰면 다시 등록해야 한다). 키가 아예 없거나 깨졌을 때만 만든다.
final class KeychainKeys: KeyStore, @unchecked Sendable {
    static let keyLength = 32
    private let service: String
    private let lock = NSLock()

    init(service: String = "io.github.hyeonsikoh.sidedesk") {
        self.service = service
    }

    func identity() throws -> Identity {
        lock.lock()
        defer { lock.unlock() }
        if let bytes = try read("identity"), bytes.count == Self.keyLength * 2 {
            return Identity(privateKey: bytes.prefix(Self.keyLength), publicKey: bytes.suffix(Self.keyLength))
        }
        // 없거나 깨졌다. 새로 만들고 짝 목록을 비운다(안드로이드 `KeyFiles` 와 같음).
        let pair = Core.newKeyPair()
        try write("identity", pair.privateKey + pair.publicKey)
        try delete("computers")
        return pair
    }

    func computers() throws -> [Data] {
        lock.lock()
        defer { lock.unlock() }
        return Self.split(try read("computers") ?? Data())
    }

    func trust(_ key: Data) throws {
        lock.lock()
        defer { lock.unlock() }
        var keys = Self.split(try read("computers") ?? Data())
        if keys.contains(key) { return }
        keys.append(key)
        try write("computers", keys.reduce(Data(), +))
    }

    static func split(_ bytes: Data) -> [Data] {
        stride(from: 0, to: bytes.count - bytes.count % keyLength, by: keyLength).map {
            Data(bytes[bytes.startIndex + $0 ..< bytes.startIndex + $0 + keyLength])
        }
    }

    private func query(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }

    private func read(_ account: String) throws -> Data? {
        var query = query(account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess: return result as? Data
        case errSecItemNotFound: return nil
        default: throw KeyError(reason: "키를 읽지 못했습니다")
        }
    }

    private func write(_ account: String, _ bytes: Data) throws {
        let found = SecItemUpdate(query(account) as CFDictionary, [kSecValueData as String: bytes] as CFDictionary)
        if found == errSecSuccess { return }
        guard found == errSecItemNotFound else { throw KeyError(reason: "키를 저장하지 못했습니다") }
        var add = query(account)
        add[kSecValueData as String] = bytes
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        guard SecItemAdd(add as CFDictionary, nil) == errSecSuccess else {
            throw KeyError(reason: "키를 저장하지 못했습니다")
        }
    }

    private func delete(_ account: String) throws {
        let status = SecItemDelete(query(account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeyError(reason: "키를 저장하지 못했습니다")
        }
    }
}
