//! HWiNFO-style per-core thermal throttling + temperature via the PawnIO
//! kernel driver (namazso) with the signed `IntelMSR` module.
//!
//! Registers read (all on the module's read whitelist):
//! - `IA32_THERM_STATUS` (0x19C), per core: bit 0 thermal status, bit 2
//!   PROCHOT#, bit 10 power limitation, bits 22:16 digital readout (offset
//!   *below* TjMax), bit 31 reading valid;
//! - `IA32_PACKAGE_THERM_STATUS` (0x1B1): same layout for the package;
//! - `IA32_TEMPERATURE_TARGET` (0x1A2): TjMax in bits 23:16, read once;
//! - `IA32_MPERF` / `IA32_APERF` (0xE7/0xE8) and `MSR_PLATFORM_INFO` (0xCE):
//!   effective clock = nominal clock * dAPERF / dMPERF.
//!
//! The sticky "log" bits cannot be used: clearing them needs a write to
//! 0x19C, which the module does not allow. Short bursts are caught by
//! sampling often instead (see `thermal_sampler.rs`).
//!
//! PawnIO must be installed once as administrator (see `driver/README.md`).
//! Its device grants access only to SYSTEM and Administrators (the INF sets
//! `D:P(A;;GA;;;SY)(A;;GA;;;BA)`), so the app relaunches itself elevated when
//! access is denied.

use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore,
};
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

use crate::model::ThrottleReasons;

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
/// 0.2.11): exposes only a whitelist of MSR reads/writes.
const INTELMSR_MODULE: &[u8] = include_bytes!("../../driver/pawnio/IntelMSR.bin");

const MSR_IA32_MPERF: u32 = 0xE7;
const MSR_IA32_APERF: u32 = 0xE8;
const MSR_PLATFORM_INFO: u32 = 0xCE;
const MSR_IA32_THERM_STATUS: u32 = 0x19C;
const MSR_IA32_TEMPERATURE_TARGET: u32 = 0x1A2;
const MSR_IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;

/// Bit 0: at/above the thermal trip point right now.
const STATUS_THERMAL: u64 = 1;
/// Bit 2: PROCHOT# / FORCEPR# asserted right now.
const STATUS_PROCHOT: u64 = 1 << 2;
/// Bit 10: running at a power limit right now.
const STATUS_POWER_LIMIT: u64 = 1 << 10;
/// Bit 31 of `IA32_THERM_STATUS`: the digital readout is valid.
const STATUS_READING_VALID: u64 = 1 << 31;

/// TjMax assumed when `IA32_TEMPERATURE_TARGET` is unreadable (Intel default).
const DEFAULT_TJMAX_C: u16 = 100;
/// Bus clock multiplying the `PLATFORM_INFO` ratio (all Core CPUs since
/// Sandy Bridge).
const BUS_CLOCK_MHZ: u64 = 100;
/// Thread affinity masks cover one processor group of 64 logical processors.
const MAX_AFFINITY_BITS: usize = usize::BITS as usize;
/// Win32 `ERROR_ACCESS_DENIED`: the device exists but denies this process.
const ERROR_ACCESS_DENIED: i32 = 5;

/// Why the thermal monitor is unavailable.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// PawnIO is not installed, or the `IntelMSR` module cannot load (non-Intel
    /// CPU, unsigned module): no temperature or throttling data.
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

/// One physical core, sampled through one of its logical processors (HT
/// siblings share the thermal MSRs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreSlot {
    /// Logical processor index (bit in the group-0 affinity mask).
    pub logical: usize,
    /// 0 = most efficient (E-core) on hybrid CPUs; higher = P-core.
    pub efficiency_class: u8,
}

/// One core in one sample.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoreSample {
    /// Raw `IA32_THERM_STATUS`, `None` when the read failed.
    pub therm_status: Option<u64>,
    pub temp_c: Option<u16>,
    /// Effective clock since the previous frequency sample.
    pub effective_mhz: Option<u32>,
}

/// One sample across all physical cores.
#[derive(Debug, Clone, Default)]
pub struct MsrThermalSample {
    pub reasons: ThrottleReasons,
    /// Per physical core, in [`MsrThermalMonitor::cores`] order.
    pub cores: Vec<CoreSample>,
    /// Raw `IA32_PACKAGE_THERM_STATUS`.
    pub package_status: Option<u64>,
    pub package_temp_c: Option<u16>,
}

