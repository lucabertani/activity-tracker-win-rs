//! HWiNFO-style per-core thermal throttling detection via the WinRing0
//! kernel driver (MSR `IA32_THERM_STATUS`, 0x19C, bit 0 = PROCHOT / thermal
//! status). The driver must be installed once as administrator
//! (`driver/install-driver.ps1`); after that the device is readable from a
//! normal user process, exactly like HWiNFO reads the same MSRs.

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

/// Reads MSR `IA32_THERM_STATUS` on every logical processor. `refresh()`
/// returns `true` as soon as a single core is thermally throttling
/// (same semantics as the frequency-based heuristic).
pub struct MsrThermalMonitor {
    handle: HANDLE,
    core_count: usize,
}

impl MsrThermalMonitor {
    /// Opens the WinRing0 device. Returns `None` when the driver is not
    /// installed (the caller then falls back to the frequency heuristic).
    pub fn open() -> Option<Self> {
        let wide: Vec<u16> = DEVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
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

    /// Returns `true` when at least one core is currently thermal-throttling.
    pub fn refresh(&self) -> bool {
        let mut any_throttling = false;
        for core in 0..self.core_count {
            let previous = unsafe { SetThreadAffinityMask(GetCurrentThread(), 1usize << core) };
            if previous == 0 {
                // Core does not exist (or affinity is restricted): skip it.
                continue;
            }
            let value = self.read_msr(MSR_IA32_THERM_STATUS);
            unsafe {
                SetThreadAffinityMask(GetCurrentThread(), previous);
            }
            if value.is_some_and(|v| v & THERM_STATUS_THERMAL != 0) {
                any_throttling = true;
            }
        }
        any_throttling
    }

    fn read_msr(&self, index: u32) -> Option<u64> {
        let mut value: u64 = 0;
        let mut returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
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

impl Drop for MsrThermalMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
