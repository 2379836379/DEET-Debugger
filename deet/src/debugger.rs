use crate::debugger_command::DebuggerCommand;
use crate::dwarf_data::{DwarfData, Error as DwarfError, Function, Location, Variable};
use crate::inferior::{Inferior, Status};
use libc::user_regs_struct;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::collections::HashMap;

pub struct Debugger {
    target: String,
    history_path: String,
    readline: DefaultEditor,
    inferior: Option<Inferior>,
    debug_data: DwarfData,
    breakpoints: HashMap<usize, u8>,
}

impl Debugger {
    pub fn new(target: &str) -> Debugger {
        let debug_data = match DwarfData::from_file(target) {
            Ok(val) => val,
            Err(DwarfError::ErrorOpeningFile) => {
                println!("Could not open file {}", target);
                std::process::exit(1);
            }
            Err(DwarfError::DwarfFormatError(err)) => {
                println!("Could not debugging symbols from {}: {:?}", target, err);
                std::process::exit(1);
            }
        };
        let history_path = match std::env::var("HOME") {
            Ok(home) => format!("{}/.deet_history", home),
            Err(_) => "./.deet_history".to_string(),
        };
        let mut readline = DefaultEditor::new().expect("Error creating line editor");
        let _ = readline.load_history(&history_path);

        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            inferior: None,
            debug_data,
            breakpoints: HashMap::new(),
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.get_next_command() {
                DebuggerCommand::Stepi => self.step_inferior(),
                DebuggerCommand::Print(target) => self.print_target(&target),
                DebuggerCommand::Help => self.print_help(),
                DebuggerCommand::X(target) => self.examine_memory(&target),
                DebuggerCommand::Info(topic) => self.print_info(&topic),
                DebuggerCommand::Jump(dest) => self.jump_to(&dest),
                DebuggerCommand::Delete(bp) => self.delete_breakpoint(&bp),
                DebuggerCommand::Run(args) => self.run_inferior(args),
                DebuggerCommand::Continue => self.continue_inferior(),
                DebuggerCommand::Quit => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().kill();
                        self.inferior = None;
                    }
                    return;
                }
                DebuggerCommand::Backtrace => {
                    if self.inferior.is_none() {
                        println!("Error: you can not use backtrace when there is no process running");
                    } else {
                        self.inferior
                            .as_mut()
                            .unwrap()
                            .print_backtrace(&self.debug_data)
                            .unwrap();
                    }
                }
                DebuggerCommand::Breakpoint(location) => self.set_breakpoint(&location),
            }
        }
    }

    fn run_inferior(&mut self, args: Vec<String>) {
        if self.inferior.is_some() {
            self.inferior.as_mut().unwrap().kill();
            self.inferior = None;
        }
        if let Some(inferior) = Inferior::new(&self.target, &args, &mut self.breakpoints) {
            self.inferior = Some(inferior);
            match self
                .inferior
                .as_mut()
                .unwrap()
                .continue_run(None, &self.breakpoints)
            {
                Ok(status) => self.handle_status(status),
                Err(err) => println!("Error continuing inferior: {}", err),
            }
        } else {
            println!("Error starting subprocess");
        }
    }

    fn continue_inferior(&mut self) {
        if self.inferior.is_none() {
            println!("Error: you can not use continue when there is no process running!");
            return;
        }
        match self
            .inferior
            .as_mut()
            .unwrap()
            .continue_run(None, &self.breakpoints)
        {
            Ok(status) => self.handle_status(status),
            Err(err) => println!("Error continuing inferior: {}", err),
        }
    }

    fn step_inferior(&mut self) {
        if self.inferior.is_none() {
            println!("Error: you can not use stepi when there is no process running!");
            return;
        }
        match self.inferior.as_mut().unwrap().step(&self.breakpoints) {
            Ok(status) => self.handle_status(status),
            Err(err) => println!("Error stepping inferior: {}", err),
        }
    }

    fn print_target(&mut self, target: &str) {
        if self.inferior.is_none() {
            println!("Error: you can not use print when there is no process running!");
            return;
        }
        let inferior = self.inferior.as_ref().unwrap();
        let target = target.trim();
        let reg_name = target.strip_prefix('$').unwrap_or(target);
        let regs = match inferior.get_all_reg() {
            Ok(regs) => regs,
            Err(err) => {
                println!("Error reading registers: {}", err);
                return;
            }
        };

        if let Some(value) = register_value(&regs, reg_name) {
            println!("${} = {:#x}", reg_name, value);
            return;
        }

        let rip = regs.rip as usize;
        let current_function = self.debug_data.get_function_data_from_addr(rip);
        if let Some(var) = self.debug_data.get_local_variable_from_addr(rip, target) {
            self.print_variable_value(inferior, &regs, rip, current_function, target, var);
            return;
        }
        if let Some(var) = self.debug_data.get_global_variable(target) {
            self.print_variable_value(inferior, &regs, rip, None, target, var);
            return;
        }

        if let Some(addr) = self.parse_address(reg_name) {
            match inferior.read_word(addr) {
                Ok(word) => println!("{:#x}: {:#x}", addr, word),
                Err(err) => println!("Error reading memory at {:#x}: {}", addr, err),
            }
            return;
        }

        println!("print currently supports register names, hexadecimal addresses, and known variable names");
    }

    fn print_help(&self) {
        println!("Available commands:");
        println!("  run [args...]          start the target program");
        println!("  continue               continue execution");
        println!("  stepi                  execute one instruction");
        println!("  break <*addr|line|fn>  set a breakpoint");
        println!("  delete <*addr|line|fn> remove a breakpoint");
        println!("  backtrace              print the call stack");
        println!("  print <$reg|0xaddr>    print a register or a word of memory");
        println!("  x <*addr|line|fn>      examine memory at an address");
        println!("  info breakpoints       list breakpoints");
        println!("  info registers         list general-purpose registers");
        println!("  info locals            list locals in the current function");
        println!("  jump <*addr|line|fn>   set RIP without resuming execution");
        println!("  help                   show this message");
        println!("  quit                   exit deet");
    }

    fn examine_memory(&mut self, target: &str) {
        if self.inferior.is_none() {
            println!("Error: you can not use x when there is no process running!");
            return;
        }
        let address = match self.resolve_location(target) {
            Ok(address) => address,
            Err(err) => {
                println!("{}", err);
                return;
            }
        };
        match self.inferior.as_ref().unwrap().read_word(address) {
            Ok(word) => println!("{:#x}: {:#x}", address, word),
            Err(err) => println!("Error reading memory at {:#x}: {}", address, err),
        }
    }

    fn print_info(&mut self, topic: &str) {
        match topic {
            "b" | "break" | "breakpoint" | "breakpoints" => self.print_breakpoints(),
            "r" | "reg" | "register" | "registers" => self.print_registers(),
            "l" | "local" | "locals" => self.print_locals(),
            _ => println!("Usage: info breakpoints|registers|locals"),
        }
    }

    fn jump_to(&mut self, dest: &str) {
        if self.inferior.is_none() {
            println!("Error: you can not use jump when there is no process running!");
            return;
        }
        let address = match self.resolve_location(dest) {
            Ok(address) => address,
            Err(err) => {
                println!("{}", err);
                return;
            }
        };
        match self.inferior.as_mut().unwrap().set_rip(address) {
            Ok(()) => {
                println!("Set instruction pointer to {:#x}", address);
                self.print_stop_location(address);
            }
            Err(err) => println!("Error updating instruction pointer: {}", err),
        }
    }

    fn delete_breakpoint(&mut self, bp: &str) {
        let address = match self.resolve_breakpoint(bp) {
            Ok(address) => address,
            Err(err) => {
                println!("{}", err);
                return;
            }
        };
        let original_byte = match self.breakpoints.remove(&address) {
            Some(byte) => byte,
            None => {
                println!("No breakpoint set at {:#x}", address);
                return;
            }
        };

        if let Some(inferior) = self.inferior.as_mut() {
            if let Err(err) = inferior.write_byte(address, original_byte) {
                println!("Warning: failed to restore instruction at {:#x}: {}", address, err);
            }
        }
        println!("Deleted breakpoint at {:#x}", address);
    }

    fn set_breakpoint(&mut self, location: &str) {
        let breakpoint_addr = match self.resolve_location(location) {
            Ok(address) => address,
            Err(err) => {
                println!("{}", err);
                return;
            }
        };

        if self.breakpoints.contains_key(&breakpoint_addr) {
            println!("Breakpoint already set at {:#x}", breakpoint_addr);
            return;
        }

        if let Some(inferior) = self.inferior.as_mut() {
            match inferior.write_byte(breakpoint_addr, 0xcc) {
                Ok(instruction) => {
                    println!(
                        "Set breakpoint {} at {:#x}",
                        self.breakpoints.len(),
                        breakpoint_addr
                    );
                    self.breakpoints.insert(breakpoint_addr, instruction);
                }
                Err(_) => println!("Invalid breakpoint address {:#x}", breakpoint_addr),
            }
        } else {
            println!(
                "Set breakpoint {} at {:#x}",
                self.breakpoints.len(),
                breakpoint_addr
            );
            self.breakpoints.insert(breakpoint_addr, 0);
        }
    }

    fn print_breakpoints(&self) {
        if self.breakpoints.is_empty() {
            println!("No breakpoints set");
            return;
        }

        for (index, address) in self.breakpoint_addresses().iter().enumerate() {
            println!("{}: {:#x} {}", index, address, self.describe_address(*address));
        }
    }

    fn print_registers(&self) {
        if self.inferior.is_none() {
            println!("Error: you can not use info registers when there is no process running!");
            return;
        }
        match self.inferior.as_ref().unwrap().get_all_reg() {
            Ok(regs) => {
                for register in GENERAL_REGISTERS {
                    if let Some(value) = register_value(&regs, register) {
                        println!("{:>8} = {:#018x}", register, value);
                    }
                }
            }
            Err(err) => println!("Error reading registers: {}", err),
        }
    }


    fn print_locals(&self) {
        if self.inferior.is_none() {
            println!("Error: you can not use info locals when there is no process running!");
            return;
        }
        let inferior = self.inferior.as_ref().unwrap();
        let regs = match inferior.get_all_reg() {
            Ok(regs) => regs,
            Err(err) => {
                println!("Error reading registers: {}", err);
                return;
            }
        };
        let rip = regs.rip as usize;
        let Some(function) = self.debug_data.get_function_data_from_addr(rip) else {
            println!("No function context for current instruction");
            return;
        };
        if function.variables.is_empty() {
            println!("No locals in current function");
            return;
        }
        for variable in &function.variables {
            println!(
                "{} = {}",
                variable.name,
                self.describe_variable_value(inferior, &regs, rip, Some(function), variable)
            );
        }
    }

    fn print_variable_value(
        &self,
        inferior: &Inferior,
        regs: &user_regs_struct,
        rip: usize,
        current_function: Option<&Function>,
        name: &str,
        variable: &Variable,
    ) {
        println!(
            "{} = {}",
            name,
            self.describe_variable_value(inferior, regs, rip, current_function, variable)
        );
    }

    fn describe_variable_value(
        &self,
        inferior: &Inferior,
        regs: &user_regs_struct,
        rip: usize,
        current_function: Option<&Function>,
        variable: &Variable,
    ) -> String {
        let size = variable.entity_type.size.max(1).min(8);

        if let Some(value) = self.read_parameter_register(variable, regs, rip, current_function) {
            return format!(
                "{} ({}, from {})",
                format_scalar_value(value, &value.to_ne_bytes()[..size], &variable.entity_type.name),
                variable.entity_type.name,
                parameter_register_label(variable)
            );
        }

        match self.debug_data.resolve_variable_location(rip, variable) {
            Location::Register(register) => {
                if let Some(value) = register_value_by_dwarf_number(regs, *register) {
                    format!(
                        "{} ({}, from {})",
                        format_scalar_value(value, &value.to_ne_bytes()[..size], &variable.entity_type.name),
                        variable.entity_type.name,
                        dwarf_register_name(*register)
                    )
                } else {
                    format!("<unsupported DWARF register {}>", register)
                }
            }
            Location::Address(addr) => {
                self.describe_memory_backed_variable(inferior, &variable.entity_type.name, size, *addr)
            }
            Location::FramePointerOffset(offset) => {
                let address = ((regs.rbp as i128) + (*offset as i128)) as usize;
                self.describe_memory_backed_variable(inferior, &variable.entity_type.name, size, address)
            }
        }
    }

    fn describe_memory_backed_variable(
        &self,
        inferior: &Inferior,
        type_name: &str,
        size: usize,
        address: usize,
    ) -> String {
        match inferior.read_bytes(address, size) {
            Ok(bytes) => {
                let value = decode_unsigned(&bytes);
                format!(
                    "{} ({}, {} bytes, @ {:#x})",
                    format_scalar_value(value, &bytes, type_name),
                    type_name,
                    size,
                    address
                )
            }
            Err(err) => format!("<read error at {:#x}: {}>", address, err),
        }
    }

    fn read_parameter_register(
        &self,
        variable: &Variable,
        regs: &user_regs_struct,
        rip: usize,
        current_function: Option<&Function>,
    ) -> Option<u64> {
        if !variable.is_parameter {
            return None;
        }
        let parameter_index = variable.parameter_index?;
        let function = current_function?;
        if !self.should_use_parameter_registers(rip, function) {
            return None;
        }
        parameter_register_value(regs, parameter_index)
    }

    fn should_use_parameter_registers(&self, rip: usize, function: &Function) -> bool {
        if let Some(line) = self.debug_data.get_line_from_addr(rip) {
            return line.number == function.line_number;
        }
        rip <= function.address.saturating_add(32)
    }

    fn describe_address(&self, address: usize) -> String {
        let func = self.debug_data.get_function_from_addr(address);
        let line = self.debug_data.get_line_from_addr(address);
        match (func, line) {
            (Some(func), Some(line)) => format!("{} ({})", func, line),
            (Some(func), None) => func,
            (None, Some(line)) => format!("{}", line),
            (None, None) => "<unknown>".to_string(),
        }
    }

    fn handle_status(&mut self, status: Status) {
        match status {
            Status::Exited(exit_code) => {
                println!("Child exited (status {})", exit_code);
                self.inferior = None;
            }
            Status::Signaled(signal) => {
                println!("Child exited due to signal {}", signal);
                self.inferior = None;
            }
            Status::Stopped(signal, rip) => {
                println!("Child stopped (signal {})", signal);
                self.print_stop_location(rip);
            }
        }
    }

    fn print_stop_location(&self, rip: usize) {
        let line = self.debug_data.get_line_from_addr(rip);
        let func = self.debug_data.get_function_from_addr(rip);
        match (func, line) {
            (Some(func), Some(line)) => println!("Stopped at {} ({})", func, line),
            (Some(func), None) => println!("Stopped at {} ({:#x})", func, rip),
            (None, Some(line)) => println!("Stopped at {}", line),
            (None, None) => println!("Stopped at {:#x}", rip),
        }
    }

    fn breakpoint_addresses(&self) -> Vec<usize> {
        let mut addresses: Vec<usize> = self.breakpoints.keys().copied().collect();
        addresses.sort_unstable();
        addresses
    }

    fn resolve_breakpoint(&self, bp: &str) -> Result<usize, &'static str> {
        if let Ok(index) = usize::from_str_radix(bp, 10) {
            if let Some(address) = self.breakpoint_addresses().get(index) {
                return Ok(*address);
            }
        }
        self.resolve_location(bp)
    }

    fn resolve_location(&self, location: &str) -> Result<usize, &'static str> {
        if let Some(address) = location
            .strip_prefix('*')
            .and_then(|raw| self.parse_address(raw))
        {
            return Ok(address);
        }
        if let Ok(line) = usize::from_str_radix(location, 10) {
            if let Some(address) = self.debug_data.get_addr_for_line(None, line) {
                return Ok(address);
            }
            return Err("Invalid line number");
        }
        if let Some(address) = self.debug_data.get_addr_for_function(None, location) {
            return Ok(address);
        }
        if let Some(address) = self.parse_address(location) {
            return Ok(address);
        }
        Err("Usage: *address|line|func")
    }

    fn parse_address(&self, addr: &str) -> Option<usize> {
        let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
            &addr[2..]
        } else {
            &addr
        };
        usize::from_str_radix(addr_without_0x, 16).ok()
    }

    fn get_next_command(&mut self) -> DebuggerCommand {
        loop {
            match self.readline.readline("(deet) ") {
                Err(ReadlineError::Interrupted) => {
                    println!("Type \"quit\" to exit");
                }
                Err(ReadlineError::Eof) => {
                    return DebuggerCommand::Quit;
                }
                Err(err) => {
                    panic!("Unexpected I/O error: {:?}", err);
                }
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let _ = self.readline.add_history_entry(line.as_str());
                    if let Err(err) = self.readline.save_history(&self.history_path) {
                        println!(
                            "Warning: failed to save history file at {}: {}",
                            self.history_path, err
                        );
                    }
                    let tokens: Vec<&str> = line.split_whitespace().collect();
                    if let Some(cmd) = DebuggerCommand::from_tokens(&tokens) {
                        return cmd;
                    } else {
                        println!("Unrecognized command.");
                    }
                }
            }
        }
    }
}