impl MsrThermalSample {
    /// At least one core answered: the sample can be trusted.
    pub fn readable(&self) -> bool {
        self.cores.iter().any(|core| core.therm_status.is_some())
    }

    /// Average of the valid core temperatures in °C.
    pub fn average_celsius(&self) -> Option<f64> {
        average(
            self.cores
                .iter()
                .filter_map(|core| core.temp_c.map(f64::from)),
        )
    }

    pub fn average_mhz(&self) -> Option<u32> {
        average(
            self.cores
                .iter()
                .filter_map(|core| core.effective_mhz.map(f64::from)),
        )
        .map(|mhz| mhz.round() as u32)
    }

    pub fn max_mhz(&self) -> Option<u32> {
        self.cores
            .iter()
            .filter_map(|core| core.effective_mhz)
            .max()
    }
}

fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0u32), |(sum, count), v| (sum + v, count + 1));
    (count > 0).then(|| sum / f64::from(count))
}

/// Reads the thermal MSRs on every physical core.
pub struct MsrThermalMonitor {
    handle: HANDLE,
    cores: Vec<CoreSlot>,
    /// Per core, read once at open (it is a fused constant).
    tjmax: Vec<u16>,
    nominal_mhz: Option<u64>,
    /// Per core `(APERF, MPERF)` from the previous frequency sample.
    last_perf: Vec<Option<(u64, u64)>>,
}

// The HANDLE is a kernel object reference usable from any thread and this
// struct owns it exclusively; safe to move the monitor across threads.
unsafe impl Send for MsrThermalMonitor {}

impl MsrThermalMonitor {
    /// Opens the PawnIO device and loads the `IntelMSR` module. Fails with
    /// [`OpenError::Missing`] when PawnIO is not available and with
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

        let cores = physical_cores();
        let tjmax = cores
            .iter()
            .map(|core| {
                on_logical(core.logical, || {
                    read_msr(handle, MSR_IA32_TEMPERATURE_TARGET).and_then(tjmax_celsius)
                })
                .flatten()
                .unwrap_or(DEFAULT_TJMAX_C)
            })
            .collect();
        let nominal_mhz = read_msr(handle, MSR_PLATFORM_INFO).and_then(nominal_mhz);
        let last_perf = vec![None; cores.len()];

        Ok(Self {
            handle,
            cores,
            tjmax,
            nominal_mhz,
            last_perf,
        })
    }

    /// The physical cores sampled, in sample order.
    pub fn cores(&self) -> &[CoreSlot] {
        &self.cores
    }

    /// Samples every core. With `with_frequency`, also reads APERF/MPERF:
    /// the effective clock is averaged since the previous frequency sample
    /// (`None` on the first one).
    pub fn sample(&mut self, with_frequency: bool) -> MsrThermalSample {
        let handle = self.handle;
        let nominal_mhz = self.nominal_mhz;
        let mut reasons = ThrottleReasons::default();
        let mut cores = Vec::with_capacity(self.cores.len());

        for (index, slot) in self.cores.iter().enumerate() {
            let read = on_logical(slot.logical, || {
                let status = read_msr(handle, MSR_IA32_THERM_STATUS);
                let perf = if with_frequency {
                    read_msr(handle, MSR_IA32_APERF).zip(read_msr(handle, MSR_IA32_MPERF))
                } else {
                    None
                };
                (status, perf)
            });
            let Some((status, perf)) = read else {
                cores.push(CoreSample::default());
                continue;
            };

            let mut effective_mhz = None;
            if let Some(perf) = perf {
                if let (Some(previous), Some(nominal)) = (self.last_perf[index], nominal_mhz) {
                    effective_mhz = effective_mhz_between(previous, perf, nominal);
                }
                self.last_perf[index] = Some(perf);
            }

            if let Some(status) = status {
                reasons = reasons.union(status_reasons(status, false));
            }
            cores.push(CoreSample {
                therm_status: status,
                temp_c: status.and_then(|s| temperature_celsius(s, self.tjmax[index])),
                effective_mhz,
            });
        }

        // Package-scoped register: any core can read it.
        let package_status = read_msr(handle, MSR_IA32_PACKAGE_THERM_STATUS);
        if let Some(status) = package_status {
            reasons = reasons.union(status_reasons(status, true));
        }
        let package_tjmax = self.tjmax.first().copied().unwrap_or(DEFAULT_TJMAX_C);
        let package_temp_c = package_status.map(|s| package_temperature_celsius(s, package_tjmax));

        MsrThermalSample {
            reasons,
            cores,
            package_status,
            package_temp_c,
        }
    }
}

