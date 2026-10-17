/// What the agent is told when TinyComputer finds no Chrome. The module looks
/// in its own cache, the standard install locations, and the Puppeteer and
/// Playwright caches, so a Chrome kept anywhere else (`~/Applications`, the
/// Desktop) needs its path set. Left without this, an agent tried to install
/// a browser itself (`npx agent-browser install`, then `sudo`).
pub(crate) const CHROME_NOT_FOUND_HINT: &str = "Chrome was not found. Ask the user to set \
    the full path to the Chrome program under Connections > Computer Control > Browser > \
    Chrome path (for example /Applications/Google Chrome.app/Contents/MacOS/Google Chrome), \
    then try again. Do not install or download a browser yourself.";

/// What the agent is told when the Chrome path the user set names no Chrome
/// the module can start: refused when a task starts, or when the browser
/// launches. Left without it, the agent reads TinyComputer's own field name
/// (`browser_executable`), which is nothing the user can find.
pub(crate) const CHROME_PATH_HINT: &str = "Chrome could not be started from the Chrome \
    path the user set. Ask the user to set Connections > Computer Control > Browser > Chrome \
    path to the Chrome program itself (for example /Applications/Google Chrome.app/Contents/\
    MacOS/Google Chrome), then try again. Do not install or download a browser yourself.";

/// How TinyComputer says no Chrome was found: agent-browser's own words
/// before TinyComputer 0.11, and the module's since.
const CHROME_MISSING: &[&str] = &["chrome not found", "no chrome or chromium was found"];

/// How TinyComputer says the browser binary it was given would not start, or
/// is no executable at all (refused before a task starts).
const CHROME_UNUSABLE: &[&str] = &[
    "browser binary given could not be started",
    "browser_executable must be the absolute path",
];

/// Whether a module error or task failure says Chrome could not be found.
pub(crate) fn chrome_not_found(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    CHROME_MISSING.iter().any(|said| text.contains(said))
}

/// Whether a module error or task failure says the Chrome path the user set
/// names no Chrome the module can start.
pub(crate) fn chrome_path_unusable(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    CHROME_UNUSABLE.iter().any(|said| text.contains(said))
}

/// What the agent is told to ask of the user when `text`, a module error or a
/// task failure, says Chrome is missing or its path is unusable.
pub(crate) fn chrome_hint(text: &str) -> Option<&'static str> {
    if chrome_not_found(text) {
        Some(CHROME_NOT_FOUND_HINT)
    } else if chrome_path_unusable(text) {
        Some(CHROME_PATH_HINT)
    } else {
        None
    }
}

#[derive(Debug, thiserror::Error)]
