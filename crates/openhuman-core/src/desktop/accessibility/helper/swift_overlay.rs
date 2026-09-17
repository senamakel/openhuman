//! Embedded Swift source fragment: the on-screen overlay controller and the
//! process's stdin-driven main dispatch loop.
#[cfg(target_os = "macos")]
pub(super) const SWIFT_OVERLAY_AND_MAIN: &str = r##"// MARK: - Overlay Controller

final class OverlayController {
    private var panel: NSPanel?
    private var textField: NSTextField?
    private var hintField: NSTextField?
    private var hideWorkItem: DispatchWorkItem?

    func show(x: CGFloat, yTop: CGFloat, width: CGFloat, height: CGFloat, text: String, ttlMs: Int, tabHint: String) {
        let showTabHint = !tabHint.isEmpty
        // Detect current system appearance for contrast-appropriate colors.
        let isDark: Bool = {
            if #available(macOS 10.14, *) {
                return NSApp.effectiveAppearance
                    .bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            }
            return false
        }()
        let bgColor = isDark
            ? NSColor(white: 0.92, alpha: 0.82)   // light badge on dark background
            : NSColor(white: 0.10, alpha: 0.82)   // dark badge on light background
        let textColor = isDark
            ? NSColor(white: 0.08, alpha: 0.95)
            : NSColor(white: 1.0, alpha: 0.95)

        // Measure badge width from actual text metrics instead of char-count estimate.
        let font = NSFont.systemFont(ofSize: 13)
        let attrs: [NSAttributedString.Key: Any] = [.font: font]
        let measured = (text as NSString).size(withAttributes: attrs)
        let hintPad: CGFloat = showTabHint ? 80 : 16
        let panelWidth = min(480, max(140, ceil(measured.width) + hintPad))
        let panelHeight: CGFloat = 28

        // Multi-monitor: find the screen containing the target or mouse cursor.
        let screen: NSScreen? = {
            let mainHeight = NSScreen.screens.first?.frame.height ?? 900
            if width > 0 && height > 0 {
                let cocoaPoint = NSPoint(x: x + width / 2, y: mainHeight - (yTop + height / 2))
                if let s = NSScreen.screens.first(where: { $0.frame.contains(cocoaPoint) }) {
                    return s
                }
            }
            let mouseLocation = NSEvent.mouseLocation
            if let s = NSScreen.screens.first(where: { $0.frame.contains(mouseLocation) }) {
                return s
            }
            return NSScreen.main ?? NSScreen.screens.first
        }()
        let screenFrame = screen?.frame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let screenHeight = screenFrame.height + screenFrame.origin.y

        var originX: CGFloat
        var originYCocoa: CGFloat

        if width > 0 && height > 0 {
            originX = x + max(8, min(width - panelWidth - 8, 28))
            let originYTop = yTop + max(5, min(height - panelHeight - 4, 10))
            originYCocoa = max(6, screenHeight - originYTop - panelHeight)
        } else {
            let mouseLocation = NSEvent.mouseLocation
            originX = mouseLocation.x + 8
            originYCocoa = mouseLocation.y - panelHeight - 8
        }

        // Clamp to screen bounds
        originX = max(screenFrame.origin.x + 4, min(originX, screenFrame.origin.x + screenFrame.width - panelWidth - 4))
        originYCocoa = max(screenFrame.origin.y + 4, min(originYCocoa, screenFrame.origin.y + screenFrame.height - panelHeight - 4))

