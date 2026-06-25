use crate::data::presets::{Preset, get_default_presets};
use crate::usb::logger::SYSTEM_STATUS_CHANNEL;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH;
use embedded_storage_async::nor_flash::NorFlash;

// "PDSP"
pub const MAGIC: u32 = 0x50445350;
// Bump on any change to the default preset bank or the on-flash layout so existing
// devices re-format and preload the new defaults on next boot.
pub const VERSION: u32 = 11;

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

/// Size of the full on-flash storage image (header + MAX_PRESETS presets, rounded up to a
/// whole number of sectors). The SysEx editor transfer is sized to this. 16 + 128*200 =
/// 25616 -> 28672 (7 sectors). Must equal the editor's `STORAGE_SIZE` in picoDSP-Edit.
pub const STORAGE_IMAGE_SIZE: usize =
    sectors_ceil(HEADER_SIZE + (MAX_PRESETS as u32) * 200) as usize;

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

    /// Read the on-flash header and report whether a (re)format is needed (missing magic or a
    /// version bump). Does NOT write — the actual `format()` is deferred until core1 is running
    /// (see the boot-ordering note on `format`). A single flash read is safe pre-core1.
    pub async fn needs_format(&mut self) -> bool {
        let mut buf = [0u8; 16];
        self.flash.read(ADDR_OFFSET, &mut buf).await.unwrap();
        let header: StorageHeader = unsafe { core::ptr::read(buf.as_ptr() as *const _) };
        let stale = header.magic != MAGIC || header.version != VERSION;
        if stale {
            log_storage!("Storage version mismatch — reformat pending.\r\n");
        } else {
            log_storage!("Storage OK. Found {} presets.\r\n", header.num_presets);
        }
        stale
    }

    /// Rewrite the whole factory bank. MUST be called with core1 spawned and the audio drain
    /// loop running (i.e. from `midi_task`, like the runtime ResetStorage command), NOT bare at
    /// boot. The 128-preset bank spans 7 flash sectors; every embassy flash op pauses core1 over
    /// the SIO FIFO. Before core1 is launched that pause is "answered" only by the bootrom's FIFO
    /// echo, which desyncs after a handful of ops and hangs mid-format. With core1 running,
    /// `write_raw`'s per-sector gate→erase→write→reinit cycle is the proven, deadlock-safe path.
    pub async fn format(&mut self) {
        log_storage!("Formatting storage area...\r\n");

        let defaults = get_default_presets();
        let num_presets = defaults.len();
        let preset_size = core::mem::size_of::<Preset>();
        debug_assert!(num_presets <= MAX_PRESETS);

        // Build the whole header+presets image (rounded up to a sector multiple, zero-padded)
        // on the heap. This spans as many 4 KB sectors as the bank needs.
        let used = HEADER_SIZE as usize + num_presets * preset_size;
        let image_len = sectors_ceil(used as u32) as usize;

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

        self.write_raw(&image).await;

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
        // Write the image ONE 4 KB sector at a time, each as a fully independent, proven-safe
        // cycle: park core1 off PSRAM (our gate) -> erase 1 sector -> write 1 sector ->
        // settle + full PSRAM reinit -> release core1. A full 128-preset bank spans 7 sectors.
        //
        // Why per-sector with a *complete* cycle (not one gate around all sectors + one final
        // reinit): the reinit's `Psram::new` does its own `multicore::pause_core1()`. Holding
        // core1 in our gate spin across many embassy flash-op pause/resume cycles and then
        // nesting reinit's pause deadlocks (see docs/psram-flash-corruption-investigation.md).
        // Releasing core1 between sectors returns it to a known-good thread-mode state, so each
        // sector reuses the exact single-sector path that's been verified safe.
        let total = sectors_ceil(data.len() as u32).min(STORAGE_SIZE);
        log_storage!(
            "Flash write: {} bytes -> {} sectors\r\n",
            data.len(),
            total / SECTOR_SIZE
        );
        let mut off = 0u32;
        while off < total {
            crate::psram::lock_core1_off_psram().await;

            let sec = ADDR_OFFSET + off;
            // Barriers around each embassy flash op: erase/write internally pause+resume core1
            // over the SIO FIFO, and the following `after_flash_write` -> `Psram::new` pauses it
            // again. Without a barrier between them the second pause can fire before core1's
            // resume from the first is globally visible, nesting the FIFO handshake into a
            // deadlock. (A stray defmt log here masked it; make the ordering explicit instead.)
            cortex_m::asm::dsb();
            self.flash.erase(sec, sec + SECTOR_SIZE).await.unwrap();
            cortex_m::asm::dsb();

            // Full-sector buffer; pad the tail of a partial final sector with zeros.
            let mut sector = [0u8; SECTOR_SIZE as usize];
            let src = off as usize;
            if src < data.len() {
                let n = (data.len() - src).min(SECTOR_SIZE as usize);
                sector[..n].copy_from_slice(&data[src..src + n]);
            }
            self.flash.write(sec, &sector).await.unwrap();
            cortex_m::asm::dsb();

            crate::psram::after_flash_write();
            crate::psram::unlock_core1();
            off += SECTOR_SIZE;
        }

        log_storage!("Flash write: Done.\r\n");
    }
}
