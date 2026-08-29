use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No matching IOKit service. Should not happen on real hardware.
    ServiceNotFound,
    /// A kern_return_t other than KERN_SUCCESS.
    Kernel(i32),
    /// The call succeeded but the SMC rejected it. Code 132 is the generic
    /// error, 133 is key-not-found, and 137 is what an unprivileged process
    /// sees on Apple Silicon for every key -- see `Error::is_likely_privilege`.
    Smc {
        code: u8,
    },
    UnexpectedType {
        key: &'static str,
    },
    /// Fan index outside what FNum reported.
    NoSuchFan(u8),
}

impl Error {
    /// Whether this looks like "you are not root" rather than a real fault.
    /// Deliberately a hint, not a claim: 137 has not been confirmed to mean
    /// EPERM on Apple Silicon, it is just the code every key returns as a
    /// normal user on this machine.
    pub fn is_likely_privilege(&self) -> bool {
        matches!(self, Error::Smc { code: 137 })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ServiceNotFound => write!(f, "AppleSMC IOKit service not found"),
            Error::Kernel(kr) => write!(f, "IOKit call failed (kern_return_t = 0x{kr:x})"),
            Error::Smc { code } => {
                write!(f, "SMC rejected the call (result = {code})")?;
                if *code == 137 {
                    write!(f, " -- try again as root")?;
                }
                Ok(())
            }
            Error::UnexpectedType { key } => write!(f, "SMC key {key} had an unexpected type"),
            Error::NoSuchFan(i) => write!(f, "no fan at index {i}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
