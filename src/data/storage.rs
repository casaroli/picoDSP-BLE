use crate::data::presets::{get_default_presets, Preset};
use crate::usb::logger::SYSTEM_STATUS_CHANNEL;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH;
use embedded_storage_async::nor_flash::NorFlash;

// "PDSP"
pub const MAGIC: u32 = 0x50445350;
// Bump on any change to the default preset bank or the on-flash layout so existing
// devices re-format and preload the new defaults on next boot.
pub const VERSION: u32 = 9;

const FLASH_SIZE: u32 = 2 * 1024 * 1024;
const STORAGE_SIZE: u32 = 64 * 1024;
const ADDR_OFFSET: u32 = FLASH_SIZE - STORAGE_SIZE;
const SECTOR_SIZE: u32 = 4096;
const HEADER_SIZE: u32 = 16;

/// Maximum presets the storage can hold. A 128-preset bank spans several 4 KB flash sectors
/// (16 + 128*200 = 25616 B -> 7 sectors), well within the 64 KB reserved region. Note: BLE/USB
/// Program Change is 7-bit, so presets >= 128 wouldn't be PC-addressable anyway.
pub const MAX_PRESETS: usize = 128;

/// Round `n` up to a whole number of flash sectors (= erase granularity).
const fn sectors_ceil(n: u32) -> u32 {
    n.div_ceil(SECTOR_SIZE) * SECTOR_SIZE
}

#[repr(C)]
struct StorageHeader {
    magic: u32,
    version: u32,
    num_presets: u32,
    padding: u32,
}

pub struct Storage<'d> {
    flash: Flash<'d, FLASH, Async, 2097152>,
}

macro_rules! log_storage {
    ($($arg:tt)*) => {
        {
            let mut msg = heapless::String::<64>::new();
            if core::fmt::write(&mut msg, format_args!($($arg)*)).is_ok() {
                let _ = SYSTEM_STATUS_CHANNEL.try_send(msg);
            }
        }
    };
}

impl<'d> Storage<'d> {
    pub fn new(flash: Flash<'d, FLASH, Async, 2097152>) -> Self {
        Self { flash }
    }

    pub async fn init(&mut self) {
        let mut buf = [0u8; 16];
        self.flash.read(ADDR_OFFSET, &mut buf).await.unwrap();

        let header: StorageHeader = unsafe { core::ptr::read(buf.as_ptr() as *const _) };

        if header.magic != MAGIC || header.version != VERSION {
            log_storage!("Storage init/version mismatch. Formatting...\r\n");
            self.format().await;
        } else {
            log_storage!(
                "Storage initialized. Found {} presets.\r\n",
                header.num_presets
            );
        }
    }

    pub async fn format(&mut self) {
        log_storage!("Formatting storage area...\r\n");

        // Park core1 off PSRAM for the whole flash write + reconfigure.
        crate::psram::lock_core1_off_psram().await;

        let defaults = get_default_presets();
        let num_presets = defaults.len();
        let preset_size = core::mem::size_of::<Preset>();
        debug_assert!(num_presets <= MAX_PRESETS);

        // Build the whole header+presets image (rounded up to a sector multiple, zero-padded)
        // on the heap — format() runs single-core at boot before the synth allocates, so the
        // heap is free. This spans as many 4 KB sectors as the bank needs.
        let used = HEADER_SIZE as usize + num_presets * preset_size;
        let image_len = sectors_ceil(used as u32) as usize;

        self.flash
            .erase(ADDR_OFFSET, ADDR_OFFSET + image_len as u32)
            .await
            .unwrap();

        let header = StorageHeader {
            magic: MAGIC,
            version: VERSION,
            num_presets: num_presets as u32,
            padding: 0,
        };
        let mut image = alloc::vec![0u8; image_len];
        let header_bytes: [u8; 16] = unsafe { core::mem::transmute(header) };
        image[0..16].copy_from_slice(&header_bytes);
        let presets_bytes = unsafe {
            core::slice::from_raw_parts(defaults.as_ptr() as *const u8, num_presets * preset_size)
        };
        image[16..16 + presets_bytes.len()].copy_from_slice(presets_bytes);

        self.flash.write(ADDR_OFFSET, &image).await.unwrap();
        crate::psram::after_flash_write();
        crate::psram::unlock_core1();
        log_storage!("Formatted: wrote {} presets.\r\n", num_presets);
    }

    /// Number of presets currently stored (from the on-flash header). Used to wrap when
    /// cycling through presets.
    pub async fn num_presets(&mut self) -> usize {
        let mut buf = [0u8; 16];
        self.flash.read(ADDR_OFFSET, &mut buf).await.unwrap();
        let header: StorageHeader = unsafe { core::ptr::read(buf.as_ptr() as *const _) };
        header.num_presets as usize
    }

    pub async fn load_preset(&mut self, index: usize) -> Option<Preset> {
        let mut buf = [0u8; 16];
        self.flash.read(ADDR_OFFSET, &mut buf).await.unwrap();
        let header: StorageHeader = unsafe { core::ptr::read(buf.as_ptr() as *const _) };

        if index >= header.num_presets as usize {
            log_storage!(
                "Error: Preset index {} out of bounds (max {})\r\n",
                index,
                header.num_presets - 1
            );
            return None;
        }

        let preset_size = core::mem::size_of::<Preset>();
        let offset = ADDR_OFFSET + 16 + (index * preset_size) as u32;

        let mut preset_buf = [0u8; 256];
        if preset_size > 256 {
            log_storage!("Error: Preset too large for buffer!\r\n");
            return None;
        }

        self.flash
            .read(offset, &mut preset_buf[..preset_size])
            .await
            .unwrap();

        let preset: Preset = unsafe { core::ptr::read(preset_buf.as_ptr() as *const _) };

        log_storage!("Loaded preset {}: {}\r\n", index, preset.get_name());
        Some(preset)
    }

    pub async fn read_raw(&mut self, buf: &mut [u8]) {
        let len = buf.len().min(STORAGE_SIZE as usize);
        self.flash.read(ADDR_OFFSET, &mut buf[..len]).await.unwrap();
    }

    pub async fn write_raw(&mut self, data: &[u8]) {
        // Park core1 off PSRAM for the whole flash write + reconfigure (the delay buffer
        // shares the QMI bus with flash), then release once PSRAM is reconfigured.
        crate::psram::lock_core1_off_psram().await;

        // Erase as many whole sectors as `data` spans (a full 128-preset bank is 7 sectors),
        // capped at the reserved region. `data` is expected to be a sector-multiple image.
        let erase_len = sectors_ceil(data.len() as u32).min(STORAGE_SIZE);
        log_storage!("SysEx Write: Erasing {} bytes...\r\n", erase_len);
        self.flash
            .erase(ADDR_OFFSET, ADDR_OFFSET + erase_len)
            .await
            .unwrap();

        log_storage!("SysEx Write: Writing {} bytes...\r\n", data.len());
        self.flash.write(ADDR_OFFSET, data).await.unwrap();

        crate::psram::after_flash_write();
        crate::psram::unlock_core1();

        log_storage!("SysEx Write: Done.\r\n");
    }
}
