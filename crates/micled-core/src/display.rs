//! Bilder für Displays wie den Pixoo 64: Aus dem Zustand von MicLED wird eine
//! [`Scene`], aus der Szene ein Bild aus 64 × 64 Pixeln. Rein und ohne Ein-/Ausgabe.

use crate::color::Rgb;

/// Kantenlänge des Bildes in Pixeln
pub const SIZE: usize = 64;

/// Was das Display gerade zeigen soll
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scene {
    /// Nichts von MicLED: Das Display zeigt wieder, was vorher lief (z. B. die Uhr)
    Idle,
    /// Auf Sendung, mit Mikro- oder Kamerasymbol und „ON AIR“
    Live { camera: bool, color: Rgb },
    /// Konzentrations-Timer mit den restlichen Minuten
    Timer { minutes: u32, color: Rgb },
    /// Fokus von macOS
    Focus { color: Rgb },
}

impl Scene {
    /// Dieselbe Szene mit umgerechneter Farbe (z. B. abends gedimmt)
    pub fn map_color(self, f: impl Fn(Rgb) -> Rgb) -> Scene {
        match self {
            Scene::Idle => Scene::Idle,
            Scene::Live { camera, color } => Scene::Live { camera, color: f(color) },
            Scene::Timer { minutes, color } => Scene::Timer { minutes, color: f(color) },
            Scene::Focus { color } => Scene::Focus { color: f(color) },
        }
    }
}

/// Bild zur Szene: 64 · 64 · 3 Bytes, Zeilen von oben nach unten, je Pixel R G B,
/// Hintergrund schwarz. `Idle` hat kein Bild und liefert einen leeren Vec.
pub fn render(scene: &Scene) -> Vec<u8> {
    let mut c = Canvas::new();
    match scene {
        Scene::Idle => return Vec::new(),
        Scene::Live { camera, color } => {
            if *camera {
                c.camera(20, 8, *color);
            } else {
                c.mic(20, 6, *color);
            }
            c.text_centered("ON AIR", 44, 1, *color);
        }
        Scene::Timer { minutes, color } => {
            let digits = if *minutes > 99 { "99+".to_string() } else { format!("{minutes:02}") };
            c.text_centered(&digits, 14, 2, *color);
            c.text_centered("MIN", 40, 1, *color);
        }
        Scene::Focus { color } => c.text_centered("FOKUS", 25, 2, *color),
    }
    c.px
}

struct Canvas {
    px: Vec<u8>,
}

impl Canvas {
    fn new() -> Canvas {
        Canvas { px: vec![0; SIZE * SIZE * 3] }
    }

    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
            let i = (y as usize * SIZE + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&[c.r, c.g, c.b]);
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, c);
            }
        }
    }

    /// Text in der 5×7-Schrift, `scale`-fach vergrößert, waagerecht mittig
    fn text_centered(&mut self, s: &str, y: i32, scale: i32, c: Rgb) {
        let n = s.chars().count() as i32;
        let width = (n * 6 - 1) * scale;
        let mut x = (SIZE as i32 - width) / 2;
        for ch in s.chars() {
            let rows = glyph(ch);
            for (ry, bits) in rows.iter().enumerate() {
                for rx in 0..5 {
                    if bits & (0b10000 >> rx) != 0 {
                        self.rect(x + rx * scale, y + ry as i32 * scale, scale, scale, c);
                    }
                }
            }
            x += 6 * scale;
        }
    }

    /// Mikrofon in einem Feld von 24 × 26 Pixeln ab (`ox`, `oy`)
    fn mic(&mut self, ox: i32, oy: i32, c: Rgb) {
        let inside = |x: f32, y: f32, cx: f32, cy: f32, r: f32| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
        for y in 0..26 {
            for x in 0..24 {
                let (fx, fy) = (x as f32, y as f32);
                // Kapsel: Rechteck mit runden Enden
                let capsule = (7..=16).contains(&x)
                    && match y {
                        0..=4 => inside(fx, fy, 11.5, 4.5, 4.8),
                        5..=10 => true,
                        11..=15 => inside(fx, fy, 11.5, 10.5, 4.8),
                        _ => false,
                    };
                // Bügel: halber Ring unter der Kapsel
                let d = ((fx - 11.5).powi(2) + (fy - 10.5).powi(2)).sqrt();
                let holder = y >= 9 && (7.2..=8.8).contains(&d);
                // Ständer und Fuß
                let stem = (11..=12).contains(&x) && (19..=23).contains(&y);
                let base = (6..=17).contains(&x) && (24..=25).contains(&y);
                if capsule || holder || stem || base {
                    self.set(ox + x, oy + y, c);
                }
            }
        }
    }

    /// Kamera in einem Feld von 24 × 24 Pixeln ab (`ox`, `oy`)
    fn camera(&mut self, ox: i32, oy: i32, c: Rgb) {
        // Gehäuse mit abgeschrägten Ecken
        for y in 5..19 {
            for x in 0..16 {
                let corner = (x == 0 || x == 15) && (y == 5 || y == 18);
                if !corner {
                    self.set(ox + x, oy + y, c);
                }
            }
        }
        // Objektiv: Dreieck nach rechts aufgehend
        for x in 17..24 {
            let half = 1.5 + (x - 17) as f32 * 0.9;
            for y in 0..24 {
                if (y as f32 - 11.5).abs() <= half {
                    self.set(ox + x, oy + y, c);
                }
            }
        }
    }
}

