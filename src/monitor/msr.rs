//! HWiNFO-style per-core thermal throttling + temperature via the WinRing0
//! kernel driver (MSR `IA32_THERM_STATUS`, 0x19C). Bit 0 is the thermal
//! status flag; bits 22:16 are the core temperature in °C on modern Intel
//! (Skylake and later, incl. Core Ultra — same layout HWiNFO and
//! LibreHardwareMonitor read). The driver must be installed once as
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
/// Bit 0 of IA32_THERM_STATUS: the core is currently at/above its thermal
/// trip point, i.e. it is thermal-throttling right now.
const THERM_STATUS_THERMAL: u64 = 1;

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
            temps_c.push(temperature_celsius(value));
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

/// Decodes the temperature field (bits 22:16) of `IA32_THERM_STATUS`.
/// A zero reading means the MSR is not implemented on that core (this
/// happens on some E-cores), so it is reported as `None`.
pub(crate) fn temperature_celsius(therm_status: u64) -> Option<u16> {
    let raw = (therm_status >> 16) & 0x7F;
    (raw != 0).then_some(raw as u16)
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
    use super::{MsrThermalSample, temperature_celsius};

    #[test]
    fn decodes_bits_22_16_as_temperature() {
        // 55 °C with the throttle flag also set.
        assert_eq!(temperature_celsius(0x0037_0001), Some(55));
        assert_eq!(temperature_celsius(0x005A_0000), Some(90));
    }

    #[test]
    fn zero_reading_means_core_does_not_support_the_msr() {
        assert_eq!(temperature_celsius(0), None);
    }

    #[test]
    fn ignores_other_bits() {
        assert_eq!(temperature_celsius(0xFF40_0000), Some(64));
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
