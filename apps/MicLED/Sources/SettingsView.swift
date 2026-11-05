import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var current: [MicUser] = []
    @State private var nextcloudResult = ""

    var body: some View {
        @Bindable var model = model

        Form {
            Section("Verbindung zur Lampe") {
                Picker("Ansteuerung", selection: $model.useWebhook) {
                    Text("USB direkt").tag(false)
                    Text("Webhook (Luxafor-App)").tag(true)
                }
                .pickerStyle(.segmented)
                if model.useWebhook {
                    TextField("Luxafor-ID", text: $model.luxaforId, prompt: Text("Luxafor-App, Tab „Webhook“"))
                    Text("Die Luxafor-App muss laufen. Sie gibt die Befehle an die Lampe weiter.")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    Text("Funktioniert ohne Luxafor-App. Läuft die Luxafor-App, kann sie die Farbe überschreiben.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }

            Section("Mikrofon") {
                Toggle("Automatisch umschalten", isOn: $model.enabled)
                ColorPicker("Farbe bei Mikro an", selection: colorBinding($model.micOnHex), supportsOpacity: false)
                Picker("Wenn das Mikro aus geht", selection: $model.restorePrevious) {
                    Text("Vorherige Farbe wiederherstellen").tag(true)
                    Text("Feste Farbe setzen").tag(false)
                }
                ColorPicker(
                    model.restorePrevious ? "Farbe, falls keine vorherige bekannt" : "Farbe bei Mikro aus",
                    selection: colorBinding($model.micOffHex),
                    supportsOpacity: false
                )
                LabeledContent("Zurückschalten nach") {
                    Stepper(value: $model.releaseDelaySeconds, in: 0...60, step: 1) {
                        Text("\(Int(model.releaseDelaySeconds)) s Stille")
                    }
                }
            }

            Section {
                Toggle("Kamera auswerten", isOn: $model.cameraEnabled)
                ColorPicker("Farbe bei Kamera an", selection: colorBinding($model.cameraHex), supportsOpacity: false)
                    .disabled(!model.cameraEnabled)
            } header: {
                Text("Kamera")
            } footer: {
                Text("Kamera an zählt wie Mikro an, hat aber eine eigene Farbe.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                TextField("Adresse", text: $model.onAirUrl, prompt: Text("http://homeassistant.local:8123/api/webhook/…"))
                    .autocorrectionDisabled()
            } header: {
                Text("„Auf Sendung“ melden")
            } footer: {
                Text("Bei jedem Wechsel geht ein POST mit {\"on_air\": true, \"camera\": false, \"apps\": […]} an diese Adresse, z. B. an einen Home-Assistant-Webhook für ein Türschild. Leer = aus.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Vor Terminen blinken", isOn: $model.remindersEnabled)
                ColorPicker("Farbe", selection: colorBinding($model.reminderHex), supportsOpacity: false)
                    .disabled(!model.remindersEnabled)
                LabeledContent("Vorlauf") {
                    Stepper(value: $model.reminderLeadMinutes, in: 1...15, step: 1) {
                        Text("\(Int(model.reminderLeadMinutes)) min")
                    }
                }
                .disabled(!model.remindersEnabled)
                Toggle("Kalender von macOS", isOn: $model.useSystemCalendar)
                Toggle("Nextcloud direkt", isOn: $model.nextcloudEnabled)
                if model.nextcloudEnabled {
                    TextField("Server", text: $model.nextcloudServer, prompt: Text("https://cloud.example.org"))
                    TextField("Benutzer", text: $model.nextcloudUser)
                    SecureField("App-Passwort", text: $model.nextcloudPassword)
                    HStack {
                        Button("Verbindung prüfen") {
                            nextcloudResult = "Prüfe …"
                            Task { nextcloudResult = await model.checkNextcloud() }
                        }
                        Text(nextcloudResult).font(.caption).foregroundStyle(.secondary)
                    }
                }
            } header: {
                Text("Termine")
            } footer: {
                Text("Die Lampe blinkt kurz vor jedem Termin, sobald „Vor Terminen blinken“ an ist, außer ganztägigen und abgesagten. Nicht während eines Calls und nur per USB. Ein Nextcloud-Kalender, der schon in macOS eingebunden ist, braucht „Nextcloud direkt“ nicht. Das App-Passwort legst du in Nextcloud unter Einstellungen › Sicherheit an, MicLED speichert es im Schlüsselbund.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("Schnellfarben im Menü") {
                HStack {
                    ForEach(model.favorites.indices, id: \.self) { i in
                        ColorPicker("", selection: colorBinding($model.favorites[i]), supportsOpacity: false)
                            .labelsHidden()
                    }
                    Spacer()
                    Button("Zurücksetzen") { model.favorites = AppModel.defaultFavorites }
                }
            }

            Section {
                if model.ignored.isEmpty {
                    Text("Keine").foregroundStyle(.secondary)
                }
                ForEach(model.ignored, id: \.self) { key in
                    HStack {
                        Text(key).font(.system(.body, design: .monospaced))
                        Spacer()
                        Button("Entfernen") { model.ignored.removeAll { $0 == key } }
                    }
                }
                DisclosureGroup("Gerade am Mikrofon") {
                    if current.isEmpty {
                        Text("Kein Programm nutzt das Mikrofon.").foregroundStyle(.secondary)
                    }
                    ForEach(current, id: \.pid) { user in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(model.displayName(user))
                                Text(user.bundleId.isEmpty ? user.name : user.bundleId)
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button("Ignorieren") { model.ignore(user) }
                        }
                    }
                    Button("Aktualisieren") { current = model.currentUsers() }
                }
            } header: {
                Text("Ignorierte Programme")
            } footer: {
                Text("Diese Programme schalten die Lampe nicht um, z. B. ein Diktier- oder Aufnahmewerkzeug, das das Mikrofon dauerhaft offen hält.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("Allgemein") {
                Toggle("Lampe aus bei Ruhezustand und Sperre", isOn: $model.offWhenAway)
                Text("Gilt auch, wenn der Bildschirm ausgeht. Während eines Calls bleibt die Mikrofarbe. Danach kommt die vorherige Farbe zurück.")
                    .font(.caption).foregroundStyle(.secondary)
                Toggle("Beim Anmelden starten", isOn: Binding(
                    get: { model.launchAtLogin },
                    set: { model.launchAtLogin = $0 }
                ))
            }
        }
        .formStyle(.grouped)
        .frame(width: 480)
        .frame(minHeight: 640)
        .onAppear { current = model.currentUsers() }
    }
}
