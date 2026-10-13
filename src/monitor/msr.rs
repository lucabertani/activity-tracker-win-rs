//! HWiNFO-style per-core thermal throttling + temperature via the PawnIO
//! kernel driver (namazso) with the signed `IntelMSR` module (MSR
//! `IA32_THERM_STATUS`, 0x19C). Bit 0 is the thermal status flag; bits 22:16
//! are the digital thermal sensor readout on modern Intel (Skylake and later,
//! incl. Core Ultra — same layout HWiNFO and LibreHardwareMonitor read): the
//! offset *below* TjMax, so the absolute temperature is `TjMax - readout`,
//! with TjMax from `IA32_TEMPERATURE_TARGET`.
//! PawnIO must be installed once as administrator (see `driver/README.md`).
//! Its device grants access only to SYSTEM and Administrators (the INF sets
//! `D:P(A;;GA;;;SY)(A;;GA;;;BA)`), so the app relaunches itself elevated when
//! access is denied. Only the module whitelist is exposed (here: two
//! read-only MSRs), not raw MSR/memory.

use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

/// PawnIO device object (`\Device\PawnIO`).
const DEVICE_NAME: &str = r"\\?\GLOBALROOT\Device\PawnIO";
const DEVICE_TYPE: u32 = 41394 << 16;
/// `CTL_CODE(41394, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS)` — PawnIO
/// `IOCTL_PIO_LOAD_BINARY`: input = module binary.
const IOCTL_PIO_LOAD_BINARY: u32 = DEVICE_TYPE | (0x821 << 2);
/// `CTL_CODE(41394, 0x841, METHOD_BUFFERED, FILE_ANY_ACCESS)` — PawnIO
/// `IOCTL_PIO_EXECUTE_FN`: input = function name (32-byte ASCII) + i64 args,
/// output = i64 results.
const IOCTL_PIO_EXECUTE_FN: u32 = DEVICE_TYPE | (0x841 << 2);
/// Execute input starts with the zero-padded ASCII function name.
const FN_NAME_LENGTH: usize = 32;
/// Compiled + signed `IntelMSR` module (LGPL-2.1, namazso/PawnIO.Modules
/// 0.2.11): exposes only a whitelist of MSR reads/writes, incl. the two
/// read-only MSRs used here (`0x19C`, `0x1A2`).
const INTELMSR_MODULE: &[u8] = include_bytes!("../../driver/pawnio/IntelMSR.bin");
const MSR_IA32_THERM_STATUS: u32 = 0x19C;
/// `IA32_TEMPERATURE_TARGET` (0x1A2): bits 23:16 hold the TCC activation
/// temperature (TjMax), the reference point the digital readout is relative to.
const MSR_IA32_TEMPERATURE_TARGET: u32 = 0x1A2;
/// Bit 0 of IA32_THERM_STATUS: the core is currently at/above its thermal
/// trip point, i.e. it is thermal-throttling right now.
const THERM_STATUS_THERMAL: u64 = 1;
/// TjMax assumed when `IA32_TEMPERATURE_TARGET` is unreadable (Intel default).
const DEFAULT_TJMAX_C: u16 = 100;
/// Win32 `ERROR_ACCESS_DENIED`: the device exists but denies this process.
const ERROR_ACCESS_DENIED: i32 = 5;

/// Why the thermal monitor is unavailable.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// PawnIO is not installed, or the `IntelMSR` module cannot load (non-Intel
    /// CPU, unsigned module): fall back to the frequency heuristic.
    Missing,
    /// The device is present but denies this process (admin-only ACL): the app
    /// can relaunch itself elevated.
    AccessDenied,
}

fn open_error(os_error: i32) -> OpenError {
    if os_error == ERROR_ACCESS_DENIED {
        OpenError::AccessDenied
    } else {
        OpenError::Missing
    }
}

/// One thermal sample across all logical processors.
pub struct MsrThermalSample {
    /// True when at least one core is currently thermal-throttling.
    pub throttling: bool,
    /// Per logical processor, in core order. `None` = not readable on that
    /// core (e.g. the MSR is not implemented there).
    pub temps_c: Vec<Option<u16>>,
}

