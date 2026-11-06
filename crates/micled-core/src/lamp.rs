//! Ansteuerung der Luxafor-Lampe per USB-HID oder Webhook-API.
//!
//! USB-Protokoll (9-Byte-Report, erstes Byte Report-ID 0):
//! - `01 FF r g b` feste Farbe auf allen LEDs
//! - `00 4F` Simple-Modus „O“, bricht laufende Muster/Blinken ab
//!
//! Die Lampe kennt keinen Lesebefehl für die aktuelle Farbe. Wer die Farbe
//! wissen will, muss sie sich merken (siehe [`crate::shared`]).

use std::thread::sleep;
use std::time::Duration;

use crate::color::Rgb;

pub const VENDOR_ID: u16 = 0x04d8;
pub const PRODUCT_ID: u16 = 0xf372;
const WEBHOOK_URL: &str = "https://api.luxafor.com/webhook/v1/actions/solid_color";

#[derive(Debug, thiserror::Error)]
pub enum LampError {
    #[error("Luxafor per USB nicht gefunden (angesteckt?): {0}")]
    Usb(String),
    #[error("Keine Luxafor-ID für den Webhook eingetragen")]
    MissingId,
    #[error("Webhook fehlgeschlagen: {0}")]
    Webhook(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    Usb,
    Webhook { luxafor_id: String },
    /// Keine Lampe: MicLED zeigt den Status nur auf Displays, in Slack und am Webhook
    None,
}

pub trait Lamp: Send {
    fn set(&mut self, color: Rgb) -> Result<(), LampError>;
}

/// Erzeugt die passende Lampe für den Transport.
pub fn open(transport: &Transport) -> Box<dyn Lamp> {
    match transport {
        Transport::Usb => Box::new(UsbLamp),
        Transport::Webhook { luxafor_id } => Box::new(WebhookLamp { id: luxafor_id.trim().to_string() }),
        Transport::None => Box::new(NoLamp),
    }
}

/// Ohne Lampe: jede Farbe gilt sofort als gesetzt, es gibt nie einen Fehler
pub struct NoLamp;

impl Lamp for NoLamp {
    fn set(&mut self, _: Rgb) -> Result<(), LampError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// USB
// ---------------------------------------------------------------------------

/// Öffnet das Gerät nur für den Befehl und gibt es danach wieder frei, damit
/// Luxafor-App, Stream-Deck-Plugin und `lux` parallel funktionieren.
pub struct UsbLamp;

impl UsbLamp {
    fn write(reports: &[&[u8]]) -> Result<(), LampError> {
        let api = hidapi::HidApi::new().map_err(|e| LampError::Usb(e.to_string()))?;
        let dev = api.open(VENDOR_ID, PRODUCT_ID).map_err(|e| LampError::Usb(e.to_string()))?;
        for (i, bytes) in reports.iter().enumerate() {
            if i > 0 {
                sleep(Duration::from_millis(60));
            }
            let mut report = [0u8; 9];
            report[1..1 + bytes.len()].copy_from_slice(bytes);
            dev.write(&report).map_err(|e| LampError::Usb(e.to_string()))?;
        }
        Ok(())
    }
}

impl Lamp for UsbLamp {
    fn set(&mut self, c: Rgb) -> Result<(), LampError> {
        // Erst evtl. laufendes Muster stoppen, dann Farbe doppelt setzen:
        // während einer Animation ignoriert die Firmware einzelne Befehle.
        let solid = [0x01, 0xff, c.r, c.g, c.b];
        Self::write(&[&[0x00, 0x4f], &solid, &solid])
    }
}

// ---------------------------------------------------------------------------
// Webhook (Luxafor-App muss laufen)
// ---------------------------------------------------------------------------

pub struct WebhookLamp {
    id: String,
}

impl Lamp for WebhookLamp {
    fn set(&mut self, c: Rgb) -> Result<(), LampError> {
        if self.id.is_empty() {
            return Err(LampError::MissingId);
        }
        let body = serde_json::json!({
            "userId": self.id,
            "actionFields": { "color": "custom", "custom_color": c.hex() }
        });
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .into();
        agent
            .post(WEBHOOK_URL)
            .send_json(&body)
            .map(|_| ())
            .map_err(|e| LampError::Webhook(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lamp_never_fails() {
        assert!(open(&Transport::None).set(Rgb::RED).is_ok());
    }
}
