//! Normalizing a raw command word down to the basename the classifier keys
//! off, and detecting bases that execute arbitrary code (interpreters,
//! shells, `xargs`, Windows LOLBins, …).

pub(in crate::security::policy) fn command_basename(command: &str) -> &str {
    command.split(['/', '\\']).next_back().unwrap_or(command)
}

pub(in crate::security::policy) fn normalized_command_name(command: &str) -> String {
    let command = command_basename(command).to_ascii_lowercase();
    command
        .strip_suffix(".exe")
        .unwrap_or(command.as_str())
        .to_string()
}

fn is_python_command(command: &str) -> bool {
    let command = normalized_command_name(command);
    command == "python"
        || command == "pythonw"
        || command
            .strip_prefix("pythonw")
            .and_then(|suffix| suffix.chars().next())
            .is_some_and(|ch| ch.is_ascii_digit())
        || command
            .strip_prefix("python")
            .and_then(|suffix| suffix.chars().next())
            .is_some_and(|ch| ch.is_ascii_digit())
}

pub(in crate::security::policy) fn is_command_executor(command: &str) -> bool {
    let command = normalized_command_name(command);
    is_python_command(command.as_str())
        || matches!(
            command.as_str(),
            "xargs"
                | "awk"
                | "gawk"
                | "mawk"
                | "nawk"
                | "perl"
                | "ruby"
                | "bash"
                | "sh"
                | "dash"
                | "zsh"
                | "ksh"
                | "fish"
                | "env"
                // JS/TS runtimes (the `node_exec`/`npm_exec` shell equivalents)
                | "node"
                | "nodejs"
                | "deno"
                | "bun"
                // Windows / PowerShell arbitrary-code launchers + LOLBins
                | "iex"
                | "invoke-expression"
                | "cmd"
                | "pwsh"
                | "powershell"
                | "wscript"
                | "cscript"
                | "mshta"
                | "rundll32"
                | "start-process"
        )
}
