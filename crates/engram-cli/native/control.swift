import Foundation
import AppKit
import ApplicationServices
import CoreGraphics
import Carbon.HIToolbox

// Engram control helper: a small, explicit bridge to macOS Accessibility and
// CoreGraphics. It only acts when called with a command; it never listens in
// the background and never runs without an armed takeover session upstream.

func emit(_ value: Any) -> Never {
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
       let text = String(data: data, encoding: .utf8) {
        print(text)
        exit(0)
    }
    print("{\"error\":\"could not encode response\"}")
    exit(1)
}

func fail(_ message: String) -> Never {
    emit(["error": message])
}

func requireTrust() {
    if !AXIsProcessTrusted() {
        fail("Accessibility permission is required. Open System Settings → Privacy & Security → Accessibility and enable Engram (or the terminal running it).")
    }
}

func axValue(_ element: AXUIElement, _ attribute: String) -> Any? {
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
    return error == .success ? value : nil
}

func axString(_ element: AXUIElement, _ attribute: String) -> String? {
    axValue(element, attribute) as? String
}

func axChildren(_ element: AXUIElement) -> [AXUIElement] {
    axValue(element, kAXChildrenAttribute) as? [AXUIElement] ?? []
}

func axPoint(_ value: Any?) -> CGPoint? {
    guard let value, CFGetTypeID(value as CFTypeRef) == AXValueGetTypeID() else { return nil }
    var point = CGPoint.zero
    if AXValueGetValue(value as! AXValue, .cgPoint, &point) { return point }
    return nil
}

func axSize(_ value: Any?) -> CGSize? {
    guard let value, CFGetTypeID(value as CFTypeRef) == AXValueGetTypeID() else { return nil }
    var size = CGSize.zero
    if AXValueGetValue(value as! AXValue, .cgSize, &size) { return size }
    return nil
}

func runningApps() -> [NSRunningApplication] {
    NSWorkspace.shared.runningApplications.filter { $0.activationPolicy == .regular }
}

func findApp(_ query: String) -> NSRunningApplication? {
    let apps = runningApps()
    return apps.first {
        $0.localizedName?.caseInsensitiveCompare(query) == .orderedSame ||
        $0.bundleIdentifier?.caseInsensitiveCompare(query) == .orderedSame
    } ?? apps.first {
        ($0.localizedName ?? "").localizedCaseInsensitiveContains(query) ||
        ($0.bundleIdentifier ?? "").localizedCaseInsensitiveContains(query)
    }
}

func appPID(_ query: String) -> pid_t {
    guard let app = findApp(query) else { fail("application is not running: \(query)") }
    return app.processIdentifier
}

func appElement(_ query: String) -> AXUIElement {
    AXUIElementCreateApplication(appPID(query))
}

func node(_ element: AXUIElement) -> [String: Any] {
    var result: [String: Any] = [
        "role": axString(element, kAXRoleAttribute) ?? "",
        "subrole": axString(element, kAXSubroleAttribute) ?? "",
        "title": axString(element, kAXTitleAttribute) ?? "",
        "description": axString(element, kAXDescriptionAttribute) ?? "",
        "value": axString(element, kAXValueAttribute) ?? "",
        "enabled": axValue(element, kAXEnabledAttribute) as? Bool ?? false
    ]
    if let point = axPoint(axValue(element, kAXPositionAttribute)) {
        result["x"] = Int(point.x)
        result["y"] = Int(point.y)
    }
    if let size = axSize(axValue(element, kAXSizeAttribute)) {
        result["width"] = Int(size.width)
        result["height"] = Int(size.height)
    }
    return result
}

func tree(_ element: AXUIElement, depth: Int, maxDepth: Int, count: inout Int) -> [[String: Any]] {
    if depth > maxDepth || count >= 600 { return [] }
    count += 1
    var result = node(element)
    let children = axChildren(element).flatMap { tree($0, depth: depth + 1, maxDepth: maxDepth, count: &count) }
    if !children.isEmpty { result["children"] = children }
    return [result]
}

func flatten(_ element: AXUIElement, depth: Int, maxDepth: Int, count: inout Int, into: inout [AXUIElement]) {
    if depth > maxDepth || count >= 1200 { return }
    count += 1
    into.append(element)
    for child in axChildren(element) {
        flatten(child, depth: depth + 1, maxDepth: maxDepth, count: &count, into: &into)
    }
}

func matchElement(_ element: AXUIElement, role: String?, title: String?) -> Bool {
    if let role, !role.isEmpty,
       axString(element, kAXRoleAttribute)?.caseInsensitiveCompare(role) != .orderedSame {
        return false
    }
    if let title, !title.isEmpty {
        let haystack = [
            axString(element, kAXTitleAttribute) ?? "",
            axString(element, kAXDescriptionAttribute) ?? "",
            axString(element, kAXValueAttribute) ?? ""
        ].joined(separator: " ").lowercased()
        if !haystack.contains(title.lowercased()) { return false }
    }
    return true
}