/// Runs `f` pinned to one logical processor, restoring the previous
/// affinity afterwards. `None` when the processor cannot be selected.
fn on_logical<T>(logical: usize, f: impl FnOnce() -> T) -> Option<T> {
    if logical >= MAX_AFFINITY_BITS {
        return None;
    }
    let previous = unsafe { SetThreadAffinityMask(GetCurrentThread(), 1usize << logical) };
    if previous == 0 {
        return None;
    }
    let result = f();
    unsafe {
        SetThreadAffinityMask(GetCurrentThread(), previous);
    }
    Some(result)
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

/// The physical cores of processor group 0, one representative logical
/// processor each. Falls back to one slot per logical processor when the
/// topology cannot be read.
fn physical_cores() -> Vec<CoreSlot> {
    let mut len = 0u32;
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, ptr::null_mut(), &mut len);
    }
    if len > 0 {
        // u64 storage keeps the entries 8-byte aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let ok = unsafe {
            GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                buf.as_mut_ptr() as *mut _,
                &mut len,
            )
        };
        if ok != 0 {
            let bytes =
                unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len as usize) };
            let cores = parse_core_slots(bytes);
            if !cores.is_empty() {
                return cores;
            }
        }
    }

    let mut sys_info = windows_sys::Win32::System::SystemInformation::SYSTEM_INFO::default();
    unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut sys_info);
    }
    (0..(sys_info.dwNumberOfProcessors as usize).min(MAX_AFFINITY_BITS))
        .map(|logical| CoreSlot {
            logical,
            efficiency_class: 0,
        })
        .collect()
}

/// `SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX` (x64) byte offsets for
/// `RelationProcessorCore` entries: header (Relationship, Size), then
/// `PROCESSOR_RELATIONSHIP` { Flags, EfficiencyClass, Reserved[20],
/// GroupCount, GroupMask[] } with each `GROUP_AFFINITY` { Mask, Group }.
const OFF_SIZE: usize = 4;
const OFF_EFFICIENCY_CLASS: usize = 9;
const OFF_GROUP_COUNT: usize = 30;
const OFF_GROUP_MASK: usize = 32;
const OFF_GROUP_INDEX: usize = 40;
const ENTRY_MIN_LEN: usize = OFF_GROUP_INDEX + 2;

/// Parses the processor-core entries. Only cores in group 0 are kept
/// (affinity masks address one group); each is represented by its lowest
/// logical processor.
fn parse_core_slots(bytes: &[u8]) -> Vec<CoreSlot> {
    let mut cores = Vec::new();
    let mut offset = 0usize;
    while offset + ENTRY_MIN_LEN <= bytes.len() {
        let size = u32_at(bytes, offset + OFF_SIZE) as usize;
        let group_count = u16_at(bytes, offset + OFF_GROUP_COUNT);
        let mask = u64_at(bytes, offset + OFF_GROUP_MASK);
        let group = u16_at(bytes, offset + OFF_GROUP_INDEX);
        if group_count >= 1 && group == 0 && mask != 0 {
            cores.push(CoreSlot {
                logical: mask.trailing_zeros() as usize,
                efficiency_class: bytes[offset + OFF_EFFICIENCY_CLASS],
            });
        }
        if size == 0 {
            break;
        }
        offset += size;
    }
    cores.sort_by_key(|core| core.logical);
    cores
}

fn u16_at(buf: &[u8], offset: usize) -> u16 {
    buf.get(offset..offset + 2)
        .and_then(|bytes| bytes.first_chunk::<2>())
        .map_or(0, |bytes| u16::from_ne_bytes(*bytes))
}

