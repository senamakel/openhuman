//! Output-language normalization shared by prompt directives.

/// Normalize a configured output language into a display name suitable for
/// prompt directives. Unknown non-empty values are treated as user-provided
/// language names after stripping control characters.
pub fn normalize_output_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        return None;
    }

    let tag = trimmed.to_ascii_lowercase().replace('_', "-");
    let mapped = match tag.as_str() {
        "ar" | "arabic" => Some("Arabic"),
        "bn" | "bengali" | "bangla" => Some("Bengali"),
        "de" | "german" => Some("German"),
        "en" | "en-us" | "en-gb" | "english" => Some("English"),
        "es" | "spanish" => Some("Spanish"),
        "fr" | "french" => Some("French"),
        "hi" | "hindi" => Some("Hindi"),
        "id" | "indonesian" | "bahasa indonesia" => Some("Indonesian"),
        "it" | "italian" => Some("Italian"),
        "ja" | "japanese" => Some("Japanese"),
        "ko" | "korean" => Some("Korean"),
        "pt" | "pt-br" | "pt-pt" | "portuguese" => Some("Portuguese"),
        "ru" | "russian" => Some("Russian"),
        "th" | "thai" => Some("Thai"),
        "tr" | "turkish" => Some("Turkish"),
        "vi" | "vietnamese" => Some("Vietnamese"),
        "zh" | "zh-cn" | "zh-hans" | "chinese" | "simplified chinese" => Some("Simplified Chinese"),
        "zh-tw" | "zh-hant" | "traditional chinese" => Some("Traditional Chinese"),
        _ => None,
    };
    if let Some(language) = mapped {
        return Some(language.to_string());
    }

    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// Build a shared instruction for non-chat background prompts. JSON keys and
/// enum values stay stable; only user-visible prose changes language.
pub fn output_language_directive(language: Option<&str>) -> Option<String> {
    let language = normalize_output_language(language?)?;
    Some(format!(
        "Output language: write all natural-language output in {language}. \
         Keep JSON keys, enum values, proper nouns, code, commands, and quoted source text unchanged."
    ))
}