func findElements(_ query: String, role: String?, title: String?) -> [AXUIElement] {
    let element = appElement(query)
    var elements: [AXUIElement] = []
    var count = 0
    flatten(element, depth: 0, maxDepth: 20, count: &count, into: &elements)
    return elements.filter { matchElement($0, role: role, title: title) }
}

let keyCodes: [String: CGKeyCode] = [
    "a": CGKeyCode(kVK_ANSI_A), "b": CGKeyCode(kVK_ANSI_B), "c": CGKeyCode(kVK_ANSI_C),
    "d": CGKeyCode(kVK_ANSI_D), "e": CGKeyCode(kVK_ANSI_E), "f": CGKeyCode(kVK_ANSI_F),
    "g": CGKeyCode(kVK_ANSI_G), "h": CGKeyCode(kVK_ANSI_H), "i": CGKeyCode(kVK_ANSI_I),
    "j": CGKeyCode(kVK_ANSI_J), "k": CGKeyCode(kVK_ANSI_K), "l": CGKeyCode(kVK_ANSI_L),
    "m": CGKeyCode(kVK_ANSI_M), "n": CGKeyCode(kVK_ANSI_N), "o": CGKeyCode(kVK_ANSI_O),
    "p": CGKeyCode(kVK_ANSI_P), "q": CGKeyCode(kVK_ANSI_Q), "r": CGKeyCode(kVK_ANSI_R),
    "s": CGKeyCode(kVK_ANSI_S), "t": CGKeyCode(kVK_ANSI_T), "u": CGKeyCode(kVK_ANSI_U),
    "v": CGKeyCode(kVK_ANSI_V), "w": CGKeyCode(kVK_ANSI_W), "x": CGKeyCode(kVK_ANSI_X),
    "y": CGKeyCode(kVK_ANSI_Y), "z": CGKeyCode(kVK_ANSI_Z),
    "0": CGKeyCode(kVK_ANSI_0), "1": CGKeyCode(kVK_ANSI_1), "2": CGKeyCode(kVK_ANSI_2),
    "3": CGKeyCode(kVK_ANSI_3), "4": CGKeyCode(kVK_ANSI_4), "5": CGKeyCode(kVK_ANSI_5),
    "6": CGKeyCode(kVK_ANSI_6), "7": CGKeyCode(kVK_ANSI_7), "8": CGKeyCode(kVK_ANSI_8),
    "9": CGKeyCode(kVK_ANSI_9),
    "return": CGKeyCode(kVK_Return), "enter": CGKeyCode(kVK_Return),
    "tab": CGKeyCode(kVK_Tab), "space": CGKeyCode(kVK_Space),
    "escape": CGKeyCode(kVK_Escape), "esc": CGKeyCode(kVK_Escape),
    "delete": CGKeyCode(kVK_Delete), "backspace": CGKeyCode(kVK_Delete),
    "forwarddelete": CGKeyCode(kVK_ForwardDelete),
    "left": CGKeyCode(kVK_LeftArrow), "right": CGKeyCode(kVK_RightArrow),
    "up": CGKeyCode(kVK_UpArrow), "down": CGKeyCode(kVK_DownArrow),
    "home": CGKeyCode(kVK_Home), "end": CGKeyCode(kVK_End),
    "pageup": CGKeyCode(kVK_PageUp), "pagedown": CGKeyCode(kVK_PageDown),
    "minus": CGKeyCode(kVK_ANSI_Minus), "equal": CGKeyCode(kVK_ANSI_Equal),
    "comma": CGKeyCode(kVK_ANSI_Comma), "period": CGKeyCode(kVK_ANSI_Period),
    "slash": CGKeyCode(kVK_ANSI_Slash), "semicolon": CGKeyCode(kVK_ANSI_Semicolon),
    "quote": CGKeyCode(kVK_ANSI_Quote), "backslash": CGKeyCode(kVK_ANSI_Backslash),
    "grave": CGKeyCode(kVK_ANSI_Grave), "leftbracket": CGKeyCode(kVK_ANSI_LeftBracket),
    "rightbracket": CGKeyCode(kVK_ANSI_RightBracket),
    "f1": CGKeyCode(kVK_F1), "f2": CGKeyCode(kVK_F2), "f3": CGKeyCode(kVK_F3),
    "f4": CGKeyCode(kVK_F4), "f5": CGKeyCode(kVK_F5), "f6": CGKeyCode(kVK_F6),
    "f7": CGKeyCode(kVK_F7), "f8": CGKeyCode(kVK_F8), "f9": CGKeyCode(kVK_F9),
    "f10": CGKeyCode(kVK_F10), "f11": CGKeyCode(kVK_F11), "f12": CGKeyCode(kVK_F12)
]

