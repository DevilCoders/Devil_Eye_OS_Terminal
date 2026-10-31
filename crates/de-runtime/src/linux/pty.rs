//! Linux PTY master/slave allocation and initial terminal configuration.
//!
//! The pair owns both descriptors. It never transfers or closes a descriptor
//! implicitly; callers may borrow descriptors or request independent CLOEXEC
//! duplicates for a later process-spawn operation.

use std::ffi::CStr;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::TerminalSize;

const PTY_NAME_BUFFER_LENGTH: usize = 4096;

/// An owned Linux PTY master/slave pair.
#[derive(Debug)]
pub struct PtyPair {
    master: OwnedFd,
    slave: OwnedFd,
    slave_name: PathBuf,
    size: TerminalSize,
}

impl PtyPair {
    /// Allocates a new PTY pair and applies initial dimensions and canonical
    /// terminal settings to the slave before returning either descriptor.
    ///
    /// If any step fails, all descriptors acquired so far are closed by RAII.
    pub fn open(size: TerminalSize) -> io::Result<Self> {
        let master_raw = unsafe {
            libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC)
        };
        if master_raw == -1 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: posix_openpt returned a new descriptor owned by this function.
        let master = unsafe { OwnedFd::from_raw_fd(master_raw) };

        if unsafe { libc::grantpt(master.as_raw_fd()) } == -1 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::unlockpt(master.as_raw_fd()) } == -1 {
            return Err(io::Error::last_os_error());
        }

        let mut name_buffer = [0 as libc::c_char; PTY_NAME_BUFFER_LENGTH];
        let name_status = unsafe {
            libc::ptsname_r(
                master.as_raw_fd(),
                name_buffer.as_mut_ptr(),
                name_buffer.len(),
            )
        };
        if name_status != 0 {
            return Err(io::Error::from_raw_os_error(name_status));
        }

        // SAFETY: ptsname_r returned success and guarantees a NUL-terminated path.
        let slave_name_c = unsafe { CStr::from_ptr(name_buffer.as_ptr()) };
        let slave_name = PathBuf::from(std::ffi::OsStr::from_bytes(slave_name_c.to_bytes()));
        let slave_raw = unsafe {
            libc::open(
                slave_name_c.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
            )
        };
        if slave_raw == -1 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: open returned a new descriptor owned by this function.
        let slave = unsafe { OwnedFd::from_raw_fd(slave_raw) };

        configure_default_termios(slave.as_raw_fd())?;
        set_window_size(master.as_raw_fd(), size)?;

        Ok(Self {
            master,
            slave,
            slave_name,
            size,
        })
    }

    /// Borrows the PTY master descriptor.
    pub fn master(&self) -> BorrowedFd<'_> {
        self.master.as_fd()
    }

    /// Borrows the PTY slave descriptor.
    pub fn slave(&self) -> BorrowedFd<'_> {
        self.slave.as_fd()
    }

    /// Returns the allocated slave device path.
    pub fn slave_name(&self) -> &Path {
        &self.slave_name
    }

    /// Returns the dimensions most recently applied to the PTY.
    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    /// Duplicates the master descriptor with close-on-exec enabled.
    pub fn try_clone_master(&self) -> io::Result<OwnedFd> {
        duplicate_cloexec(self.master.as_raw_fd())
    }

    /// Duplicates the slave descriptor with close-on-exec enabled.
    pub fn try_clone_slave(&self) -> io::Result<OwnedFd> {
        duplicate_cloexec(self.slave.as_raw_fd())
    }

    /// Applies new dimensions to the PTY master.
    ///
    /// The cached size changes only after the kernel accepts the resize.
    pub fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
        set_window_size(self.master.as_raw_fd(), size)?;
        self.size = size;
        Ok(())
    }

    /// Reads the effective dimensions from the PTY master.
    pub fn query_size(&self) -> io::Result<TerminalSize> {
        let mut window_size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        if unsafe {
            libc::ioctl(
                self.master.as_raw_fd(),
                libc::TIOCGWINSZ,
                &mut window_size,
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }

        TerminalSize::new(window_size.ws_col, window_size.ws_row).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "kernel returned zero PTY dimensions",
            )
        })
    }

    /// Transfers ownership of both descriptors to the caller.
    pub fn into_parts(self) -> (OwnedFd, OwnedFd) {
        (self.master, self.slave)
    }
}

