pub enum DebuggerCommand {
    Quit,
    Run(Vec<String>),
    Stepi,
    Continue,
    Backtrace,
    Print(String),
    Breakpoint(String),
    Help,
    X(String),
    Info(String),
    Jump(String),
    Delete(String),
}

impl DebuggerCommand {
    pub fn from_tokens(tokens: &[&str]) -> Option<DebuggerCommand> {
        match tokens[0] {
            "q" | "quit" => Some(DebuggerCommand::Quit),
            "r" | "run" => {
                let args = tokens[1..].iter().map(|s| s.to_string()).collect();
                Some(DebuggerCommand::Run(args))
            }
            "si" | "stepi" => Some(DebuggerCommand::Stepi),
            "c" | "cont" | "continue" => Some(DebuggerCommand::Continue),
            "bt" | "back" | "backtrace" => Some(DebuggerCommand::Backtrace),
            "p" | "print" => tokens
                .get(1)
                .map(|arg| DebuggerCommand::Print((*arg).to_string())),
            "b" | "break" | "breakpoint" => tokens
                .get(1)
                .map(|arg| DebuggerCommand::Breakpoint((*arg).to_string())),
            "h" | "help" => Some(DebuggerCommand::Help),
            "x" => tokens.get(1).map(|arg| DebuggerCommand::X((*arg).to_string())),
            "info" => tokens
                .get(1)
                .map(|arg| DebuggerCommand::Info((*arg).to_string())),
            "j" | "jump" => tokens
                .get(1)
                .map(|arg| DebuggerCommand::Jump((*arg).to_string())),
            "d" | "delete" => tokens
                .get(1)
                .map(|arg| DebuggerCommand::Delete((*arg).to_string())),
            _ => None,
        }
    }
}
