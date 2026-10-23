//! Optional user-mode control-device client for the first-party NDIS miniport.
//!
//! This is only the service-to-driver receive direction (tunnel payload ->
//! `IOCTL_BALANCIFY_INJECT_NBL`). The reverse TX queue/ring is not implemented.

use std::io;

use crate::packet::{flow_key_from_packet, MAX_INNER_PACKET_LEN};

const FILE_DEVICE_NETWORK: u32 = 0x12;
const FILE_WRITE_ACCESS: u32 = 0x2;
const METHOD_BUFFERED: u32 = 0;
const IOCTL_FUNCTION_INJECT_NBL: u32 = 0x802;

const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

/// Control code mirrored from `driver/src/ioctl.c`.
pub const IOCTL_BALANCIFY_INJECT_NBL: u32 = ctl_code(
    FILE_DEVICE_NETWORK,
    IOCTL_FUNCTION_INJECT_NBL,
    METHOD_BUFFERED,
    FILE_WRITE_ACCESS,
);

/// Validate the full packet before making a user-to-kernel transition.
pub fn validate_injection_packet(packet: &[u8]) -> io::Result<()> {
    if packet.is_empty() || packet.len() > MAX_INNER_PACKET_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "inner IP packet is empty or exceeds the 9000-byte limit",
        ));
    }
    flow_key_from_packet(packet)
        .map(|_| ())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: Handle,
        ) -> Handle;
        fn DeviceIoControl(
            device: Handle,
            control_code: u32,
            input_buffer: *mut c_void,
            input_buffer_size: u32,
            output_buffer: *mut c_void,
            output_buffer_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub struct DriverDevice {
        handle: Handle,
    }

    impl DriverDevice {
        pub fn open() -> io::Result<Self> {
            Self::open_path(Path::new(r"\\.\Balancify"))
        }

        fn open_path(path: &Path) -> io::Result<Self> {
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_WRITE,
                    0,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { handle })
        }

        pub fn inject_nbl(&self, packet: &[u8]) -> io::Result<()> {
            validate_injection_packet(packet)?;
            let input_len = u32::try_from(packet.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "packet length overflows Win32 size"))?;
            let mut bytes_returned = 0u32;
            let succeeded = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_BALANCIFY_INJECT_NBL,
                    packet.as_ptr() as *mut c_void,
                    input_len,
                    std::ptr::null_mut(),
                    0,
                    &mut bytes_returned,
                    std::ptr::null_mut(),
                )
            };
            if succeeded == 0 {
                return Err(io::Error::last_os_error());
            }
            if bytes_returned != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "INJECT_NBL unexpectedly returned output bytes",
                ));
            }
            Ok(())
        }
    }

    impl Drop for DriverDevice {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub struct DriverDevice;

    impl DriverDevice {
        pub fn open() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the BalanciFy NDIS control device is available only on Windows",
            ))
        }

        pub fn inject_nbl(&self, packet: &[u8]) -> io::Result<()> {
            validate_injection_packet(packet)?;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the BalanciFy NDIS control device is available only on Windows",
            ))
        }
    }
}

pub use platform::DriverDevice;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_ioctl_matches_the_wdf_control_code_contract() {
        assert_eq!(IOCTL_BALANCIFY_INJECT_NBL, 0x0012_a008);
    }

    #[test]
    fn injection_validation_rejects_bad_or_oversized_packets() {
        assert_eq!(
            validate_injection_packet(&[]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            validate_injection_packet(&[0x45, 0, 0, 20]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            validate_injection_packet(&vec![0; MAX_INNER_PACKET_LEN + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
