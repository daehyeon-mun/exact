import Foundation

/// An explicit declaration and printable chord are required before native key echo.
enum PassiveKeyPolicy {
    static func declaredPassive(_ dataset: String?) -> Bool {
        guard let data = dataset?.data(using: .utf8),
            let fields = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else { return false }
        return fields["keys-passive"] as? String == "yes"
    }

    static func printable(_ name: String) -> Bool {
        name.count == 1
            && name.unicodeScalars.allSatisfy {
                $0.properties.generalCategory != .control && !(0xF700...0xF8FF).contains($0.value)
            }
    }

    /// The chord's key name, its modifiers taken off.
    static func keyName(_ chord: String) -> (name: String, command: Bool) {
        var rest = Substring(chord), command = false
        for prefix in ["Shift+", "Control+", "Alt+", "Meta+"] where rest.hasPrefix(prefix) && rest.count > prefix.count {
            if prefix == "Control+" || prefix == "Meta+" { command = true }
            rest = rest.dropFirst(prefix.count)
        }
        // The prefixes come in `KeyCodes.held`'s order; a second pass takes what the first left.
        for prefix in ["Shift+", "Control+", "Alt+", "Meta+"] where rest.hasPrefix(prefix) && rest.count > prefix.count {
            if prefix == "Control+" || prefix == "Meta+" { command = true }
            rest = rest.dropFirst(prefix.count)
        }
        return (String(rest), command)
    }

}