const GENERAL_REGISTERS: [&str; 18] = [
    "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "rip", "r8", "r9", "r10",
    "r11", "r12", "r13", "r14", "r15", "eflags",
];

fn register_value(regs: &user_regs_struct, name: &str) -> Option<u64> {
    match name {
        "rax" => Some(regs.rax),
        "rbx" => Some(regs.rbx),
        "rcx" => Some(regs.rcx),
        "rdx" => Some(regs.rdx),
        "rsi" => Some(regs.rsi),
        "rdi" => Some(regs.rdi),
        "rbp" => Some(regs.rbp),
        "rsp" => Some(regs.rsp),
        "rip" => Some(regs.rip),
        "r8" => Some(regs.r8),
        "r9" => Some(regs.r9),
        "r10" => Some(regs.r10),
        "r11" => Some(regs.r11),
        "r12" => Some(regs.r12),
        "r13" => Some(regs.r13),
        "r14" => Some(regs.r14),
        "r15" => Some(regs.r15),
        "eflags" => Some(regs.eflags),
        "orig_rax" => Some(regs.orig_rax),
        "cs" => Some(regs.cs),
        "ss" => Some(regs.ss),
        "ds" => Some(regs.ds),
        "es" => Some(regs.es),
        "fs" => Some(regs.fs),
        "gs" => Some(regs.gs),
        "fs_base" => Some(regs.fs_base),
        "gs_base" => Some(regs.gs_base),
        _ => None,
    }
}