func modifierFlags(_ names: [String]) -> CGEventFlags {
    var flags: CGEventFlags = []
    for name in names.map({ $0.lowercased() }) {
        switch name {
        case "cmd", "command", "meta": flags.insert(.maskCommand)
        case "shift": flags.insert(.maskShift)
        case "option", "alt": flags.insert(.maskAlternate)
        case "control", "ctrl": flags.insert(.maskControl)
        case "fn": flags.insert(.maskSecondaryFn)
        default: break
        }
    }
    return flags
}

func postKey(_ key: String, modifiers: [String]) {
    guard let code = keyCodes[key.lowercased()] else { fail("unknown key: \(key)") }
    let source = CGEventSource(stateID: .hidSystemState)
    let flags = modifierFlags(modifiers)
    let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true)!
    down.flags = flags
    down.post(tap: .cghidEventTap)
    usleep(18_000)
    let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false)!
    up.flags = flags
    up.post(tap: .cghidEventTap)
}

func typeText(_ text: String) {
    let source = CGEventSource(stateID: .hidSystemState)
    let lines = text.components(separatedBy: "\n")
    for (lineIndex, line) in lines.enumerated() {
        let units = Array(line.utf16)
        var offset = 0
        while offset < units.count {
            let end = min(offset + 20, units.count)
            let chunk = Array(units[offset..<end])
            for isDown in [true, false] {
                let event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: isDown)!
                chunk.withUnsafeBufferPointer { buffer in
                    event.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
                }
                event.post(tap: .cghidEventTap)
            }
            offset = end
            usleep(1_000)
        }
        if lineIndex < lines.count - 1 { postKey("return", modifiers: []) }
    }
}

func mousePoint(_ x: String, _ y: String) -> CGPoint {
    guard let px = Double(x), let py = Double(y) else { fail("mouse coordinates must be numbers") }
    return CGPoint(x: px, y: py)
}

func postMouse(_ type: CGEventType, _ point: CGPoint, _ button: CGMouseButton = .left) {
    let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: button)!
    event.post(tap: .cghidEventTap)
}

let args = CommandLine.arguments
guard args.count >= 2 else {
    fail("usage: engram-control <status|apps|focus|windows|tree|key|type|click|move|scroll|drag|click-ui|set-value|request-trust>")
}

switch args[1] {
case "status":
    let screens = NSScreen.screens.map { screen -> [String: Any] in
        ["x": Int(screen.frame.origin.x), "y": Int(screen.frame.origin.y),
         "width": Int(screen.frame.width), "height": Int(screen.frame.height)]
    }
    let front = NSWorkspace.shared.frontmostApplication
    emit([
        "accessibility": AXIsProcessTrusted(),
        "frontmost": front?.localizedName ?? "",
        "frontmostBundleId": front?.bundleIdentifier ?? "",
        "screens": screens
    ])
case "request-trust":
    let prompt = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
    let options = [prompt: true] as CFDictionary
    let trusted = AXIsProcessTrustedWithOptions(options)
    emit(["accessibility": trusted])
case "apps":
    emit(["apps": runningApps().map {
        ["name": $0.localizedName ?? "", "bundleId": $0.bundleIdentifier ?? "",
         "pid": Int($0.processIdentifier), "active": $0.isActive]
    }])
case "focus":
    guard args.count >= 3 else { fail("usage: focus <app>") }
    guard let app = findApp(args[2]) else { fail("application is not running: \(args[2])") }
    requireTrust()
    if #available(macOS 14.0, *) {
        app.activate(options: [.activateAllWindows])
    } else {
        app.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
    }
    usleep(250_000)
    emit(["focused": app.localizedName ?? args[2], "pid": Int(app.processIdentifier)])
case "windows":
    guard args.count >= 3 else { fail("usage: windows <app>") }
    requireTrust()
    let element = appElement(args[2])
    let windows = axValue(element, kAXWindowsAttribute) as? [AXUIElement] ?? []
    emit(["windows": windows.map { window -> [String: Any] in
        var entry: [String: Any] = ["title": axString(window, kAXTitleAttribute) ?? ""]
        if let point = axPoint(axValue(window, kAXPositionAttribute)) {
            entry["x"] = Int(point.x); entry["y"] = Int(point.y)
        }
        if let size = axSize(axValue(window, kAXSizeAttribute)) {
            entry["width"] = Int(size.width); entry["height"] = Int(size.height)
        }
        return entry
    }])
