import AppKit

/// Symbol für die Menüleiste: Kreis in der aktuellen Lampenfarbe,
/// bei aktivem Mikrofon mit Mikrofon-Zeichen.
enum StatusIcon {
    static func image(hex: String?, micActive: Bool) -> NSImage {
        let size = NSSize(width: 18, height: 18)
        let image = NSImage(size: size, flipped: false) { rect in
            let circle = NSBezierPath(ovalIn: rect.insetBy(dx: 2, dy: 2))
            if let hex, !isOff(hex) {
                NSColor(hex: hex).setFill()
                circle.fill()
                NSColor.black.withAlphaComponent(0.25).setStroke()
            } else {
                // aus oder unbekannt: nur Ring
                NSColor.secondaryLabelColor.setStroke()
            }
            circle.lineWidth = 1.2
            circle.stroke()

            if micActive,
               let mic = NSImage(systemSymbolName: "mic.fill", accessibilityDescription: "Mikrofon aktiv")?
                .withSymbolConfiguration(.init(pointSize: 9, weight: .bold)
                    .applying(.init(paletteColors: [.white])))
            {
                let m = mic.size
                mic.draw(in: NSRect(x: (rect.width - m.width) / 2, y: (rect.height - m.height) / 2,
                                    width: m.width, height: m.height))
            }
            return true
        }
        image.isTemplate = false
        image.accessibilityDescription = micActive ? "MicLED, Mikrofon aktiv" : "MicLED"
        return image
    }
}