fn u32_at(buf: &[u8], offset: usize) -> u32 {
    buf.get(offset..offset + 4)
        .and_then(|bytes| bytes.first_chunk::<4>())
        .map_or(0, |bytes| u32::from_ne_bytes(*bytes))
}

fn u64_at(buf: &[u8], offset: usize) -> u64 {
    buf.get(offset..offset + 8)
        .and_then(|bytes| bytes.first_chunk::<8>())
        .map_or(0, |bytes| u64::from_ne_bytes(*bytes))
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

/// Throttle reasons encoded in a core or package thermal status register.
/// Both share the bit positions used here.
pub(crate) fn status_reasons(status: u64, package: bool) -> ThrottleReasons {
    let thermal = status & STATUS_THERMAL != 0;
    ThrottleReasons {
        core_thermal: thermal && !package,
        package_thermal: thermal && package,
        prochot: status & STATUS_PROCHOT != 0,
        power_limit: status & STATUS_POWER_LIMIT != 0,
    }
}

/// Decodes TjMax (TCC activation temperature) from `IA32_TEMPERATURE_TARGET`.
pub(crate) fn tjmax_celsius(temperature_target: u64) -> Option<u16> {
    let raw = ((temperature_target >> 16) & 0xFF) as u16;
    (raw != 0).then_some(raw)
}

/// Nominal (non-turbo) clock from `MSR_PLATFORM_INFO` bits 15:8.
pub(crate) fn nominal_mhz(platform_info: u64) -> Option<u64> {
    let ratio = (platform_info >> 8) & 0xFF;
    (ratio != 0).then_some(ratio * BUS_CLOCK_MHZ)
}

/// Decodes the core temperature in °C from `IA32_THERM_STATUS`: TjMax minus
/// the digital readout in bits 22:16 (Intel SDM Vol. 4, MSR 19CH). `None`
/// unless bit 31 (reading valid) is set, as LibreHardwareMonitor does.
pub(crate) fn temperature_celsius(therm_status: u64, tjmax: u16) -> Option<u16> {
    if therm_status & STATUS_READING_VALID == 0 {
        return None;
    }
    let readout = ((therm_status >> 16) & 0x7F) as u16;
    Some(tjmax.saturating_sub(readout))
}

/// Package temperature from `IA32_PACKAGE_THERM_STATUS` (no valid bit there).
pub(crate) fn package_temperature_celsius(package_status: u64, tjmax: u16) -> u16 {
    let readout = ((package_status >> 16) & 0x7F) as u16;
    tjmax.saturating_sub(readout)
}

/// Effective clock between two `(APERF, MPERF)` readings. `None` when the
/// core sat idle the whole time (MPERF does not count outside C0) or a
/// counter went backwards.
pub(crate) fn effective_mhz_between(
    previous: (u64, u64),
    current: (u64, u64),
    nominal_mhz: u64,
) -> Option<u32> {
    let aperf = current.0.checked_sub(previous.0)?;
    let mperf = current.1.checked_sub(previous.1)?;
    if mperf == 0 {
        return None;
    }
    let mhz = nominal_mhz as f64 * aperf as f64 / mperf as f64;
    Some(mhz.round() as u32)
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
    use super::*;

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
        // Valid bit + readout 55 below TjMax 100 → 45 °C.
        assert_eq!(temperature_celsius(0x8037_0001, 100), Some(45));
        // Readout 10 below TjMax 100 → 90 °C.
        assert_eq!(temperature_celsius(0x800A_0000, 100), Some(90));
        // TjMax comes per core, e.g. 105.
        assert_eq!(temperature_celsius(0x802D_0000, 105), Some(60));
        // Readout 0 with the valid bit = exactly TjMax.
        assert_eq!(temperature_celsius(0x8000_0000, 100), Some(100));
    }

    #[test]
    fn readings_without_the_valid_bit_are_rejected() {
        assert_eq!(temperature_celsius(0, 100), None);
        // A readout without bit 31 is not a temperature.
        assert_eq!(temperature_celsius(0x0037_0001, 100), None);
    }

    #[test]
    fn ignores_other_bits() {
        // Readout 0x40 = 64 below TjMax 100 → 36 °C.
        assert_eq!(temperature_celsius(0xFF40_0000, 100), Some(36));
    }

    #[test]
    fn decodes_package_temperature() {
        assert_eq!(package_temperature_celsius(0x0028_0000, 100), 60);
        assert_eq!(package_temperature_celsius(0x0000_0000, 100), 100);
    }

    #[test]
    fn decodes_tjmax_from_temperature_target() {
        assert_eq!(tjmax_celsius(0x0064_0000), Some(100));
        assert_eq!(tjmax_celsius(0x0069_0000), Some(105));
        assert_eq!(tjmax_celsius(0), None);
    }

    #[test]
    fn decodes_nominal_clock() {
        // Ratio 23 → 2300 MHz (Core Ultra 9 185H base clock).
        assert_eq!(nominal_mhz(0x0000_1700), Some(2300));
        assert_eq!(nominal_mhz(0), None);
    }

    #[test]
    fn decodes_throttle_reasons() {
        let core = status_reasons(0x8000_0405, false);
        assert!(core.core_thermal && core.prochot && core.power_limit);
        assert!(!core.package_thermal);

        let package = status_reasons(0x0000_0001, true);
        assert!(package.package_thermal && !package.core_thermal);

        // Log bits (1, 3, 11) alone are history, not a current reason.
        assert_eq!(
            status_reasons(0x0000_080A, false),
            ThrottleReasons::default()
        );
    }

    #[test]
    fn effective_clock_from_aperf_mperf() {
        // APERF advanced 1.5x MPERF → 1.5x nominal.
        assert_eq!(
            effective_mhz_between((1000, 1000), (2500, 2000), 2300),
            Some(3450)
        );
        // Idle core (MPERF did not move) and counter wrap give no value.
        assert_eq!(effective_mhz_between((10, 10), (10, 10), 2300), None);
        assert_eq!(effective_mhz_between((10, 10), (5, 20), 2300), None);
    }

    fn core_entry(mask: u64, group: u16, efficiency_class: u8) -> Vec<u8> {
        let mut entry = vec![0u8; 48];
        entry[0..4].copy_from_slice(&0u32.to_ne_bytes()); // RelationProcessorCore
        entry[4..8].copy_from_slice(&48u32.to_ne_bytes());
        entry[OFF_EFFICIENCY_CLASS] = efficiency_class;
        entry[OFF_GROUP_COUNT..OFF_GROUP_COUNT + 2].copy_from_slice(&1u16.to_ne_bytes());
        entry[OFF_GROUP_MASK..OFF_GROUP_MASK + 8].copy_from_slice(&mask.to_ne_bytes());
        entry[OFF_GROUP_INDEX..OFF_GROUP_INDEX + 2].copy_from_slice(&group.to_ne_bytes());
        entry
    }

    #[test]
    fn parses_one_slot_per_physical_core() {
        let mut bytes = Vec::new();
        bytes.extend(core_entry(0b11, 0, 1)); // P-core with HT: logical 0+1
        bytes.extend(core_entry(0b100, 0, 0)); // E-core: logical 2
        bytes.extend(core_entry(0b1, 1, 0)); // group 1: skipped
        assert_eq!(
            parse_core_slots(&bytes),
            vec![
                CoreSlot {
                    logical: 0,
                    efficiency_class: 1
                },
                CoreSlot {
                    logical: 2,
                    efficiency_class: 0
                },
            ]
        );
    }

    #[test]
    fn averages_skip_unreadable_cores() {
        let sample = MsrThermalSample {
            cores: vec![
                CoreSample {
                    therm_status: Some(0),
                    temp_c: Some(50),
                    effective_mhz: Some(3000),
                },
                CoreSample::default(),
                CoreSample {
                    therm_status: Some(0),
                    temp_c: Some(70),
                    effective_mhz: Some(1000),
                },
            ],
            ..Default::default()
        };
        assert!(sample.readable());
        assert_eq!(sample.average_celsius(), Some(60.0));
        assert_eq!(sample.average_mhz(), Some(2000));
        assert_eq!(sample.max_mhz(), Some(3000));

        let none = MsrThermalSample {
            cores: vec![CoreSample::default(); 2],
            ..Default::default()
        };
        assert!(!none.readable());
        assert_eq!(none.average_celsius(), None);
    }
}
