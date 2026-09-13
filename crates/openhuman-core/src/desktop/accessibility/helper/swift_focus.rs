//! Embedded Swift source fragment: imports, the thread-safe stdout writer, and
//! the accessibility focus-query logic (`queryFocusedElement` and its helpers).
#[cfg(target_os = "macos")]
pub(super) const SWIFT_HEADER_AND_FOCUS: &str = r##"import Cocoa
import Foundation
import ApplicationServices

// MARK: - Thread-safe stdout writer

let stdoutLock = NSLock()

func writeResponse(_ dict: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: dict),
          let line = String(data: data, encoding: .utf8) else { return }
    stdoutLock.lock()
    print(line)
    fflush(stdout)
    stdoutLock.unlock()
}

// MARK: - Accessibility Focus Query

let textRoles: Set<String> = ["AXTextArea", "AXTextField", "AXSearchField", "AXComboBox", "AXEditableText"]

// Apps that need AXEnhancedUserInterface to expose focused text elements properly.
let chromiumAppPatterns = ["chrom", "electron", "code", "slack", "discord", "brave", "edge", "opera", "vivaldi", "arc"]

func isChromiumApp(_ name: String) -> Bool {
    let lower = name.lowercased()
    return chromiumAppPatterns.contains(where: { lower.contains($0) })
}

func getAXStringAttr(_ element: AXUIElement, _ attr: String) -> String? {
    var value: AnyObject?
    let err = AXUIElementCopyAttributeValue(element, attr as CFString, &value)
    guard err == .success, let str = value as? String, str != "missing value" else { return nil }
    return str
}

func getAXPosition(_ element: AXUIElement) -> (x: Int, y: Int)? {
    var value: AnyObject?
    let err = AXUIElementCopyAttributeValue(element, kAXPositionAttribute as String as CFString, &value)
    guard err == .success else { return nil }
    var point = CGPoint.zero
    AXValueGetValue(value as! AXValue, .cgPoint, &point)
    return (Int(point.x), Int(point.y))
}

func getAXSize(_ element: AXUIElement) -> (w: Int, h: Int)? {
    var value: AnyObject?
    let err = AXUIElementCopyAttributeValue(element, kAXSizeAttribute as String as CFString, &value)
    guard err == .success else { return nil }
    var size = CGSize.zero
    AXValueGetValue(value as! AXValue, .cgSize, &size)
    return (Int(size.width), Int(size.height))
}

func scanChildrenForText(_ parent: AXUIElement, depth: Int = 0) -> (role: String, text: String, pos: (Int, Int)?, size: (Int, Int)?)? {
    if depth > 5 { return nil }
    var childrenRef: AnyObject?
    let err = AXUIElementCopyAttributeValue(parent, kAXChildrenAttribute as String as CFString, &childrenRef)
    guard err == .success, let children = childrenRef as? [AXUIElement] else { return nil }

    // First pass: look for text-role elements with content
    for child in children.prefix(200) {
        let role = getAXStringAttr(child, kAXRoleAttribute as String) ?? ""
        if textRoles.contains(role) {
            var text = getAXStringAttr(child, kAXValueAttribute as String) ?? ""
            if text.isEmpty {
                text = getAXStringAttr(child, kAXSelectedTextAttribute as String) ?? ""
            }
            if !text.isEmpty {
                return (role, text, getAXPosition(child), getAXSize(child))
            }
        }
    }

    // Second pass: look for AXStaticText with prompt patterns (terminal support)
    var staticFallback: (role: String, text: String, pos: (Int, Int)?, size: (Int, Int)?)?
    for child in children.prefix(200) {
        let role = getAXStringAttr(child, kAXRoleAttribute as String) ?? ""
        if role == "AXStaticText" {
            let text = getAXStringAttr(child, kAXValueAttribute as String) ?? ""
            if !text.isEmpty {
                if text.contains("$ ") || text.contains("# ") || text.contains("> ") {
                    return (role, text, getAXPosition(child), getAXSize(child))
                }
                if staticFallback == nil {
                    staticFallback = (role, text, getAXPosition(child), getAXSize(child))
                }
            }
        }
    }
    if let fb = staticFallback { return fb }

    // Recurse into children
    for child in children.prefix(50) {
        if let result = scanChildrenForText(child, depth: depth + 1) {
            return result
        }
    }
    return nil
}

