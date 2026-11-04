import AppKit
import SwiftUI

extension NSColor {
    /// `rrggbb` → NSColor (sRGB). Ungültige Werte werden grau.
    convenience init(hex: String) {
        let s = hex.trimmingCharacters(in: CharacterSet(charactersIn: "# "))
        let v = UInt32(s, radix: 16) ?? 0x808080
        self.init(
            srgbRed: CGFloat((v >> 16) & 0xff) / 255,
            green: CGFloat((v >> 8) & 0xff) / 255,
            blue: CGFloat(v & 0xff) / 255,
            alpha: 1
        )
    }

    var hex: String {
        let c = usingColorSpace(.sRGB) ?? self
        let r = Int((c.redComponent * 255).rounded())
        let g = Int((c.greenComponent * 255).rounded())
        let b = Int((c.blueComponent * 255).rounded())
        return String(format: "%02x%02x%02x", r, g, b)
    }
}

extension Color {
    init(hex: String) {
        self.init(nsColor: NSColor(hex: hex))
    }

    var hex: String {
        NSColor(self).hex
    }
}

/// Binding zwischen einem Hex-String und einem SwiftUI-ColorPicker
func colorBinding(_ hex: Binding<String>) -> Binding<Color> {
    Binding(
        get: { Color(hex: hex.wrappedValue) },
        set: { hex.wrappedValue = $0.hex }
    )
}

func isOff(_ hex: String?) -> Bool {
    hex == "000000"
}
