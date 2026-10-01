use crate::{WorkerConfig, contract::MAX_LINE_BYTES, unavailable};
use babble_types::Result;
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::process::CommandExt,
    },
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl Worker {
    pub(crate) fn spawn(config: &WorkerConfig, deadline: Instant) -> Result<Self> {
        remaining(deadline)?;
        let mut command = Command::new(&config.executable);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8");
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        if let Some(directory) = &config.working_directory {
            command.current_dir(directory);
        }
        // Only async-signal-safe syscalls run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                for (resource, maximum) in [
                    (libc::RLIMIT_CORE, 0),
                    (libc::RLIMIT_FSIZE, 0),
                    (libc::RLIMIT_NOFILE, 64),
                    #[cfg(target_os = "linux")]
                    (libc::RLIMIT_AS, 512 * 1024 * 1024),
                ] {
                    let mut current = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    if libc::getrlimit(resource, &mut current) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    let limit = libc::rlimit {
                        rlim_cur: maximum.min(current.rlim_max),
                        rlim_max: maximum.min(current.rlim_max),
                    };
                    if libc::setrlimit(resource, &limit) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .map_err(|_| unavailable("could not start worker"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let worker = Self {
            child,
            stdin,
            stdout,
        };
        nonblocking(worker.stdin.as_raw_fd())?;
        nonblocking(worker.stdout.as_raw_fd())?;
        remaining(deadline)?;
        Ok(worker)
    }

    pub(crate) fn exchange(&mut self, request: &[u8], deadline: Instant) -> Result<Vec<u8>> {
        remaining(deadline)?;
        if self
            .child
            .try_wait()
            .map_err(|_| unavailable("worker status failed"))?
            .is_some()
        {
            return Err(unavailable("worker exited"));
        }
        // Any pending byte predates this request and cannot be its response.
        let mut byte = [0];
        match self.stdout.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            _ => return Err(unavailable("unsolicited worker output or exit")),
        }
        let mut offset = 0;
        while offset < request.len() {
            remaining(deadline)?;
            memory_limit(self.child.id())?;
            match self.stdin.write(&request[offset..]) {
                Ok(0) => return Err(unavailable("worker input closed")),
                Ok(count) => offset += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    ready(
                        self.stdin.as_raw_fd(),
                        libc::POLLOUT,
                        deadline,
                        self.child.id(),
                    )?;
                }
                Err(_) => return Err(unavailable("worker write failed")),
            }
        }
        let mut line = Vec::with_capacity(4096);
        let mut buffer = [0; 8192];
        loop {
            remaining(deadline)?;
            memory_limit(self.child.id())?;
            match self.stdout.read(&mut buffer) {
                Ok(0) => return Err(unavailable("worker output truncated")),
                Ok(count) => {
                    if line.len() + count > MAX_LINE_BYTES {
                        return Err(unavailable("worker output too large"));
                    }
                    line.extend_from_slice(&buffer[..count]);
                    if let Some(end) = buffer[..count].iter().position(|byte| *byte == b'\n') {
                        if end + 1 != count {
                            return Err(unavailable("multiple worker responses"));
                        }
                        line.pop();
                        return Ok(line);
                    }
                    if line.len() == MAX_LINE_BYTES {
                        return Err(unavailable("worker output too large"));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    ready(
                        self.stdout.as_raw_fd(),
                        libc::POLLIN,
                        deadline,
                        self.child.id(),
                    )?;
                }
                Err(_) => return Err(unavailable("worker read failed")),
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Kill the entire dedicated group, including children retaining pipe ends.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| unavailable("deadline exceeded"))
}

fn nonblocking(fd: RawFd) -> Result<()> {
    // The descriptors are owned by Worker for the duration of these calls.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(unavailable("worker pipe configuration failed"));
        }
    }
    Ok(())
}

fn ready(fd: RawFd, events: i16, deadline: Instant, pid: u32) -> Result<()> {
    loop {
        memory_limit(pid)?;
        let timeout = remaining(deadline)?.as_millis().saturating_add(1).min(20) as i32;
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result > 0 {
            remaining(deadline)?;
            return Ok(());
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(unavailable("worker pipe polling failed"));
        }
    }
}

fn memory_limit(pid: u32) -> Result<()> {
    // macOS rejects AS/DATA limits for this runtime; measure resident memory instead.
    #[cfg(target_os = "macos")]
    unsafe {
        let mut usage: libc::rusage_info_v0 = std::mem::zeroed();
        if libc::proc_pid_rusage(
            pid as i32,
            libc::RUSAGE_INFO_V0,
            (&mut usage as *mut libc::rusage_info_v0).cast(),
        ) == 0
            && usage.ri_resident_size > 512 * 1024 * 1024
        {
            return Err(unavailable("worker memory limit exceeded"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = pid;
    Ok(())
}