impl MsrThermalSample {
    /// Average of the readable core temperatures in °C. `None` when no core
    /// returned a usable reading.
    pub fn average_celsius(&self) -> Option<f64> {
        let readable: Vec<u16> = self.temps_c.iter().flatten().copied().collect();
        readable
            .first()
            .map(|_| readable.iter().map(|&t| t as u32).sum::<u32>() as f64 / readable.len() as f64)
    }
}

/// Reads MSR `IA32_THERM_STATUS` on every logical processor.
pub struct MsrThermalMonitor {
    handle: HANDLE,
    core_count: usize,
}

// The HANDLE is a kernel object reference usable from any thread and this
// struct owns it exclusively; safe to move the monitor across threads.
unsafe impl Send for MsrThermalMonitor {}

impl MsrThermalMonitor {
    /// Opens the PawnIO device and loads the `IntelMSR` module. Fails with
    /// [`OpenError::Missing`] when PawnIO is not available (the caller then
    /// falls back to the frequency heuristic) and with
    /// [`OpenError::AccessDenied`] when the admin-only device denies this
    /// process.
    pub fn open() -> Result<Self, OpenError> {
        let wide: Vec<u16> = DEVICE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            windows_sys::Win32::Storage::FileSystem::CreateFileW(
                wide.as_ptr(),
                0xC0000000, // GENERIC_READ | GENERIC_WRITE
                3,          // FILE_SHARE_READ | FILE_SHARE_WRITE
                ptr::null_mut(),
                3, // OPEN_EXISTING
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let os_error = std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or_default();
            return Err(open_error(os_error));
        }

        // On AMD CPUs (or with an unsigned module) the load fails and the
        // temperature path is disabled, same as a missing driver.
        let mut returned: u32 = 0;
        let loaded = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_PIO_LOAD_BINARY,
                INTELMSR_MODULE.as_ptr() as *const _,
                INTELMSR_MODULE.len() as u32,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if loaded == 0 {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(OpenError::Missing);
        }

        let mut sys_info = windows_sys::Win32::System::SystemInformation::SYSTEM_INFO::default();
        unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut sys_info);
        }
        let core_count = sys_info.dwNumberOfProcessors as usize;

        Ok(Self { handle, core_count })
    }

    /// Samples every core: throttle flags + per-core temperature in °C.
    pub fn refresh(&self) -> MsrThermalSample {
        let mut throttling = false;
        let mut temps_c = Vec::with_capacity(self.core_count);
        for core in 0..self.core_count {
            let previous = unsafe { SetThreadAffinityMask(GetCurrentThread(), 1usize << core) };
            if previous == 0 {
                // Core does not exist (or affinity is restricted): skip it.
                temps_c.push(None);
                continue;
            }
            let value = Self::read_msr(self.handle, MSR_IA32_THERM_STATUS);
            let tjmax = Self::read_msr(self.handle, MSR_IA32_TEMPERATURE_TARGET)
                .and_then(tjmax_celsius)
                .unwrap_or(DEFAULT_TJMAX_C);
            unsafe {
                SetThreadAffinityMask(GetCurrentThread(), previous);
            }
            let Some(value) = value else {
                temps_c.push(None);
                continue;
            };
            if value & THERM_STATUS_THERMAL != 0 {
                throttling = true;
            }
            temps_c.push(temperature_celsius(value, tjmax));
        }
        MsrThermalSample {
            throttling,
            temps_c,
        }
    }

    fn read_msr(handle: HANDLE, index: u32) -> Option<u64> {
        let input = execute_input("ioctl_read_msr", &[index as i64]);
        let mut value: i64 = 0;
        let mut returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_PIO_EXECUTE_FN,
                input.as_ptr() as *const _,
                input.len() as u32,
                &mut value as *mut i64 as *mut _,
                mem::size_of::<i64>() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok != 0 && returned == mem::size_of::<i64>() as u32 {
            Some(value as u64)
        } else {
            None
        }
    }
}

