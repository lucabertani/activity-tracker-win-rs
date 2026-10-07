// Probe: per-core MSR 0x19C (IA32_THERM_STATUS) via the WinRing0 driver.
// Requires the WinRing0_1_2_0 kernel service to be installed and running.
use std::mem;
use std::ptr;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

const OLS_TYPE: u32 = 40000;
// CTL_CODE(OLS_TYPE, 0x821, METHOD_BUFFERED(0), FILE_ANY_ACCESS(0))
const IOCTL_OLS_READ_MSR: u32 = (OLS_TYPE << 16) | (0x821 << 2);
const DEVICE: &str = r"\\.\WinRing0_1_2_0";

const MSR_IA32_THERM_STATUS: u32 = 0x19C;
const MSR_IA32_TEMPERATURE_TARGET: u32 = 0x1A2;

fn open_device() -> Option<HANDLE> {
    let wide: Vec<u16> = DEVICE.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        windows_sys::Win32::Storage::FileSystem::CreateFileW(
            wide.as_ptr(),
            0xC0000000, // GENERIC_READ | GENERIC_WRITE
            0,
            std::ptr::null_mut(),
            3, // OPEN_EXISTING
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        let err = std::io::Error::last_os_error();
        eprintln!(
            "open {DEVICE} failed: {err} (code 0x{:08X})",
            err.raw_os_error().unwrap_or(0)
        );
        None
    } else {
        Some(handle)
    }
}

fn read_msr(handle: HANDLE, index: u32) -> Option<u64> {
    let mut out: u64 = 0;
    let mut ret: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_OLS_READ_MSR,
            &index as *const u32 as *const _,
            mem::size_of::<u32>() as u32,
            &mut out as *mut u64 as *mut _,
            mem::size_of::<u64>() as u32,
            &mut ret,
            ptr::null_mut(),
        )
    };
    if ok != 0 && ret == mem::size_of::<u64>() as u32 {
        Some(out)
    } else {
        None
    }
}

fn on_core<T>(core: usize, f: impl FnOnce() -> T) -> T {
    let mask = 1usize << core;
    unsafe {
        let _ = SetThreadAffinityMask(GetCurrentThread(), mask);
    }
    f()
}

fn main() {
    let Some(handle) = open_device() else {
        eprintln!("driver not available; run install-driver.ps1 as admin first");
        return;
    };

    let mut sys_info = windows_sys::Win32::System::SystemInformation::SYSTEM_INFO::default();
    unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut sys_info);
    }
    let max_core = sys_info.dwNumberOfProcessors as usize;
    println!("logical processors: {max_core}");
    let tjmax = on_core(0, || read_msr(handle, MSR_IA32_TEMPERATURE_TARGET))
        .map(|v| ((v >> 16) & 0xFF) as i64)
        .unwrap_or(100);
    println!("TjMax (from MSR 0x1A2): {tjmax} C");

    for sample in 0..6 {
        std::thread::sleep(Duration::from_millis(500));
        println!("--- sample {sample} ---");
        let mut any_throttle = false;
        for core in 0..max_core {
            let msr = on_core(core, || read_msr(handle, MSR_IA32_THERM_STATUS));
            let Some(msr) = msr else { break }; // fewer than 64 cores -> stop
            let thermal = msr & 1 != 0;
            let prochot_log = (msr >> 1) & 1 != 0;
            // Modern Intel (Skylake+): bits 22:16 are the actual temperature in °C.
            // A zero reading means the MSR is not implemented on that core.
            let temp = ((msr >> 16) & 0x7F) as i64;
            let temp = if temp == 0 { -1 } else { temp };
            if thermal {
                any_throttle = true;
            }
            println!(
                "core {core:2}: thermal_status={thermal} prochot_log={prochot_log} temp={temp:3} C  msr=0x{msr:016X}"
            );
        }
        println!("any core thermally throttling: {any_throttle}");
    }

    unsafe {
        let _ = CloseHandle(handle);
    }
}
