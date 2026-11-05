// Gibt die Fenster-ID (für `screencapture -l`) eines Fensters aus.
// Aufruf: swift scripts/window-id.swift <pid> <titel>
// Exit 1: Fenster nicht gefunden, Exit 2: keine Fenster lesbar (Recht „Bildschirmaufnahme“ fehlt)
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count == 3, let pid = Int(args[1]) else {
    FileHandle.standardError.write("Aufruf: window-id.swift <pid> <titel>\n".data(using: .utf8)!)
    exit(64)
}
let all = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
if !all.contains(where: { $0["kCGWindowName"] != nil }) {
    FileHandle.standardError.write("Keine Fenstertitel lesbar: dem Terminal in Systemeinstellungen › Datenschutz › Bildschirmaufnahme das Recht geben.\n".data(using: .utf8)!)
    exit(2)
}
let match = all.first {
    ($0["kCGWindowOwnerPID"] as? Int) == pid
        && ($0["kCGWindowLayer"] as? Int) == 0
        && (($0["kCGWindowName"] as? String) ?? "").contains(args[2])
}
guard let id = match?["kCGWindowNumber"] as? Int else { exit(1) }
print(id)