fn duplicate_cloexec(fd: RawFd) -> io::Result<OwnedFd> {
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate == -1 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: F_DUPFD_CLOEXEC returned a new descriptor owned by the caller.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

fn set_window_size(fd: RawFd, size: TerminalSize) -> io::Result<()> {
    let window_size = libc::winsize {
        ws_row: size.rows(),
        ws_col: size.columns(),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &window_size) } == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn configure_default_termios(fd: RawFd) -> io::Result<()> {
    let mut settings = MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, settings.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: tcgetattr initialized the termios structure on success.
    let mut settings = unsafe { settings.assume_init() };

    settings.c_iflag &= !(libc::IGNCR | libc::INLCR | libc::ISTRIP);
    settings.c_iflag |= libc::ICRNL | libc::IXON;
    settings.c_oflag |= libc::OPOST | libc::ONLCR;
    settings.c_cflag &= !libc::CSIZE;
    settings.c_cflag |= libc::CREAD | libc::CS8;
    settings.c_lflag |=
        libc::ISIG | libc::ICANON | libc::ECHO | libc::ECHOE | libc::ECHOK | libc::IEXTEN;

    settings.c_cc[libc::VINTR] = 3;
    settings.c_cc[libc::VQUIT] = 28;
    settings.c_cc[libc::VERASE] = 127;
    settings.c_cc[libc::VKILL] = 21;
    settings.c_cc[libc::VEOF] = 4;
    settings.c_cc[libc::VSTART] = 17;
    settings.c_cc[libc::VSTOP] = 19;
    settings.c_cc[libc::VSUSP] = 26;
    settings.c_cc[libc::VMIN] = 1;
    settings.c_cc[libc::VTIME] = 0;

    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &settings) } == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PtyPair;
    use crate::TerminalSize;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::sync::Mutex;

    static PTY_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn allocates_tty_descriptors_with_cloexec_and_initial_dimensions() {
        let _guard = PTY_TEST_LOCK.lock().expect("PTY test lock");
        let size = TerminalSize::new(100, 37).expect("valid dimensions");
        let pair = PtyPair::open(size).expect("allocate PTY pair");

        assert!(pair.slave_name().is_absolute());
        assert_eq!(pair.size(), size);
        assert_eq!(pair.query_size().expect("query PTY size"), size);
        assert_eq!(unsafe { libc::isatty(pair.master().as_raw_fd()) }, 1);
        assert_eq!(unsafe { libc::isatty(pair.slave().as_raw_fd()) }, 1);

        let master_flags = unsafe { libc::fcntl(pair.master().as_raw_fd(), libc::F_GETFD) };
        let slave_flags = unsafe { libc::fcntl(pair.slave().as_raw_fd(), libc::F_GETFD) };
        assert_ne!(master_flags & libc::FD_CLOEXEC, 0);
        assert_ne!(slave_flags & libc::FD_CLOEXEC, 0);
    }

    #[test]
    fn applies_canonical_echo_and_signal_termios_defaults() {
        let _guard = PTY_TEST_LOCK.lock().expect("PTY test lock");
        let pair = PtyPair::open(TerminalSize::default()).expect("allocate PTY pair");
        let mut settings = std::mem::MaybeUninit::<libc::termios>::uninit();

        assert_eq!(
            unsafe { libc::tcgetattr(pair.slave().as_raw_fd(), settings.as_mut_ptr()) },
            0
        );
        // SAFETY: tcgetattr initialized the structure on success.
        let settings = unsafe { settings.assume_init() };

        assert_ne!(settings.c_lflag & libc::ICANON, 0);
        assert_ne!(settings.c_lflag & libc::ECHO, 0);
        assert_ne!(settings.c_lflag & libc::ISIG, 0);
        assert_ne!(settings.c_iflag & libc::ICRNL, 0);
        assert_ne!(settings.c_oflag & libc::OPOST, 0);
        assert_eq!(settings.c_cc[libc::VMIN], 1);
        assert_eq!(settings.c_cc[libc::VTIME], 0);
    }

    #[test]
    fn resize_updates_kernel_and_cached_dimensions_atomically() {
        let _guard = PTY_TEST_LOCK.lock().expect("PTY test lock");
        let initial = TerminalSize::new(80, 24).expect("initial dimensions");
        let resized = TerminalSize::new(132, 43).expect("resized dimensions");
        let mut pair = PtyPair::open(initial).expect("allocate PTY pair");

        pair.resize(resized).expect("resize PTY");
        assert_eq!(pair.size(), resized);
        assert_eq!(pair.query_size().expect("query resized PTY"), resized);
    }

    #[test]
    fn cloned_descriptors_have_independent_ownership() {
        let _guard = PTY_TEST_LOCK.lock().expect("PTY test lock");
        let pair = PtyPair::open(TerminalSize::default()).expect("allocate PTY pair");
        let master = pair.try_clone_master().expect("clone master");
        let slave = pair.try_clone_slave().expect("clone slave");

        drop(pair);

        assert_eq!(unsafe { libc::isatty(master.as_raw_fd()) }, 1);
        assert_eq!(unsafe { libc::isatty(slave.as_raw_fd()) }, 1);
    }

    #[test]
    fn dropping_pair_closes_both_owned_descriptors() {
        let _guard = PTY_TEST_LOCK.lock().expect("PTY test lock");
        let pair = PtyPair::open(TerminalSize::default()).expect("allocate PTY pair");
        let master_fd = pair.master().as_raw_fd();
        let slave_fd = pair.slave().as_raw_fd();

        drop(pair);

        assert_eq!(unsafe { libc::fcntl(master_fd, libc::F_GETFD) }, -1);
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EBADF)
        );
        assert_eq!(unsafe { libc::fcntl(slave_fd, libc::F_GETFD) }, -1);
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EBADF)
        );
    }
}