case "tree":
    guard args.count >= 3 else { fail("usage: tree <app> [maxDepth]") }
    requireTrust()
    let maxDepth = args.count >= 4 ? (Int(args[3]) ?? 6) : 6
    var count = 0
    emit(["app": args[2], "tree": tree(appElement(args[2]), depth: 0, maxDepth: maxDepth, count: &count), "nodes": count])
case "key":
    guard args.count >= 3 else { fail("usage: key <key or cmd+key>") }
    requireTrust()
    let parts = args[2].split(separator: "+").map(String.init)
    guard let key = parts.last else { fail("missing key") }
    postKey(key, modifiers: Array(parts.dropLast()))
    emit(["key": args[2]])
case "type":
    guard args.count >= 3 else { fail("usage: type <text>") }
    requireTrust()
    let text = args.dropFirst(2).joined(separator: " ")
    typeText(text)
    emit(["typed": text.count])
case "click":
    guard args.count >= 4 else { fail("usage: click <x> <y> [left|right|middle]") }
    requireTrust()
    let point = mousePoint(args[2], args[3])
    let buttonName = args.count >= 5 ? args[4].lowercased() : "left"
    var downType: CGEventType = .leftMouseDown
    var upType: CGEventType = .leftMouseUp
    var button: CGMouseButton = .left
    switch buttonName {
    case "right": downType = .rightMouseDown; upType = .rightMouseUp; button = .right
    case "middle": downType = .otherMouseDown; upType = .otherMouseUp; button = .center
    default: break
    }
    postMouse(.mouseMoved, point)
    usleep(40_000)
    postMouse(downType, point, button)
    usleep(60_000)
    postMouse(upType, point, button)
    emit(["clicked": ["x": Int(point.x), "y": Int(point.y), "button": buttonName]])
case "move":
    guard args.count >= 4 else { fail("usage: move <x> <y>") }
    requireTrust()
    let point = mousePoint(args[2], args[3])
    postMouse(.mouseMoved, point)
    emit(["moved": ["x": Int(point.x), "y": Int(point.y)]])
case "scroll":
    guard args.count >= 3 else { fail("usage: scroll <dy> [dx]") }
    requireTrust()
    guard let dy = Int32(args[2]) else { fail("scroll delta must be an integer") }
    let dx = args.count >= 4 ? (Int32(args[3]) ?? 0) : 0
    let event = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 2, wheel1: dy, wheel2: dx, wheel3: 0)!
    event.post(tap: .cghidEventTap)
    emit(["scrolled": ["dy": Int(dy), "dx": Int(dx)]])
case "drag":
    guard args.count >= 6 else { fail("usage: drag <x1> <y1> <x2> <y2>") }
    requireTrust()
    let from = mousePoint(args[2], args[3])
    let to = mousePoint(args[4], args[5])
    postMouse(.mouseMoved, from)
    usleep(40_000)
    postMouse(.leftMouseDown, from)
    usleep(60_000)
    for step in 1...12 {
        let t = Double(step) / 12.0
        let point = CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t)
        postMouse(.leftMouseDragged, point)
        usleep(18_000)
    }
    postMouse(.leftMouseUp, to)
    emit(["dragged": ["from": [Int(from.x), Int(from.y)], "to": [Int(to.x), Int(to.y)]]])
case "click-ui":
    guard args.count >= 3 else { fail("usage: click-ui <app> [role] [title]") }
    requireTrust()
    let app = args[2]
    let role = args.count >= 4 ? args[3] : ""
    let title = args.count >= 5 ? args[4] : ""
    let matches = findElements(app, role: role.isEmpty ? nil : role, title: title.isEmpty ? nil : title)
    guard let element = matches.first else { fail("no matching UI element") }
    var error = AXUIElementPerformAction(element, kAXPressAction as CFString)
    if error != .success {
        error = AXUIElementPerformAction(element, kAXConfirmAction as CFString)
    }
    if error != .success { fail("the UI element did not accept a press action") }
    emit(["pressed": node(element)])
case "set-value":
    guard args.count >= 6 else { fail("usage: set-value <app> <role> <title> <value>") }
    requireTrust()
    let app = args[2]
    let role = args[3]
    let title = args[4]
    let text = args.dropFirst(5).joined(separator: " ")
    let matches = findElements(app, role: role.isEmpty ? nil : role, title: title.isEmpty ? nil : title)
    guard let element = matches.first else { fail("no matching UI element") }
    _ = AXUIElementPerformAction(element, kAXPressAction as CFString)
    let error = AXUIElementSetAttributeValue(element, kAXValueAttribute as CFString, text as CFTypeRef)
    if error != .success { fail("the UI element did not accept a value") }
    emit(["value": text, "role": role, "title": title])
default:
    fail("unknown command: \(args[1])")
}
