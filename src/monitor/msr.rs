//! HWiNFO-style per-core thermal throttling + temperature via the WinRing0
//! kernel driver (MSR `IA32_THERM_STATUS`, 0x19C). Bit 0 is the thermal
//! status flag; bits 22:16 are the digital thermal sensor readout on modern
//! Intel (Skylake and later, incl. Core Ultra — same layout HWiNFO and
//! LibreHardwareMonitor read): the offset *below* TjMax, so the absolute
//! temperature is `TjMax - readout`, with TjMax from `IA32_TEMPERATURE_TARGET`.
//! The driver must be installed once as
//! administrator (`driver/install-driver.ps1`); after that the device is
//! readable from a normal user process.

use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

/// `CTL_CODE(40000, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS)` — WinRing0
/// `IOCTL_OLS_READ_MSR`: input = MSR index (u32), output = value (u64).
const IOCTL_OLS_READ_MSR: u32 = (40000 << 16) | (0x821 << 2);
const DEVICE_NAME: &str = r"\\.\WinRing0_1_2_0";
const MSR_IA32_THERM_STATUS: u32 = 0x19C;
/// `IA32_TEMPERATURE_TARGET` (0x1A2): bits 23:16 hold the TCC activation
/// temperature (TjMax), the reference point the digital readout is relative to.
const MSR_IA32_TEMPERATURE_TARGET: u32 = 0x1A2;
/// Bit 0 of IA32_THERM_STATUS: the core is currently at/above its thermal
/// trip point, i.e. it is thermal-throttling right now.
const THERM_STATUS_THERMAL: u64 = 1;
/// TjMax assumed when `IA32_TEMPERATURE_TARGET` is unreadable (Intel default).
const DEFAULT_TJMAX_C: u16 = 100;

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
    /// Opens the WinRing0 device. Returns `None` when the driver is not
    /// installed (the caller then falls back to the frequency heuristic).
    pub fn open() -> Option<Self> {
        let wide: Vec<u16> = DEVICE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            windows_sys::Win32::Storage::FileSystem::CreateFileW(
                wide.as_ptr(),
                0xC0000000, // GENERIC_READ | GENERIC_WRITE
                0,
                ptr::null_mut(),
                3, // OPEN_EXISTING
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut sys_info = windows_sys::Win32::System::SystemInformation::SYSTEM_INFO::default();
        unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut sys_info);
        }
        let core_count = sys_info.dwNumberOfProcessors as usize;

        Some(Self { handle, core_count })
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
        let mut value: u64 = 0;
        let mut returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_OLS_READ_MSR,
                &index as *const u32 as *const _,
                mem::size_of::<u32>() as u32,
                &mut value as *mut u64 as *mut _,
                mem::size_of::<u64>() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok != 0 && returned == mem::size_of::<u64>() as u32 {
            Some(value)
        } else {
            None
        }
    }
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
    use super::{MsrThermalSample, temperature_celsius, tjmax_celsius};

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