/// Frames a PawnIO `Execute` call: 32-byte zero-padded ASCII function name,
/// then the arguments as little-endian i64 values.
fn execute_input(name: &str, args: &[i64]) -> Vec<u8> {
    let mut input = vec![0u8; FN_NAME_LENGTH + mem::size_of_val(args)];
    let n = name.len().min(FN_NAME_LENGTH - 1);
    input[..n].copy_from_slice(&name.as_bytes()[..n]);
    for (slot, arg) in input[FN_NAME_LENGTH..]
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(args)
    {
        slot.copy_from_slice(&arg.to_le_bytes());
    }
    input
}

/// Decodes TjMax (TCC activation temperature) from `IA32_TEMPERATURE_TARGET`.
pub(crate) fn tjmax_celsius(temperature_target: u64) -> Option<u16> {
    let raw = ((temperature_target >> 16) & 0xFF) as u16;
    (raw != 0).then_some(raw)
}

/// Decodes the core temperature in °C from `IA32_THERM_STATUS`.
/// Bits 22:16 are the digital readout: the offset *below* TjMax (Intel SDM
/// Vol. 4, MSR 019H), so the absolute temperature is `TjMax - readout`, not
/// the raw field. `None` when the core reports no reading at all (bit 31
/// clear and zero readout — MSR not implemented on some E-cores).
pub(crate) fn temperature_celsius(therm_status: u64, tjmax: u16) -> Option<u16> {
    const READING_VALID: u64 = 1 << 31;
    let readout = ((therm_status >> 16) & 0x7F) as u16;
    if therm_status & READING_VALID == 0 && readout == 0 {
        return None;
    }
    Some(tjmax.saturating_sub(readout))
}

impl Drop for MsrThermalMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MsrThermalSample, OpenError, execute_input, open_error, temperature_celsius, tjmax_celsius,
    };

    #[test]
    fn access_denied_is_distinguished_from_a_missing_driver() {
        assert_eq!(open_error(5), OpenError::AccessDenied);
        assert_eq!(open_error(2), OpenError::Missing);
        assert_eq!(open_error(0), OpenError::Missing);
    }

    #[test]
    fn execute_input_frames_name_and_args() {
        let buf = execute_input("ioctl_read_msr", &[0x19C]);
        assert_eq!(buf.len(), 32 + 8);
        assert_eq!(&buf[..14], b"ioctl_read_msr");
        assert_eq!(buf[15], 0); // zero-padded name field
        assert_eq!(&buf[32..], &0x19Ci64.to_le_bytes());
    }

    #[test]
    fn decodes_readout_as_offset_below_tjmax() {
        // Readout 55 below TjMax 100 → 45 °C (throttle flag also set).
        assert_eq!(temperature_celsius(0x0037_0001, 100), Some(45));
        // Readout 10 below TjMax 100 → 90 °C.
        assert_eq!(temperature_celsius(0x000A_0000, 100), Some(90));
        // TjMax comes per-core, e.g. 105.
        assert_eq!(temperature_celsius(0x002D_0000, 105), Some(60));
        // Readout 0 with the reading-valid bit set = exactly TjMax.
        assert_eq!(temperature_celsius(0x8000_0000, 100), Some(100));
    }

    #[test]
    fn no_reading_means_core_does_not_support_the_msr() {
        assert_eq!(temperature_celsius(0, 100), None);
        assert_eq!(temperature_celsius(0x0000_0001, 100), None); // bit 31 clear, zero readout
    }

    #[test]
    fn ignores_other_bits() {
        // Readout 0x40 = 64 below TjMax 100 → 36 °C.
        assert_eq!(temperature_celsius(0xFF40_0000, 100), Some(36));
    }

    #[test]
    fn decodes_tjmax_from_temperature_target() {
        assert_eq!(tjmax_celsius(0x0064_0000), Some(100));
        assert_eq!(tjmax_celsius(0x0069_0000), Some(105));
        assert_eq!(tjmax_celsius(0), None);
    }

    #[test]
    fn average_skips_unreadable_cores() {
        let sample = MsrThermalSample {
            throttling: false,
            temps_c: vec![Some(50), None, Some(70)],
        };
        assert_eq!(sample.average_celsius(), Some(60.0));

        let none = MsrThermalSample {
            throttling: false,
            temps_c: vec![None, None],
        };
        assert_eq!(none.average_celsius(), None);
    }
}
