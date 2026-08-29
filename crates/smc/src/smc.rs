//! The SMC key protocol: the fan control path.
//!
//! STATUS ON APPLE SILICON: unproven. On this M3 Max (`Mac15,8`, macOS 26.5.1)
//! `IOServiceOpen` against `AppleSMC` succeeds but every key read returns
//! `result = 137` when run unprivileged -- including `#KEY`, which always exists.
//! Whether that is a privilege error or a protocol that no longer exists on
//! Apple Silicon is the open question this crate has to answer first.
//!
//! Run `sudo smc-probe` to settle it. See CLAUDE.md, "Spike 0".

use crate::error::{Error, Result};
use crate::ffi::*;
use std::ffi::{c_void, CString};

/// SMC user-client selector: `kSMCHandleYPCEvent`.
const KERNEL_INDEX_SMC: u32 = 2;
/// `data8` selectors within that call.
const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_READ_KEYINFO: u8 = 9;
const SMC_CMD_WRITE_BYTES: u8 = 6;

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct SmcVersion {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct SmcPLimitData {
    version: u16,
    length: u16,
    cpu_plimit: u32,
    gpu_plimit: u32,
    mem_plimit: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct SmcKeyInfoData {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct SmcKeyData {
    key: u32,
    vers: SmcVersion,
    plimit: SmcPLimitData,
    key_info: SmcKeyInfoData,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

/// A four-character SMC key such as `F0Ac`, packed big-endian into a u32.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Key(pub u32);

impl Key {
    pub const fn new(s: &[u8; 4]) -> Self {
        Key(((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32)
    }
    pub fn as_string(&self) -> String {
        let b = self.0.to_be_bytes();
        String::from_utf8_lossy(&b).into_owned()
    }
    /// Per-fan key, e.g. `fan(0, b"Ac")` -> `F0Ac`.
    pub fn fan(index: u8, suffix: &[u8; 2]) -> Self {
        Key(((b'F' as u32) << 24)
            | ((b'0' as u32 + index as u32) << 16)
            | ((suffix[0] as u32) << 8)
            | suffix[1] as u32)
    }
}

/// A decoded SMC value. The SMC is dynamically typed; the type tag comes back
/// with each read and determines the decoding.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `flt ` -- what Apple Silicon uses for fan RPM.
    Float(f32),
    /// `fpe2` -- fixed point, Intel-era fan encoding. Kept because a machine
    /// that answers the legacy protocol may well answer in the legacy encoding.
    Fpe2(f32),
    UInt(u64),
    Bytes(Vec<u8>),
}

impl Value {
    /// Numeric view, whatever the wire encoding was.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Float(f) | Value::Fpe2(f) => Some(*f),
            Value::UInt(u) => Some(*u as f32),
            Value::Bytes(_) => None,
        }
    }
}

fn decode(type_tag: u32, size: u32, bytes: &[u8; 32]) -> Value {
    let n = (size as usize).min(32);
    let raw = &bytes[..n];
    match (&type_tag.to_be_bytes(), n) {
        (b"flt ", 4) => Value::Float(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
        (b"fpe2", 2) => Value::Fpe2((((raw[0] as u16) << 6) | ((raw[1] as u16) >> 2)) as f32),
        (b"ui8 ", _) | (b"ui16", _) | (b"ui32", _) | (b"ui64", _) => {
            let mut v: u64 = 0;
            for b in raw {
                v = (v << 8) | *b as u64;
            }
            Value::UInt(v)
        }
        _ => Value::Bytes(raw.to_vec()),
    }
}

fn encode(type_tag: u32, value: f32, size: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    match (&type_tag.to_be_bytes(), size) {
        (b"flt ", 4) => out[..4].copy_from_slice(&value.to_le_bytes()),
        (b"fpe2", 2) => {
            let v = (value as u16) << 2;
            out[0] = (v >> 8) as u8;
            out[1] = (v & 0xff) as u8;
        }
        _ => {
            let v = value as u64;
            let n = (size as usize).min(8);
            for (i, slot) in out.iter_mut().enumerate().take(n) {
                *slot = (v >> (8 * (n - 1 - i))) as u8;
            }
        }
    }
    out
}

/// An open connection to the SMC user client.
pub struct Smc {
    conn: IoObject,
}

impl Smc {
    pub fn open() -> Result<Self> {
        // SAFETY: IOServiceMatching takes a NUL-terminated C string and returns
        // a dictionary that IOServiceGetMatchingService consumes.
        unsafe {
            let name = CString::new("AppleSMC").expect("no interior NUL");
            let matching = IOServiceMatching(name.as_ptr());
            if matching.is_null() {
                return Err(Error::ServiceNotFound);
            }
            let service = IOServiceGetMatchingService(K_IO_MAIN_PORT_DEFAULT, matching);
            if service == 0 {
                return Err(Error::ServiceNotFound);
            }
            let mut conn: IoObject = 0;
            let kr = IOServiceOpen(service, mach_task_self_, 0, &mut conn);
            IOObjectRelease(service);
            if kr != KERN_SUCCESS {
                return Err(Error::Kernel(kr));
            }
            Ok(Smc { conn })
        }
    }

    fn call(&self, input: &SmcKeyData) -> Result<SmcKeyData> {
        let mut output = SmcKeyData::default();
        let mut out_size = std::mem::size_of::<SmcKeyData>();
        // SAFETY: both structs are #[repr(C)] and exactly the 80 bytes the
        // kernel expects; sizes are passed explicitly.
        let kr = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                KERNEL_INDEX_SMC,
                input as *const _ as *const c_void,
                std::mem::size_of::<SmcKeyData>(),
                &mut output as *mut _ as *mut c_void,
                &mut out_size,
            )
        };
        if kr != KERN_SUCCESS {
            return Err(Error::Kernel(kr));
        }
        if output.result != 0 {
            return Err(Error::Smc {
                code: output.result,
            });
        }
        Ok(output)
    }

    fn key_info(&self, key: Key) -> Result<(u32, u32)> {
        let input = SmcKeyData {
            key: key.0,
            data8: SMC_CMD_READ_KEYINFO,
            ..Default::default()
        };
        let out = self.call(&input)?;
        Ok((out.key_info.data_type, out.key_info.data_size))
    }

    pub fn read(&self, key: Key) -> Result<Value> {
        let (type_tag, size) = self.key_info(key)?;
        let mut input = SmcKeyData {
            key: key.0,
            data8: SMC_CMD_READ_BYTES,
            ..Default::default()
        };
        input.key_info.data_size = size;
        let out = self.call(&input)?;
        Ok(decode(type_tag, size, &out.bytes))
    }

    /// Write a numeric value. Requires root.
    pub fn write(&self, key: Key, value: f32) -> Result<()> {
        let (type_tag, size) = self.key_info(key)?;
        let mut input = SmcKeyData {
            key: key.0,
            data8: SMC_CMD_WRITE_BYTES,
            bytes: encode(type_tag, value, size),
            ..Default::default()
        };
        input.key_info.data_size = size;
        self.call(&input)?;
        Ok(())
    }

    pub fn fan_count(&self) -> Result<u8> {
        match self.read(Key::new(b"FNum"))? {
            Value::UInt(n) => Ok(n as u8),
            v => v
                .as_f32()
                .map(|f| f as u8)
                .ok_or(Error::UnexpectedType { key: "FNum" }),
        }
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        // NOTE: this is a best-effort close of the mach port only. It is NOT
        // the mechanism that returns fans to auto -- Drop does not run on
        // SIGKILL. See fand's restore path.
        if self.conn != 0 {
            // SAFETY: conn came from a successful IOServiceOpen.
            unsafe { IOServiceClose(self.conn) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_is_the_80_bytes_the_kernel_expects() {
        // Verified against the C definition on macOS 26.5.1. If this changes,
        // every SMC call silently corrupts.
        assert_eq!(std::mem::size_of::<SmcKeyData>(), 80);
    }

    #[test]
    fn key_packs_big_endian() {
        assert_eq!(Key::new(b"FNum").as_string(), "FNum");
        assert_eq!(Key::new(b"F0Ac").0, 0x4630_4163);
    }

    #[test]
    fn fan_keys_are_indexed() {
        assert_eq!(Key::fan(0, b"Ac").as_string(), "F0Ac");
        assert_eq!(Key::fan(1, b"Tg").as_string(), "F1Tg");
        assert_eq!(Key::fan(2, b"Md").as_string(), "F2Md");
    }

    #[test]
    fn float_roundtrips() {
        let tag = u32::from_be_bytes(*b"flt ");
        let enc = encode(tag, 2400.0, 4);
        assert_eq!(decode(tag, 4, &enc), Value::Float(2400.0));
    }

    #[test]
    fn fpe2_roundtrips() {
        let tag = u32::from_be_bytes(*b"fpe2");
        let enc = encode(tag, 1200.0, 2);
        assert_eq!(decode(tag, 2, &enc), Value::Fpe2(1200.0));
    }

    #[test]
    fn uint_decodes_big_endian() {
        let tag = u32::from_be_bytes(*b"ui16");
        let mut b = [0u8; 32];
        b[0] = 0x01;
        b[1] = 0x02;
        assert_eq!(decode(tag, 2, &b), Value::UInt(0x0102));
    }
}
