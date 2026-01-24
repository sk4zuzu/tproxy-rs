use crate::{
    self as tproxy,
    Result, TProxyError,
    Operation,
};

// ---

use nix::errno::Errno;
use nix::sys::signal::{self};
use nix::unistd::{self, Pid};
use std::env::{self};
use std::fs::{self, OpenOptions, ReadDir};
use std::os::unix::process::CommandExt;
use std::path::Component;
use std::process::{self, Command};

// ---

pub struct DaemonDetector { g: ReadDir }

impl DaemonDetector {
    pub fn new() -> Result<Self> { Ok(Self { g: fs::read_dir("/proc")? }) }
}

impl Iterator for DaemonDetector {
    type Item = (Pid, String);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let p = {
                let Some(v) = self.g.next() else { break None };
                let Ok(v) = v else { continue };
                v.path()
            };
            if !p.is_dir() { continue }

            let mut components = p.components();
            let pid = {
                let Some(Component::Normal(v)) = components.nth(2) else { continue };
                let v = v.to_string_lossy();
                if !v.chars().all(|c| c.is_ascii_digit()) { continue };
                let Ok(v) = v.parse() else { continue };
                Pid::from_raw(v)
            };

            let cmd = {
                let Ok(v) = fs::read_to_string(format!("/proc/{}/cmdline", pid)) else { continue };
                if !v.starts_with(tproxy::TPROXY_PREFIX) { continue };
                v[..(v.len() - 1)].to_string() // r-strip \0
            };

            break Some((pid, cmd))
        }
    }
}

// ---

pub struct Daemon { cmd: String }

impl Daemon {
    pub fn new(cmd: String) -> Self { Self { cmd } }

    pub fn run(&self, block: impl Fn() -> Result<()> + 'static) -> Result<()> {
        let arg0 = env::args().nth(0).unwrap();

        // run the closure for the exact match and exit
        if arg0 == self.cmd {
            self.detach()?;
            match block() {
                Ok(_) => process::exit(0),
                Err(e) => { tproxy::log_err(e); process::exit(-1) },
            }
        }

        // ignore other daemons
        if arg0.starts_with(tproxy::TPROXY_PREFIX) {
            return Ok(())
        }

        let mut r = self.detect();

        if let Ok((pid, cmd)) = r {
            match Operation::get()? {
                Operation::Status | Operation::None => {
                    println!("{}: {}", cmd, pid);
                    return Ok(())
                },
                Operation::Reload => {
                    signal::kill(pid, signal::SIGHUP)?;
                    return Ok(())
                },
                Operation::Stop | Operation::Restart => {
                    signal::kill(pid, signal::SIGTERM)?;
                    r = self.detect(); // rerun
                },
                Operation::Start => {
                    return Ok(())
                },
                _ => return Err(TProxyError::InvalidOperation),
            }
        }

        if let Err(TProxyError::NotFound) = r {
            match Operation::get()? {
                Operation::Status | Operation::None => {
                    return Ok(())
                },
                Operation::Reload | Operation::Stop => {
                    return Ok(())
                },
                Operation::Restart | Operation::Start => {
                    // posix_spawn() is used here instead of fork() since modification
                    // of the ARGV array requires unreasonable effort in Rust
                    Command::new(env::current_exe()?)
                        .arg0(self.cmd.clone())
                        .spawn()?;
                    return Ok(())
                },
                _ => return Err(TProxyError::InvalidOperation),
            }
        }

        r.and(Ok(()))
    }

    fn detect(&self) -> Result<(Pid, String)> {
        let mut g = DaemonDetector::new()?;
        loop {
            let Some((pid, cmd)) = g.next() else { break Err(TProxyError::NotFound) };
            if cmd == self.cmd { break Ok((pid, cmd)) }
        }
    }

    fn detach(&self) -> Result<()> {
        unsafe { if libc::setsid() == -1 { return Err(Errno::last().into()) } }

        // redirect STDIN
        let dev_null = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")?;
        unistd::dup2_stdin(&dev_null)?;

        // redirect STDOUT and STDERR
        let log_file = OpenOptions::new()
            .append(true)
            .create(true)
            .read(false)
            .open(format!("{}/{}.log", tproxy::LOG_LOCATION, &self.cmd))?;
        unistd::dup2_stdout(&log_file)?;
        unistd::dup2_stderr(&log_file)?;

        Ok(())
    }
}
