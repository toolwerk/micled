import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var updater = UpdaterController.shared
    @State private var current: [MicUser] = []
    @State private var nextcloudResult = ""
    @State private var slackResult = ""
    @State private var pixooResult = ""
    @State private var awtrixResult = ""
    @State private var hueResult = ""
    @State private var goveeResult = ""
    @State private var nanoleafResult = ""
    @State private var robobloqResult = ""
    /// Meldung vom Koppeln je Nanoleaf-Eintrag
    @State private var nanoleafPairResults: [UUID: String] = [:]

    var body: some View {
        @Bindable var model = model

        ScrollViewReader { proxy in
        Form {
            Section("Verbindung zur Lampe") {
                Picker("Ansteuerung", selection: $model.lampKind) {
                    Text("USB direkt").tag(LampKind.usb)
                    Text("Webhook").tag(LampKind.webhook)
                    Text("Keine Lampe").tag(LampKind.noLamp)
                }
                .pickerStyle(.segmented)
                switch model.lampKind {
                case .webhook:
                    TextField("Luxafor-ID", text: $model.luxaforId, prompt: Text("Luxafor-App, Tab „Webhook“"))
                    Text("Die Luxafor-App muss laufen. Sie gibt die Befehle an die Lampe weiter.")
                        .font(.caption).foregroundStyle(.secondary)
                case .usb:
                    Text("Funktioniert ohne Luxafor-App. Läuft die Luxafor-App, kann sie die Farbe überschreiben.")
                        .font(.caption).foregroundStyle(.secondary)
                case .noLamp:
                    Text("MicLED zeigt den Status nur auf dem Display, in Slack und am Webhook.")
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
                Toggle("Slack-Status setzen", isOn: $model.slackEnabled)
                if model.slackEnabled {
                    SecureField("User-Token", text: $model.slackToken, prompt: Text("xoxp-…"))
                    TextField("Statustext", text: $model.slackText)
                    TextField("Emoji", text: $model.slackEmoji, prompt: Text(":red_circle:"))
                    HStack {
                        Button("Verbindung prüfen") {
                            slackResult = "Prüfe …"
                            Task { slackResult = await model.checkSlack() }
                        }
                        Text(slackResult).font(.caption).foregroundStyle(.secondary)
                    }
                }
            } header: {
                Text("Slack")
            } footer: {
                Text("Während du auf Sendung bist, steht der Text in deinem Slack-Profil, danach kommt der vorherige Status zurück. Einen selbst gesetzten Status überschreibt MicLED nicht. Der Token liegt im Schlüsselbund; Anleitung in der README.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Status auf dem Pixoo 64 zeigen", isOn: $model.pixooEnabled)
                if model.pixooEnabled {
                    TextField("Adresse", text: $model.pixooHost, prompt: Text("192.168.1.50"))
                        .autocorrectionDisabled()
                    HStack {
                        Button("Verbindung prüfen") {
                            pixooResult = "Prüfe …"
                            Task { pixooResult = await model.checkPixoo() }
                        }
                        Text(pixooResult).font(.caption).foregroundStyle(.secondary)
                    }
                }
            } header: {
                Text("Pixoo 64")
            } footer: {
                Text("Auf Sendung zeigt das Display ein Mikro- oder Kamerasymbol mit „ON AIR“, beim Timer die Restminuten und bei Fokus „FOKUS“, jeweils in der eingestellten Farbe. Danach läuft wieder, was vorher lief. Feste IP im Router empfohlen.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Status auf der AWTRIX-Uhr zeigen", isOn: $model.awtrixEnabled)
                if model.awtrixEnabled {
                    TextField("Adresse", text: $model.awtrixHost, prompt: Text("192.168.1.60"))
                        .autocorrectionDisabled()
                    HStack {
                        Button("Verbindung prüfen") {
                            awtrixResult = "Prüfe …"
                            Task { awtrixResult = await model.checkAwtrix() }
                        }
                        Text(awtrixResult).font(.caption).foregroundStyle(.secondary)
                    }
                }
            } header: {
                Text("AWTRIX")
            } footer: {
                Text("Auf Sendung zeigt die Uhr „ON AIR“, beim Timer die Restminuten und bei Fokus „FOKUS“, jeweils in der eingestellten Farbe und bis zum Ende. Danach läuft wieder ihre normale Anzeige. Feste IP im Router empfohlen.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Status auf einer Hue-Lampe zeigen", isOn: $model.hueEnabled)
                if model.hueEnabled {
                    HStack {
                        TextField("Bridge", text: $model.hueHost, prompt: Text("192.168.1.20"))
                            .autocorrectionDisabled()
                        Button("Koppeln") {
                            hueResult = "Kopple …"
                            Task { hueResult = await model.pairHue() }
                        }
                        .disabled(model.hueHost.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                    Text(hueResult.isEmpty ? (model.huePaired ? "Gekoppelt ✓" : "Nicht gekoppelt") : hueResult)
                        .font(.caption).foregroundStyle(.secondary)
                    if model.huePaired {
                        Picker("Lampe", selection: $model.hueSelection) {
                            Text("Bitte wählen").tag("")
                            ForEach(model.hueChoices, id: \.self) { t in
                                Text(t.kind == .room ? "Raum: \(t.name)" : t.name)
                                    .tag("\(t.kind == .room ? "room" : "light"):\(t.id)")
                            }
                            // gespeicherte Auswahl, solange die Liste noch nicht geladen ist
                            if !model.hueSelection.isEmpty,
                               !model.hueChoices.contains(where: { $0.id == model.hueTargetId }) {
                                Text(model.hueTargetId).tag(model.hueSelection)
                            }
                        }
                        if let error = model.hueChoicesError {
                            Text(error).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            } header: {
                Text("Philips Hue")
            } footer: {
                Text("Knopf an der Bridge drücken, dann „Koppeln“. Die IP-Adresse steht in der Hue-App unter Einstellungen › Bridges. Bei einem Raum werden alle Lampen darin geschaltet. Danach haben die Lampen wieder Farbe, Helligkeit und An/Aus von vorher. Der Schlüssel liegt im Schlüsselbund.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .onChange(of: model.hueHost) { hueResult = "" }
            .task(id: model.hueEnabled) {
                if model.hueEnabled && model.hueChoices.isEmpty { await model.loadHueTargets() }
            }

            Section {
                Toggle("Status auf Govee-Lampen zeigen", isOn: $model.goveeEnabled)
                if model.goveeEnabled {
                    ForEach($model.goveeDevices) { $light in
                        HStack {
                            lightAddress($light)
                            Button("Entfernen") { model.goveeDevices.removeAll { $0.id == light.id } }
                        }
                    }
                    HStack {
                        Button("Suchen") {
                            goveeResult = "Suche …"
                            Task { goveeResult = await model.searchGovee() }
                        }
                        Button("Adresse hinzufügen") {
                            model.goveeDevices.append(NetLight(id: UUID(), host: "", name: ""))
                        }
                        Text(goveeResult).font(.caption).foregroundStyle(.secondary)
                    }
                    found(model.goveeFound, in: model.goveeDevices, nanoleaf: false)
                }
            } header: {
                Text("Govee").id("network")
            } footer: {
                Text("„LAN Control“ in der Govee-App je Gerät einschalten. Danach haben die Lampen wieder An/Aus, Helligkeit und Farbe von vorher. Feste IP empfohlen.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Status auf Nanoleaf-Lampen zeigen", isOn: $model.nanoleafEnabled)
                if model.nanoleafEnabled {
                    ForEach($model.nanoleafDevices) { $light in
                        HStack {
                            lightAddress($light)
                            Text(nanoleafPairResults[light.id] ?? (model.nanoleafPaired(light.id) ? "Gekoppelt ✓" : "Nicht gekoppelt"))
                                .font(.caption).foregroundStyle(.secondary)
                                .lineLimit(2)
                            Button("Koppeln") {
                                let id = light.id
                                nanoleafPairResults[id] = "Kopple …"
                                Task { nanoleafPairResults[id] = await model.pairNanoleaf(id) }
                            }
                            .disabled(light.host.trimmingCharacters(in: .whitespaces).isEmpty)
                            Button("Entfernen") { model.nanoleafDevices.removeAll { $0.id == light.id } }
                        }
                        .onChange(of: light.host) { nanoleafPairResults[light.id] = nil }
                    }
                    HStack {
                        Button("Suchen") {
                            nanoleafResult = "Suche …"
                            Task { nanoleafResult = await model.searchNanoleaf() }
                        }
                        Button("Adresse hinzufügen") {
                            model.nanoleafDevices.append(NetLight(id: UUID(), host: "", name: ""))
                        }
                        Text(nanoleafResult).font(.caption).foregroundStyle(.secondary)
                    }
                    found(model.nanoleafFound, in: model.nanoleafDevices, nanoleaf: true)
                }
            } header: {
                Text("Nanoleaf")
            } footer: {
                Text("Einschaltknopf 5–7 Sekunden halten, bis die Lichter blinken, dann „Koppeln“. Danach hat die Lampe wieder An/Aus, Helligkeit und Farbe bzw. Effekt von vorher. Der Schlüssel liegt im Schlüsselbund.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Status auf dem Lichtstreifen zeigen", isOn: $model.robobloqEnabled)
                if model.robobloqEnabled {
                    HStack {
                        Button("Testen") {
                            robobloqResult = "Suche …"
                            Task { robobloqResult = await model.checkRobobloq() }
                        }
                        Text(robobloqResult).font(.caption).foregroundStyle(.secondary)
                    }
                }
                Text("Nur ein Programm zur Zeit kann den Controller steuern: DX-Light, SyncLight, backlit usw. vorher beenden.")
                    .font(.caption).foregroundStyle(.secondary)
            } header: {
                Text("Lichtstreifen (USB)").id("strip")
            } footer: {
                Text("Streifen mit Robobloq-Controller (USB 1A86:FE07), z. B. Meewata. Bei Call, Timer und Fokus leuchtet der ganze Streifen in der Farbe, sonst ist er aus. Nach dem Wiederanstecken bekommt er die Farbe nach wenigen Sekunden neu.")
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

            Section {
                if model.appColors.isEmpty {
                    Text("Keine, alle Programme nutzen die Mikrofarbe.").foregroundStyle(.secondary)
                }
                ForEach($model.appColors, id: \.key) { $entry in
                    HStack {
                        ColorPicker(selection: colorBinding($entry.hex), supportsOpacity: false) {
                            Text(entry.key).font(.system(.body, design: .monospaced))
                        }
                        Button("Entfernen") {
                            let key = entry.key
                            model.appColors.removeAll { $0.key == key }
                        }
                    }
                }
                DisclosureGroup("Gerade am Mikrofon") {
                    if current.isEmpty {
                        Text("Kein Programm nutzt das Mikrofon.").foregroundStyle(.secondary)
                    }
                    ForEach(current, id: \.pid) { user in
                        HStack {
                            Text(model.displayName(user))
                            Spacer()
                            Button("Farbe zuweisen") { model.assignColor(user) }
                        }
                    }
                    Button("Aktualisieren") { current = model.currentUsers() }
                }
            } header: {
                Text("Farbe je Programm")
            } footer: {
                Text("Statt der Mikrofarbe, z. B. Orange für eine Aufnahme mit Plaud. Nutzen mehrere Programme das Mikrofon, gilt der oberste Eintrag. Die Kamerafarbe hat Vorrang.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                ColorPicker("Farbe beim Konzentrations-Timer", selection: colorBinding($model.timerHex), supportsOpacity: false)
                Toggle("Fokus von macOS anzeigen", isOn: $model.focusEnabled)
                ColorPicker("Farbe bei Fokus", selection: colorBinding($model.focusHex), supportsOpacity: false)
                    .disabled(!model.focusEnabled)
                if model.focusEnabled && model.focusUnavailable {
                    Text("macOS gibt den Fokus nicht frei. Lege in Kurzbefehle › Automation je Fokus „Fokus an MicLED melden“ an (an und aus).")
                        .font(.caption).foregroundStyle(.orange)
                }
            } header: {
                Text("Konzentration und Fokus")
            } footer: {
                Text("Den Timer startest du im Menü, per Rechtsklick auf das Symbol, mit micled://timer/25 oder in Kurzbefehlen. Am Ende blinkt die Lampe kurz. Ein Call hat Vorrang, eine gewählte Farbe beendet den Timer.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section {
                Toggle("Abends dimmen", isOn: $model.dimEnabled)
                DatePicker("Von", selection: timeBinding($model.dimFromMinutes), displayedComponents: .hourAndMinute)
                    .disabled(!model.dimEnabled)
                DatePicker("Bis", selection: timeBinding($model.dimToMinutes), displayedComponents: .hourAndMinute)
                    .disabled(!model.dimEnabled)
                LabeledContent("Helligkeit") {
                    HStack {
                        Slider(value: $model.dimPercent, in: 10...100, step: 5)
                        Text("\(Int(model.dimPercent)) %").monospacedDigit().frame(width: 44, alignment: .trailing)
                    }
                }
                .disabled(!model.dimEnabled)
            } header: {
                Text("Helligkeit")
            } footer: {
                Text("Die Lampe hat keine eigene Helligkeit, MicLED rechnet die Farben herunter. Gilt für alle Farben. „Bis“ vor „Von“ heißt über Mitternacht.")
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

            Section {
                Toggle("Automatisch nach Updates suchen", isOn: $updater.automaticallyChecksForUpdates)
                    .disabled(!updater.isAvailable)
            } header: {
                Text("Updates")
            } footer: {
                Text(updater.isAvailable
                    ? "Einmal am Tag. Vor der Installation fragt MicLED nach.\nInstalliert: MicLED \(updater.versionText)"
                    : "Entwickler-Builds suchen nicht nach Updates.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onAppear {
            // Screenshot der Abschnitte Govee und Nanoleaf
            if let target = ScreenshotMode.scroll {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { proxy.scrollTo(target, anchor: .top) }
            }
        }
        }
        .frame(width: 480)
        .frame(minHeight: 640)
        .onAppear { current = model.currentUsers() }
    }

    /// Gefundenes Gerät mit Namen und Adresse, von Hand eingetragenes als Eingabefeld
    @ViewBuilder
    private func lightAddress(_ light: Binding<NetLight>) -> some View {
        if light.wrappedValue.name.isEmpty {
            TextField("Adresse", text: light.host, prompt: Text("192.168.1.32"))
                .labelsHidden()
                .autocorrectionDisabled()
        } else {
            Text("\(light.wrappedValue.name) · \(light.wrappedValue.shownHost)")
            Spacer()
        }
    }

    /// Gefundene Geräte, die noch nicht eingetragen sind
    @ViewBuilder
    private func found(_ found: [FoundDevice], in lights: [NetLight], nanoleaf: Bool) -> some View {
        ForEach(found.filter { !NetLight.contains(lights, $0.host, nanoleaf: nanoleaf) }, id: \.host) { f in
            HStack {
                Text("Gefunden: \(f.name) · \(NetLight(id: UUID(), host: f.host, name: f.name).shownHost)")
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Hinzufügen") { model.add(f, toNanoleaf: nanoleaf) }
            }
        }
    }

    /// Minuten nach Mitternacht als Uhrzeit für den `DatePicker`
    private func timeBinding(_ minutes: Binding<Int>) -> Binding<Date> {
        Binding(
            get: {
                let m = minutes.wrappedValue
                return Calendar.current.date(bySettingHour: m / 60, minute: m % 60, second: 0, of: Date()) ?? Date()
            },
            set: {
                let c = Calendar.current.dateComponents([.hour, .minute], from: $0)
                minutes.wrappedValue = (c.hour ?? 0) * 60 + (c.minute ?? 0)
            }
        )
    }
}
