#if os(macOS)
import AppKit

enum AppearanceMetrics {

    /// The appearance with light and dark folded together, against the last one measured by.
    static func measuringAppearance(_ appearance: NSAppearance) -> NSAppearance.Name {
        let folded: [NSAppearance.Name: NSAppearance.Name] = [
            .darkAqua: .aqua, .vibrantDark: .vibrantLight,
            .accessibilityHighContrastDarkAqua: .accessibilityHighContrastAqua,
            .accessibilityHighContrastVibrantDark: .accessibilityHighContrastVibrantLight,
        ]
        let name =
            appearance.bestMatch(from: [
                .aqua, .darkAqua, .vibrantLight, .vibrantDark,
                .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua,
                .accessibilityHighContrastVibrantLight, .accessibilityHighContrastVibrantDark,
            ]) ?? appearance.name
        return folded[name] ?? name
    }
}
#endif
