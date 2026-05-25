use crate::dwarf_data::DwarfData;
use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::mem::size_of;
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::Command;

fn align_addr_to_word(addr: usize) -> usize {
    addr & (-(size_of::<usize>() as isize) as usize)
}

pub enum Status {
    Stopped(signal::Signal, usize),
    Exited(i32),
    Signaled(signal::Signal),
}

fn child_traceme() -> Result<(), std::io::Error> {
    ptrace::traceme().or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "ptrace TRACEME failed",
    )))
}

pub struct Inferior {
    child: Child,
}

impl Inferior {
    pub fn new(
        target: &str,
        args: &Vec<String>,
        breakpoints: &mut HashMap<usize, u8>,
    ) -> Option<Inferior> {
        let mut cmd = Command::new(target);
        cmd.args(args);
        unsafe {
            cmd.pre_exec(child_traceme);
        }
        let child = cmd.spawn().ok()?;
        let mut inferior = Inferior { child };
        let bps = breakpoints.clone();
        for bp in bps.keys() {
            match inferior.write_byte(*bp, 0xcc) {
                Ok(ori_instr) => {
                    breakpoints.insert(*bp, ori_instr);
                }
                Err(_) => println!("Invalid breakpoint address {:#x}", bp),
            }
        }
        Some(inferior)
    }

    pub fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.child.id() as i32)
    }

    pub fn wait(&self, options: Option<WaitPidFlag>) -> Result<Status, nix::Error> {
        Ok(match waitpid(self.pid(), options)? {
            WaitStatus::Exited(_pid, exit_code) => Status::Exited(exit_code),
            WaitStatus::Signaled(_pid, signal, _core_dumped) => Status::Signaled(signal),
            WaitStatus::Stopped(_pid, signal) => {
                let regs = ptrace::getregs(self.pid())?;
                Status::Stopped(signal, regs.rip as usize)
            }
            other => panic!("waitpid returned unexpected status: {:?}", other),
        })
    }

    pub fn continue_run(
        &mut self,
        signal: Option<signal::Signal>,
        breakpoints: &HashMap<usize, u8>,
    ) -> Result<Status, nix::Error> {
        let mut regs = ptrace::getregs(self.pid())?;
        let rip = regs.rip as usize;
        if let Some(ori_instr) = breakpoints.get(&(rip - 1)) {
            println!("stopped at a breakpoint");
            self.write_byte(rip - 1, *ori_instr).unwrap();
            regs.rip = (rip - 1) as u64;
            ptrace::setregs(self.pid(), regs).unwrap();
            ptrace::step(self.pid(), None).unwrap();
            match self.wait(None).unwrap() {
                Status::Exited(exit_code) => return Ok(Status::Exited(exit_code)),
                Status::Signaled(signal) => return Ok(Status::Signaled(signal)),
                Status::Stopped(_, _) => {
                    self.write_byte(rip - 1, 0xcc).unwrap();
                }
            }
        }
        ptrace::cont(self.pid(), signal)?;
        self.wait(None)
    }

    pub fn kill(&mut self) {
        self.child.kill().unwrap();
        self.wait(None).unwrap();
        println!("Killing running inferior (pid {})", self.pid())
    }

    pub fn get_all_reg(&self) -> Result<libc::user_regs_struct, nix::Error> {
        ptrace::getregs(self.pid())
    }

    pub fn read_word(&self, addr: usize) -> Result<u64, nix::Error> {
        Ok(ptrace::read(self.pid(), addr as ptrace::AddressType)? as u64)
    }


    pub fn read_bytes(&self, addr: usize, size: usize) -> Result<Vec<u8>, nix::Error> {
        let mut bytes = Vec::with_capacity(size);
        let word_size = size_of::<usize>();
        let mut current = addr;
        while bytes.len() < size {
            let aligned = align_addr_to_word(current);
            let word = self.read_word(aligned)?;
            let raw = word.to_ne_bytes();
            let start = current - aligned;
            let available = std::cmp::min(word_size - start, size - bytes.len());
            bytes.extend_from_slice(&raw[start..start + available]);
            current += available;
        }
        Ok(bytes)
    }

    pub fn set_rip(&mut self, rip: usize) -> Result<(), nix::Error> {
        let mut regs = ptrace::getregs(self.pid())?;
        regs.rip = rip as u64;
        ptrace::setregs(self.pid(), regs)
    }

    pub fn step(&mut self, breakpoints: &HashMap<usize, u8>) -> Result<Status, nix::Error> {
        let mut regs = ptrace::getregs(self.pid())?;
        let rip = regs.rip as usize;
        if let Some(ori_instr) = breakpoints.get(&(rip - 1)) {
            self.write_byte(rip - 1, *ori_instr)?;
            regs.rip = (rip - 1) as u64;
            ptrace::setregs(self.pid(), regs)?;
            ptrace::step(self.pid(), None)?;
            let status = self.wait(None)?;
            if matches!(status, Status::Stopped(_, _)) {
                self.write_byte(rip - 1, 0xcc)?;
            }
            return Ok(status);
        }

        ptrace::step(self.pid(), None)?;
        self.wait(None)
    }

    pub fn print_backtrace(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid())?;
        let mut rip = regs.rip as usize;
        let mut rbp = regs.rbp as usize;
        loop {
            let _line = debug_data.get_line_from_addr(rip);
            let _func = debug_data.get_function_from_addr(rip);
            match (&_line, &_func) {
                (None, None) => println!("unknown func (source file not found)"),
                (Some(line), None) => println!("unknown func ({})", line),
                (None, Some(func)) => println!("{} (source file not found)", func),
                (Some(line), Some(func)) => println!("{} ({})", func, line),
            }
            if let Some(func) = _func {
                if func == "main" {
                    break;
                }
            } else {
                break;
            }
            rip = ptrace::read(self.pid(), (rbp + 8) as ptrace::AddressType)? as usize;
            rbp = ptrace::read(self.pid(), rbp as ptrace::AddressType)? as usize;
        }
        Ok(())
    }

    pub fn write_byte(&mut self, addr: usize, val: u8) -> Result<u8, nix::Error> {
        let aligned_addr = align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((val as u64) << 8 * byte_offset);
        ptrace::write(
            self.pid(),
            aligned_addr as ptrace::AddressType,
            updated_word as libc::c_long,
        )?;
        Ok(orig_byte as u8)
    }
}
