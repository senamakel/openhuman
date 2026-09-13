//! Embedded Swift source fragment: AXUIElement app interaction — finding a
//! running app, walking its AX tree, listing elements, pressing controls, and
//! setting text-field values.
#[cfg(target_os = "macos")]
pub(super) const SWIFT_AX_ACTIONS: &str = r##"// MARK: - AXUIElement App Interaction

/// Find a running application by display name (exact, prefix, or contains match).
func findRunningApp(named appName: String) -> NSRunningApplication? {
    let apps = NSWorkspace.shared.runningApplications
    let lower = appName.lowercased()
    if let app = apps.first(where: { $0.localizedName?.lowercased() == lower }) { return app }
    if let app = apps.first(where: { $0.localizedName?.lowercased().hasPrefix(lower) ?? false }) { return app }
    return apps.first(where: { $0.localizedName?.lowercased().contains(lower) ?? false })
}

/// Walk the AX element tree depth-first. Visitor returns true to stop early.
func axWalk(_ element: AXUIElement, depth: Int = 0, maxDepth: Int = 12,
            visitor: (AXUIElement, String, String) -> Bool) -> Bool {
    if depth > maxDepth { return false }
    var roleRef: AnyObject?
    AXUIElementCopyAttributeValue(element, kAXRoleAttribute as CFString, &roleRef)
    let role = (roleRef as? String) ?? ""
    var labelRef: AnyObject?
    AXUIElementCopyAttributeValue(element, kAXTitleAttribute as CFString, &labelRef)
    var label = (labelRef as? String) ?? ""
    if label.isEmpty {
        var descRef: AnyObject?
        AXUIElementCopyAttributeValue(element, kAXDescriptionAttribute as CFString, &descRef)
        label = (descRef as? String) ?? ""
    }
    if visitor(element, role, label) { return true }
    var childrenRef: AnyObject?
    guard AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &childrenRef) == .success,
          let children = childrenRef as? [AXUIElement] else { return false }
    for child in children {
        if axWalk(child, depth: depth + 1, maxDepth: maxDepth, visitor: visitor) { return true }
    }
    return false
}

/// List interactive UI elements in the named app.
func axListElements(appName: String, id: String?) -> [String: Any] {
    guard let app = findRunningApp(named: appName) else {
        return ["type": "ax_list", "id": id ?? "", "ok": false,
                "error": "App '\(appName)' not found or not running", "elements": [] as [Any]]
    }
    let axApp = AXUIElementCreateApplication(app.processIdentifier)
    let interactiveRoles: Set<String> = [
        "AXButton", "AXMenuItem", "AXMenuBarItem", "AXTextField", "AXTextArea",
        "AXCheckBox", "AXRadioButton", "AXSlider", "AXPopUpButton",
        "AXComboBox", "AXLink", "AXTab"
    ]
    var elements: [[String: Any]] = []
    axWalk(axApp, maxDepth: 10) { el, role, label in
        if interactiveRoles.contains(role) && !label.isEmpty {
            elements.append(["role": role, "label": label, "enabled": axEnabled(el)])
        }
        return false
    }
    return ["type": "ax_list", "id": id ?? "", "ok": true, "error": NSNull(), "elements": elements]
}

/// Read the AXEnabled attribute; default to `true` when the attribute is absent
/// (most static/text elements don't expose it, and we don't want to hide them).
func axEnabled(_ element: AXUIElement) -> Bool {
    var ref: AnyObject?
    if AXUIElementCopyAttributeValue(element, kAXEnabledAttribute as CFString, &ref) == .success,
       let b = ref as? Bool {
        return b
    }
    return true
}

/// Collect all AX elements whose label contains `label` (case-insensitive).
/// Returns matches sorted exact-first so "Play" beats "Playlist".
struct AXCandidate {
    var element: AXUIElement
    var label: String
    var exact: Bool
}

func axCollectMatches(_ root: AXUIElement, label: String, depth: Int = 0, maxDepth: Int = 12) -> [AXCandidate] {
    if depth > maxDepth { return [] }
    var roleRef: AnyObject?; AXUIElementCopyAttributeValue(root, kAXRoleAttribute as CFString, &roleRef)
    var titleRef: AnyObject?; AXUIElementCopyAttributeValue(root, kAXTitleAttribute as CFString, &titleRef)
    var elemLabel = (titleRef as? String) ?? ""
    if elemLabel.isEmpty { var d: AnyObject?; AXUIElementCopyAttributeValue(root, kAXDescriptionAttribute as CFString, &d); elemLabel = (d as? String) ?? "" }
    let lower = label.lowercased(); let elLower = elemLabel.lowercased()
    var results: [AXCandidate] = []
    if !elemLabel.isEmpty, elLower.contains(lower) {
        results.append(AXCandidate(element: root, label: elemLabel, exact: elLower == lower))
    }
    var childrenRef: AnyObject?
    guard AXUIElementCopyAttributeValue(root, kAXChildrenAttribute as CFString, &childrenRef) == .success,
          let children = childrenRef as? [AXUIElement] else { return results }
    for child in children { results += axCollectMatches(child, label: label, depth: depth+1, maxDepth: maxDepth) }
    return results
}

/// Press a UI element by label — exact match preferred over contains match.
func axPress(appName: String, label: String, id: String?) -> [String: Any] {
    guard let app = findRunningApp(named: appName) else {
        return ["type": "ax_press", "id": id ?? "", "ok": false,
                "error": "App '\(appName)' not found or not running"]
    }
    let axApp = AXUIElementCreateApplication(app.processIdentifier)
    var matches = axCollectMatches(axApp, label: label)
    // Exact matches first so "Play" beats "Playlist"
    matches.sort { $0.exact && !$1.exact }
    for match in matches {
        if AXUIElementPerformAction(match.element, kAXPressAction as CFString) == .success {
            return ["type": "ax_press", "id": id ?? "", "ok": true, "error": NSNull(),
                    "pressed": match.label]
        }
    }
    return ["type": "ax_press", "id": id ?? "", "ok": false,
            "error": "No pressable element matching '\(label)' found in '\(appName)' (\(matches.count) label match(es) not actionable)"]
}

/// Set the value of a text field by partial label match (or first text field if label is empty).
func axSetValue(appName: String, label: String, value: String, id: String?) -> [String: Any] {
    guard let app = findRunningApp(named: appName) else {
        return ["type": "ax_set_value", "id": id ?? "", "ok": false,
                "error": "App '\(appName)' not found or not running"]
    }
    let axApp = AXUIElementCreateApplication(app.processIdentifier)
    let textRoles: Set<String> = ["AXTextField", "AXTextArea", "AXSearchField", "AXComboBox"]
    let lower = label.lowercased()
    var setLabel = ""
    let found = axWalk(axApp) { element, role, elemLabel in
        guard textRoles.contains(role) else { return false }
        let matchLabel = lower.isEmpty || elemLabel.lowercased().contains(lower)
        guard matchLabel else { return false }
        if AXUIElementSetAttributeValue(element, kAXValueAttribute as CFString, value as CFTypeRef) == .success {
            setLabel = elemLabel
            return true
        }
        return false
    }
    if found {
        return ["type": "ax_set_value", "id": id ?? "", "ok": true, "error": NSNull(),
                "field": setLabel]
    }
    return ["type": "ax_set_value", "id": id ?? "", "ok": false,
            "error": "No text field matching '\(label)' found in '\(appName)'"]
}

"##;
