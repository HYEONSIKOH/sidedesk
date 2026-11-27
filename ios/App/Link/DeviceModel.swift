import Foundation

/// 컴퓨터 목록에 보일 기종 이름. iOS 16 부터 기기 이름은 "iPhone" 으로만 읽혀 기종 번호로 찾는다.
enum DeviceModel {
    static var name: String {
        var info = utsname()
        uname(&info)
        let id = withUnsafeBytes(of: &info.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
        return named(id)
    }

    /// 기종 번호(예: iPad13,1)를 이름으로. 모르는 번호는 iPhone, iPad 같은 종류와 번호.
    static func named(_ id: String) -> String {
        if let name = names[id] { return name }
        if id.hasPrefix("iPhone") { return "iPhone (\(id))" }
        if id.hasPrefix("iPad") { return "iPad (\(id))" }
        return id.isEmpty ? "iPhone" : id
    }

    private static let names: [String: String] = [
        "iPhone12,1": "iPhone 11", "iPhone12,3": "iPhone 11 Pro", "iPhone12,5": "iPhone 11 Pro Max", "iPhone12,8": "iPhone SE (2세대)",
        "iPhone13,1": "iPhone 12 mini", "iPhone13,2": "iPhone 12", "iPhone13,3": "iPhone 12 Pro", "iPhone13,4": "iPhone 12 Pro Max",
        "iPhone14,4": "iPhone 13 mini", "iPhone14,5": "iPhone 13", "iPhone14,2": "iPhone 13 Pro", "iPhone14,3": "iPhone 13 Pro Max",
        "iPhone14,6": "iPhone SE (3세대)", "iPhone14,7": "iPhone 14", "iPhone14,8": "iPhone 14 Plus",
        "iPhone15,2": "iPhone 14 Pro", "iPhone15,3": "iPhone 14 Pro Max", "iPhone15,4": "iPhone 15", "iPhone15,5": "iPhone 15 Plus",
        "iPhone16,1": "iPhone 15 Pro", "iPhone16,2": "iPhone 15 Pro Max",
        "iPhone17,3": "iPhone 16", "iPhone17,4": "iPhone 16 Plus", "iPhone17,1": "iPhone 16 Pro", "iPhone17,2": "iPhone 16 Pro Max",
        "iPhone17,5": "iPhone 16e",
        "iPhone18,3": "iPhone 17", "iPhone18,1": "iPhone 17 Pro", "iPhone18,2": "iPhone 17 Pro Max", "iPhone18,4": "iPhone Air",
        "iPad11,6": "iPad (8세대)", "iPad11,7": "iPad (8세대)", "iPad12,1": "iPad (9세대)", "iPad12,2": "iPad (9세대)",
        "iPad13,18": "iPad (10세대)", "iPad13,19": "iPad (10세대)", "iPad15,7": "iPad (A16)", "iPad15,8": "iPad (A16)",
        "iPad11,3": "iPad Air (3세대)", "iPad11,4": "iPad Air (3세대)", "iPad13,1": "iPad Air (4세대)", "iPad13,2": "iPad Air (4세대)",
        "iPad13,16": "iPad Air (5세대)", "iPad13,17": "iPad Air (5세대)",
        "iPad14,8": "iPad Air 11 (M2)", "iPad14,9": "iPad Air 11 (M2)", "iPad14,10": "iPad Air 13 (M2)", "iPad14,11": "iPad Air 13 (M2)",
        "iPad15,3": "iPad Air 11 (M3)", "iPad15,4": "iPad Air 11 (M3)", "iPad15,5": "iPad Air 13 (M3)", "iPad15,6": "iPad Air 13 (M3)",
        "iPad11,1": "iPad mini (5세대)", "iPad11,2": "iPad mini (5세대)", "iPad14,1": "iPad mini (6세대)", "iPad14,2": "iPad mini (6세대)",
        "iPad16,1": "iPad mini (A17 Pro)", "iPad16,2": "iPad mini (A17 Pro)",
        "iPad13,4": "iPad Pro 11 (3세대)", "iPad13,5": "iPad Pro 11 (3세대)", "iPad13,6": "iPad Pro 11 (3세대)", "iPad13,7": "iPad Pro 11 (3세대)",
        "iPad13,8": "iPad Pro 12.9 (5세대)", "iPad13,9": "iPad Pro 12.9 (5세대)", "iPad13,10": "iPad Pro 12.9 (5세대)", "iPad13,11": "iPad Pro 12.9 (5세대)",
        "iPad14,3": "iPad Pro 11 (4세대)", "iPad14,4": "iPad Pro 11 (4세대)", "iPad14,5": "iPad Pro 12.9 (6세대)", "iPad14,6": "iPad Pro 12.9 (6세대)",
        "iPad16,3": "iPad Pro 11 (M4)", "iPad16,4": "iPad Pro 11 (M4)", "iPad16,5": "iPad Pro 13 (M4)", "iPad16,6": "iPad Pro 13 (M4)",
    ]
}