/// 5×7-Zeichen, je Zeile 5 Bits (links = höchstes Bit). Nur, was die Bilder brauchen.
fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORANGE: Rgb = Rgb { r: 0xff, g: 0x88, b: 0x00 };

    fn pixels(img: &[u8]) -> impl Iterator<Item = Rgb> + '_ {
        img.chunks(3).map(|p| Rgb { r: p[0], g: p[1], b: p[2] })
    }

    fn lit(img: &[u8]) -> usize {
        pixels(img).filter(|p| !p.is_off()).count()
    }

    fn all() -> Vec<Scene> {
        vec![
            Scene::Live { camera: false, color: ORANGE },
            Scene::Live { camera: true, color: ORANGE },
            Scene::Timer { minutes: 25, color: ORANGE },
            Scene::Timer { minutes: 120, color: ORANGE },
            Scene::Focus { color: ORANGE },
        ]
    }

    #[test]
    fn image_has_64_by_64_rgb_pixels() {
        for s in all() {
            assert_eq!(render(&s).len(), 12288, "{s:?}");
        }
    }

    #[test]
    fn only_scene_color_on_black() {
        for s in all() {
            let img = render(&s);
            assert!(pixels(&img).all(|p| p == ORANGE || p.is_off()), "{s:?}");
            assert!(lit(&img) > 40, "{s:?} ist erkennbar");
        }
    }

    #[test]
    fn idle_has_no_image() {
        assert!(render(&Scene::Idle).is_empty());
    }

    #[test]
    fn mic_and_camera_differ() {
        let mic = render(&Scene::Live { camera: false, color: ORANGE });
        let cam = render(&Scene::Live { camera: true, color: ORANGE });
        assert_ne!(mic, cam);
    }

    #[test]
    fn timer_shows_remaining_minutes() {
        let a = render(&Scene::Timer { minutes: 25, color: ORANGE });
        let b = render(&Scene::Timer { minutes: 24, color: ORANGE });
        assert_ne!(a, b);
        let over = render(&Scene::Timer { minutes: 100, color: ORANGE });
        assert_eq!(over, render(&Scene::Timer { minutes: 250, color: ORANGE }), "über 99 immer „99+“");
    }

    #[test]
    fn color_follows_the_scene() {
        let red = render(&Scene::Focus { color: Rgb::RED });
        assert!(pixels(&red).all(|p| p == Rgb::RED || p.is_off()));
        assert_ne!(red, render(&Scene::Focus { color: ORANGE }));
    }

    #[test]
    fn map_color_keeps_the_scene() {
        let s = Scene::Timer { minutes: 5, color: Rgb::RED }.map_color(|_| ORANGE);
        assert_eq!(s, Scene::Timer { minutes: 5, color: ORANGE });
        assert_eq!(Scene::Idle.map_color(|_| ORANGE), Scene::Idle);
    }

    #[test]
    fn everything_inside_the_image() {
        // Text am Rand würde abgeschnitten: alle Zeichen passen in die Breite
        for s in ["ON AIR", "FOKUS", "99+", "MIN"] {
            let scale = if s == "FOKUS" || s == "99+" { 2 } else { 1 };
            assert!((s.len() as i32 * 6 - 1) * scale <= SIZE as i32, "{s}");
        }
    }
}
