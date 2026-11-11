import AppKit

/// Symbol für die Menüleiste: Kreis in der aktuellen Lampenfarbe,
/// bei aktivem Mikrofon mit Mikrofon-, bei Kamera mit Kamera-Zeichen.
enum StatusIcon {
    static func image(hex: String?, micActive: Bool, cameraOn: Bool = false) -> NSImage {
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

            if micActive || cameraOn,
               let mic = NSImage(systemSymbolName: cameraOn ? "video.fill" : "mic.fill",
                                 accessibilityDescription: cameraOn ? String(localized: "Kamera aktiv") : String(localized: "Mikrofon aktiv"))?
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
        image.accessibilityDescription = cameraOn
            ? String(localized: "MicLED, Kamera aktiv")
            : micActive ? String(localized: "MicLED, Mikrofon aktiv") : "MicLED"
        return image
    }
}