func queryFocusedElement(id: String?) -> [String: Any] {
    var result: [String: Any] = [
        "type": "focus",
        "app_name": NSNull(),
        "role": NSNull(),
        "text": "",
        "selected_text": NSNull(),
        "x": NSNull(), "y": NSNull(), "w": NSNull(), "h": NSNull(),
        "error": NSNull(),
        "ax_trusted": AXIsProcessTrusted(),
    ]
    if let id = id { result["id"] = id }

    let systemWide = AXUIElementCreateSystemWide()

    // Get focused application
    var appRef: AnyObject?
    var appErr = AXUIElementCopyAttributeValue(systemWide, kAXFocusedApplicationAttribute as String as CFString, &appRef)
    guard appErr == .success, let appElement = appRef else {
        result["error"] = "ERROR:no_focused_application"
        return result
    }

    let appName = getAXStringAttr(appElement as! AXUIElement, kAXTitleAttribute as String) ?? "unknown"
    result["app_name"] = appName

    // Enable AXEnhancedUserInterface for Chromium apps
    if isChromiumApp(appName) {
        AXUIElementSetAttributeValue(appElement as! AXUIElement, "AXEnhancedUserInterface" as CFString, true as CFBoolean)
    }

    // Get focused element
    var focusedRef: AnyObject?
    let focusErr = AXUIElementCopyAttributeValue(appElement as! AXUIElement, kAXFocusedUIElementAttribute as String as CFString, &focusedRef)

    if focusErr == .success, let focused = focusedRef {
        let focusedElement = focused as! AXUIElement
        let role = getAXStringAttr(focusedElement, kAXRoleAttribute as String) ?? "unknown"
        result["role"] = role

        var text = getAXStringAttr(focusedElement, kAXValueAttribute as String) ?? ""
        let selectedText = getAXStringAttr(focusedElement, kAXSelectedTextAttribute as String)
        result["selected_text"] = selectedText ?? NSNull()

        if text.isEmpty, let sel = selectedText, !sel.isEmpty {
            text = sel
        }
        if text.isEmpty {
            text = getAXStringAttr(focusedElement, kAXTitleAttribute as String) ?? ""
        }

        if let pos = getAXPosition(focusedElement) {
            result["x"] = pos.x
            result["y"] = pos.y
        }
        if let size = getAXSize(focusedElement) {
            result["w"] = size.w
            result["h"] = size.h
        }

        // If we got text from a text-role element, we're done
        if !text.isEmpty && textRoles.contains(role) {
            result["text"] = text
            return result
        }

        // If role is not a text role, still return text if it looks terminal-like
        let terminalApps = ["terminal", "iterm", "wezterm", "warp", "alacritty", "kitty", "ghostty", "hyper", "rio"]
        let isTerminal = terminalApps.contains(where: { appName.lowercased().contains($0) })
        if isTerminal && !text.isEmpty {
            result["text"] = text
            return result
        }

        // Text is empty or not from a text role — scan window children
        if text.isEmpty || !textRoles.contains(role) {
            // Try scanning focused window's children
            var windowRef: AnyObject?
            let winErr = AXUIElementCopyAttributeValue(appElement as! AXUIElement, kAXFocusedWindowAttribute as String as CFString, &windowRef)
            if winErr == .success, let window = windowRef {
                if let found = scanChildrenForText(window as! AXUIElement) {
                    result["role"] = found.role
                    result["text"] = found.text
                    if let pos = found.pos { result["x"] = pos.0; result["y"] = pos.1 }
                    if let size = found.size { result["w"] = size.0; result["h"] = size.1 }
                    return result
                }
            }

            if text.isEmpty {
                result["error"] = "ERROR:no_text_candidate_found"
            } else {
                // Got text but from non-text role and not terminal — still return it
                result["text"] = text
            }
        } else {
            result["text"] = text
        }
    } else {
        // No focused element found — try window scanning
        var windowRef: AnyObject?
        let winErr = AXUIElementCopyAttributeValue(appElement as! AXUIElement, kAXFocusedWindowAttribute as String as CFString, &windowRef)
        if winErr == .success, let window = windowRef {
            if let found = scanChildrenForText(window as! AXUIElement) {
                result["role"] = found.role
                result["text"] = found.text
                if let pos = found.pos { result["x"] = pos.0; result["y"] = pos.1 }
                if let size = found.size { result["w"] = size.0; result["h"] = size.1 }
                return result
            }
        }
        result["error"] = "ERROR:-1728:no_focused_element"
    }

    return result
}

"##;
