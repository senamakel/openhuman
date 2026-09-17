//! Embedded Swift source fragment: the clipboard-based paste helper (`pasteText`).
#[cfg(target_os = "macos")]
pub(super) const SWIFT_PASTE: &str = r##"// MARK: - Paste Helper

func pasteText(id: String?, text: String) -> [String: Any] {
    var result: [String: Any] = ["type": "paste", "ok": true, "error": NSNull()]
    if let id = id { result["id"] = id }

    let pb = NSPasteboard.general
    let originalContents = pb.string(forType: .string)

    // Set clipboard to new text
    pb.clearContents()
    pb.setString(text, forType: .string)

    // Brief delay for clipboard to settle
    usleep(10_000) // 10ms

    // Simulate Cmd+V via CGEvent
    guard let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: 0x09, keyDown: true),
          let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: 0x09, keyDown: false) else {
        result["ok"] = false
        result["error"] = "failed to create CGEvent"
        return result
    }
    keyDown.flags = .maskCommand
    keyUp.flags = .maskCommand
    keyDown.post(tap: .cgSessionEventTap)
    usleep(8_000) // 8ms between key down/up
    keyUp.post(tap: .cgSessionEventTap)

    // Restore clipboard after delay
    if let original = originalContents {
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + .milliseconds(250)) {
            let pb = NSPasteboard.general
            pb.clearContents()
            pb.setString(original, forType: .string)
        }
    }

    return result
}

"##;