        if panel == nil {
            let p = NSPanel(
                contentRect: NSRect(x: originX, y: originYCocoa, width: panelWidth, height: panelHeight),
                styleMask: [.borderless, .nonactivatingPanel],
                backing: .buffered,
                defer: false
            )
            p.level = .statusBar
            p.hasShadow = false
            p.isOpaque = false
            p.backgroundColor = .clear
            p.ignoresMouseEvents = true
            p.collectionBehavior = [.canJoinAllSpaces, .transient]

            let content = NSView(frame: NSRect(x: 0, y: 0, width: panelWidth, height: panelHeight))
            content.wantsLayer = true
            content.layer?.cornerRadius = 6
            content.layer?.backgroundColor = bgColor.cgColor
            p.contentView = content

            let label = NSTextField(labelWithString: text)
            label.frame = NSRect(x: 8, y: 5, width: panelWidth - (showTabHint ? 62 : 16), height: 18)
            label.textColor = textColor
            label.font = font
            label.lineBreakMode = .byTruncatingTail
            content.addSubview(label)

            let hint = NSTextField(labelWithString: tabHint.isEmpty ? "Tab ↵" : tabHint)
            hint.frame = NSRect(x: panelWidth - 54, y: 5, width: 48, height: 18)
            hint.textColor = NSColor(white: isDark ? 0.35 : 0.65, alpha: 1.0)
            hint.font = NSFont.monospacedSystemFont(ofSize: 10, weight: .regular)
            hint.alignment = .right
            hint.isHidden = !showTabHint
            content.addSubview(hint)

            panel = p
            textField = label
            hintField = hint
        }

        // Re-apply colors on every show so runtime appearance changes are reflected.
        panel?.contentView?.layer?.backgroundColor = bgColor.cgColor
        textField?.textColor = textColor
        hintField?.textColor = NSColor(white: isDark ? 0.35 : 0.65, alpha: 1.0)
        hintField?.isHidden = !showTabHint
        if showTabHint {
            hintField?.stringValue = tabHint
        }

        panel?.setFrame(NSRect(x: originX, y: originYCocoa, width: panelWidth, height: panelHeight), display: true)
        panel?.contentView?.frame = NSRect(x: 0, y: 0, width: panelWidth, height: panelHeight)
        textField?.frame = NSRect(x: 8, y: 5, width: panelWidth - (showTabHint ? 62 : 16), height: 18)
        hintField?.frame = NSRect(x: panelWidth - 54, y: 5, width: 48, height: 18)
        textField?.stringValue = text
        panel?.orderFrontRegardless()

        hideWorkItem?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.hide()
        }
        hideWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(max(120, ttlMs)), execute: work)
    }

    func hide() {
        panel?.orderOut(nil)
    }
}

// MARK: - Main Entry Point

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let controller = OverlayController()

DispatchQueue.global(qos: .userInitiated).async {
    while let line = readLine() {
        guard let data = line.data(using: .utf8),
              let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let kind = payload["type"] as? String else {
            continue
        }
        let id = payload["id"] as? String

        switch kind {
        case "focus":
            let response = queryFocusedElement(id: id)
            writeResponse(response)

        case "paste":
            let text = (payload["text"] as? String) ?? ""
            let response = pasteText(id: id, text: text)
            writeResponse(response)

        case "ax_list":
            let appName = (payload["app_name"] as? String) ?? ""
            writeResponse(axListElements(appName: appName, id: id))

        case "ax_press":
            let appName = (payload["app_name"] as? String) ?? ""
            let label = (payload["label"] as? String) ?? ""
            writeResponse(axPress(appName: appName, label: label, id: id))

        case "ax_set_value":
            let appName = (payload["app_name"] as? String) ?? ""
            let label = (payload["label"] as? String) ?? ""
            let value = (payload["value"] as? String) ?? ""
            writeResponse(axSetValue(appName: appName, label: label, value: value, id: id))

        case "show":
            let x = CGFloat((payload["x"] as? NSNumber)?.doubleValue ?? 0)
            let y = CGFloat((payload["y"] as? NSNumber)?.doubleValue ?? 0)
            let w = CGFloat((payload["w"] as? NSNumber)?.doubleValue ?? 0)
            let h = CGFloat((payload["h"] as? NSNumber)?.doubleValue ?? 0)
            let text = (payload["text"] as? String) ?? ""
            let ttl = (payload["ttl_ms"] as? NSNumber)?.intValue ?? 900
            let tabHint: String = {
                if let s = payload["tab_hint"] as? String { return s }
                return "Tab ↵"
            }()
            DispatchQueue.main.async {
                controller.show(x: x, yTop: y, width: w, height: h, text: text, ttlMs: ttl, tabHint: tabHint)
            }

        case "hide":
            DispatchQueue.main.async {
                controller.hide()
            }

        case "quit":
            DispatchQueue.main.async {
                controller.hide()
                NSApplication.shared.terminate(nil)
            }
            return

        default:
            break
        }
    }
}

app.run()
"##;