fn decode_unsigned(bytes: &[u8]) -> u64 {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= (*byte as u64) << (index * 8);
    }
    value
}

fn format_scalar_value(value: u64, bytes: &[u8], type_name: &str) -> String {
    let lower = type_name.to_ascii_lowercase();
    if lower.contains("char") && !bytes.is_empty() {
        let ch = bytes[0];
        if ch.is_ascii_graphic() || ch == b' ' {
            return format!("{} '{:?}'", value as i8, ch as char);
        }
        return format!("{}", value as i8);
    }
    if lower.contains("int") || lower.contains("long") || lower.contains("short") {
        let signed = match bytes.len() {
            1 => value as i8 as i64,
            2 => value as i16 as i64,
            4 => value as i32 as i64,
            _ => value as i64,
        };
        return format!("{} ({:#x})", signed, value);
    }
    format!("{:#x}", value)
}


fn parameter_register_value(regs: &user_regs_struct, parameter_index: usize) -> Option<u64> {
    match parameter_index {
        0 => Some(regs.rdi),
        1 => Some(regs.rsi),
        2 => Some(regs.rdx),
        3 => Some(regs.rcx),
        4 => Some(regs.r8),
        5 => Some(regs.r9),
        _ => None,
    }
}

fn parameter_register_label(variable: &Variable) -> &'static str {
    match variable.parameter_index {
        Some(0) => "rdi",
        Some(1) => "rsi",
        Some(2) => "rdx",
        Some(3) => "rcx",
        Some(4) => "r8",
        Some(5) => "r9",
        _ => "parameter register",
    }
}

fn register_value_by_dwarf_number(regs: &user_regs_struct, register: u16) -> Option<u64> {
    match register {
        0 => Some(regs.rax),
        1 => Some(regs.rdx),
        2 => Some(regs.rcx),
        3 => Some(regs.rbx),
        4 => Some(regs.rsi),
        5 => Some(regs.rdi),
        6 => Some(regs.rbp),
        7 => Some(regs.rsp),
        8 => Some(regs.r8),
        9 => Some(regs.r9),
        10 => Some(regs.r10),
        11 => Some(regs.r11),
        12 => Some(regs.r12),
        13 => Some(regs.r13),
        14 => Some(regs.r14),
        15 => Some(regs.r15),
        16 => Some(regs.rip),
        _ => None,
    }
}

fn dwarf_register_name(register: u16) -> &'static str {
    match register {
        0 => "rax",
        1 => "rdx",
        2 => "rcx",
        3 => "rbx",
        4 => "rsi",
        5 => "rdi",
        6 => "rbp",
        7 => "rsp",
        8 => "r8",
        9 => "r9",
        10 => "r10",
        11 => "r11",
        12 => "r12",
        13 => "r13",
        14 => "r14",
        15 => "r15",
        16 => "rip",
        _ => "unknown register",
    }
}
