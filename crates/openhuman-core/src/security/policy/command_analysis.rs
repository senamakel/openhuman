use super::command_vocabulary::{
    classify_segment_source, effective_command, has_dangerous_env_assignment, is_assignment,
    is_command_executor, normalized_command_name, shell_words,
};
use crate::security::policy::CommandClass;

/// Host-owned interpretation of TinyBox's structural shell facts.
///
/// TinyBox reports live command segments and redirection only. OpenHuman adds
/// the policy fields below locally so command permission and autonomy remain
/// host decisions instead of bus-contract vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CommandAnalysis {
    pub segments: Vec<CommandSegment>,
    pub hidden_execution: bool,
    pub redirection: bool,
    pub expansion: bool,
    pub tee: bool,
    pub background: bool,
    pub literal_words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CommandSegment {
    pub source: String,
    pub command: String,
    pub basename: String,
    pub normalized_name: String,
    pub arguments: Vec<String>,
    pub class: CommandClass,
    pub leading_env_assignment: bool,
    pub dangerous_env_prefix: bool,
    pub executor: bool,
}

impl CommandAnalysis {
    pub(crate) fn from_shell_analysis(facts: tinybox_bus::ShellAnalysis) -> Self {
        let mut analysis = Self {
            segments: Vec::new(),
            hidden_execution: false,
            redirection: facts.redirection,
            expansion: false,
            tee: false,
            background: false,
            literal_words: Vec::new(),
        };

        for source in facts.segments {
            let words = shell_words(&source);
            let Some(raw_command) = words.first().copied() else {
                continue;
            };
            let command = raw_command.trim_matches(|c: char| matches!(c, '\'' | '"'));
            let basename = command.rsplit(['/', '\\']).next().unwrap_or(command);
            let normalized_name = normalized_command_name(raw_command);
            let arguments = words
                .iter()
                .skip(1)
                .map(|word| {
                    word.trim_matches(|c: char| matches!(c, '\'' | '"'))
                        .to_owned()
                })
                .collect::<Vec<_>>();
            let lower = source.to_ascii_lowercase();
            let leading_env_assignment = is_assignment(raw_command);
            let dangerous_env_prefix = has_dangerous_env_assignment(&words);
            let (effective_name, _) = effective_command(&words);
            let executor =
                is_command_executor(&normalized_name) || is_command_executor(&effective_name);
            let class = classify_segment_source(&source);
            analysis.hidden_execution |= lower.contains("$(")
                || lower.contains("<(")
                || lower.contains(">(")
                || source.contains('`')
                || has_background_operator(&source);
            analysis.expansion |= source.contains('$') || source.contains('`');
            analysis.background |= has_background_operator(&source);
            analysis.tee |= words
                .iter()
                .any(|word| word == &"tee" || word.ends_with("/tee"));
            // Split option values out (`--file=/p`, `if=/p`, `-d@/p`) so the
            // protected-path floor sees the path inside them.
            analysis.literal_words.extend(
                source
                    .split(|c: char| {
                        c.is_whitespace()
                            || matches!(
                                c,
                                '\'' | '"' | '(' | ')' | '[' | ']' | ',' | ';' | '=' | '@'
                            )
                    })
                    .map(|word| word.trim_matches(['<', '>', '&', '|']).to_owned())
                    .filter(|word| !word.is_empty()),
            );
            analysis.segments.push(CommandSegment {
                source: source.clone(),
                command: source.clone(),
                basename: basename.to_owned(),
                normalized_name,
                arguments,
                class,
                leading_env_assignment,
                dangerous_env_prefix,
                executor,
            });
        }
        analysis
    }
}

fn has_background_operator(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == b'&'
            && bytes.get(index + 1) != Some(&b'&')
            && (index == 0 || bytes[index - 1] != b'&')
            && (index == 0 || bytes[index - 1] != b'>')
    })
}
