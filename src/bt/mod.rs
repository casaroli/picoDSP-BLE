//! Bluetooth host for a BLE-MIDI keyboard, running on core0.
//!
//! Brings up the CYW43439 radio in Bluetooth mode, runs the TrouBLE host stack as a BLE
//! Central, scans for a peripheral advertising the MIDI-over-BLE service, connects, subscribes
//! to its MIDI characteristic, decodes the BLE-MIDI packet framing and forwards channel-voice
//! messages to [`BLE_MIDI_CHANNEL`], where `midi_task` consumes them through the same handler
//! as USB-MIDI.

use bt_hci::controller::ExternalController;
use bt_hci::param::LeAdvReportsIter;
use cyw43::aligned_bytes;
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::{info, warn};
use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_rp::Peri;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH2, PIN_23, PIN_24, PIN_25, PIN_29, PIO1, TRNG};
use embassy_rp::pio::Pio;
use embassy_rp::trng::Trng;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;
use trouble_host::prelude::*;

use crate::Irqs;
use crate::common::shared::BLE_MIDI_CHANNEL;
use crate::usb::logger::LED_SIGNAL_CHANNEL;

/// MIDI-over-BLE *service* UUID `03B80E5A-EDE8-4B33-A751-6CE34EC4C700`, stored as the
/// little-endian byte array BLE transmits (reverse of the canonical text form).
const MIDI_SERVICE_UUID: [u8; 16] = [
    0x00, 0xC7, 0xC4, 0x4E, 0xE3, 0x6C, 0x51, 0xA7, 0x33, 0x4B, 0xE8, 0xED, 0x5A, 0x0E, 0xB8, 0x03,
];
/// MIDI-over-BLE *I/O characteristic* UUID `7772E5DB-3868-4112-A1A9-F2669D106BF3` (little-endian).
const MIDI_CHAR_UUID: [u8; 16] = [
    0xF3, 0x6B, 0x10, 0x9D, 0x66, 0xF2, 0xA9, 0xA1, 0x12, 0x41, 0x68, 0x38, 0xDB, 0xE5, 0x72, 0x77,
];

/// HCI command slots for the external controller.
const BT_SLOTS: usize = 4;
/// We only ever talk to one keyboard at a time.
const CONNECTIONS_MAX: usize = 1;
/// Signalling + ATT + a spare CoC channel.
const L2CAP_CHANNELS_MAX: usize = 3;
/// Max GATT services we keep handles for during discovery.
const MAX_SERVICES: usize = 4;

/// Drives the onboard LED, which on the Pico Plus 2 W lives on CYW43 GPIO 0 (the bare RP2350
/// `PIN_25` the LED used to use is now the radio's SPI chip-select). Reuses the existing
/// note-on/off `LED_SIGNAL_CHANNEL` so the LED still blinks with played notes.
async fn led_loop(mut control: cyw43::Control<'static>) {
    loop {
        let state = LED_SIGNAL_CHANNEL.receive().await;
        control.gpio_set(0, state).await;
    }
}

/// Captures the address of the first peripheral advertising the MIDI service.
struct MidiScanHandler {
    found: Signal<CriticalSectionRawMutex, Address>,
}

impl MidiScanHandler {
    fn new() -> Self {
        Self {
            found: Signal::new(),
        }
    }

    fn reset(&self) {
        self.found.reset();
    }

    async fn wait(&self) -> Address {
        self.found.wait().await
    }
}

impl EventHandler for MidiScanHandler {
    fn on_adv_reports(&self, mut it: LeAdvReportsIter) {
        while let Some(Ok(report)) = it.next() {
            if adv_contains_uuid(report.data, &MIDI_SERVICE_UUID) {
                self.found
                    .signal(Address::new(report.addr_kind, report.addr));
            }
        }
    }
}

/// Returns true if the advertising payload lists `uuid` among its 128-bit service-class UUIDs
/// (AD types 0x06 incomplete / 0x07 complete).
fn adv_contains_uuid(mut data: &[u8], uuid: &[u8; 16]) -> bool {
    while data.len() >= 2 {
        let len = data[0] as usize;
        if len == 0 || len + 1 > data.len() {
            break;
        }
        let ad_type = data[1];
        let payload = &data[2..1 + len];
        if ad_type == 0x06 || ad_type == 0x07 {
            for chunk in payload.chunks(16) {
                if chunk.len() == 16 && chunk == uuid {
                    return true;
                }
            }
        }
        data = &data[1 + len..];
    }
    false
}

/// Decode a BLE-MIDI notification payload into channel-voice messages and forward them.
///
/// BLE-MIDI prefixes a header byte (timestamp high), then each MIDI message is preceded by a
/// timestamp-low byte; the status byte may be omitted for running status. We only forward
/// channel-voice messages (note/CC/pitch-bend/program); SysEx and system messages are ignored.
fn parse_ble_midi(packet: &[u8]) {
    if packet.len() < 3 {
        return;
    }
    let mut i = 1; // skip header (timestamp high)
    let mut status = 0u8;

    while i < packet.len() {
        // A byte with bit 7 set is a timestamp-low byte, optionally followed by a new status.
        if packet[i] & 0x80 != 0 {
            i += 1;
            if i >= packet.len() {
                break;
            }
            if packet[i] & 0x80 != 0 {
                status = packet[i];
                i += 1;
            }
        }

        let high = status & 0xF0;
        if status == 0 || high == 0xF0 {
            // No running status yet, or a system/SysEx message we don't forward: bail out to
            // avoid mis-parsing the remainder.
            break;
        }

        let needed = if high == 0xC0 || high == 0xD0 { 1 } else { 2 };
        if i + needed > packet.len() {
            break;
        }
        let d1 = packet[i];
        let d2 = if needed == 2 { packet[i + 1] } else { 0 };
        i += needed;
        let _ = BLE_MIDI_CHANNEL.try_send([status, d1, d2]);
    }
}

/// Pair (encrypt) the link using JustWorks security and wait for it to complete.
///
/// BLE-MIDI keyboards typically guard the MIDI I/O characteristic behind an encrypted link
/// (the CCCD subscribe otherwise returns ATT 0x05, Insufficient Authentication). Returns
/// `true` once the link is encrypted, `false` if pairing failed or the connection dropped.
async fn pair<C: Controller>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    conn: &Connection<'_, DefaultPacketPool>,
) -> bool {
    // Request *bonding* (not just an ephemeral pairing): keyboards generally reject a
    // NoBonding pairing request, which surfaces as the peer sending PairingFailed.
    if let Err(e) = conn.set_bondable(true) {
        warn!("[bt] set_bondable failed: {:?}", e);
        return false;
    }
    if let Err(e) = conn.request_security() {
        warn!("[bt] request_security failed: {:?}", e);
        return false;
    }
    loop {
        match conn.next().await {
            ConnectionEvent::PairingComplete { security_level, .. } => {
                info!("[bt] paired (security level {:?})", security_level);
                return true;
            }
            ConnectionEvent::PairingFailed(err) => {
                warn!("[bt] pairing failed: {:?}", err);
                return false;
            }
            ConnectionEvent::Disconnected { reason } => {
                warn!("[bt] disconnected during pairing: {:?}", reason);
                return false;
            }
            // The keyboard may ask to relax the connection parameters mid-pairing; accept
            // its preferred values so the link stays up.
            ConnectionEvent::RequestConnectionParams(req) => {
                let _ = req.accept(None, stack).await;
            }
            _ => {}
        }
    }
}

/// Discover the MIDI service/characteristic over an established connection, subscribe to
/// notifications and pump them into [`parse_ble_midi`] until the link drops.
async fn run_gatt<C: Controller>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    conn: &Connection<'_, DefaultPacketPool>,
) {
    let client = match GattClient::<_, DefaultPacketPool, MAX_SERVICES>::new(stack, conn).await {
        Ok(c) => c,
        Err(_) => {
            warn!("[bt] gatt client init failed");
            return;
        }
    };

    let work = async {
        let services = match client
            .services_by_uuid(&Uuid::new_long(MIDI_SERVICE_UUID))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!("[bt] service discovery failed: {:?}", e);
                return;
            }
        };
        let Some(service) = services.first().cloned() else {
            warn!("[bt] MIDI service not found");
            return;
        };
        let ch: Characteristic<u8> = match client
            .characteristic_by_uuid(&service, &Uuid::new_long(MIDI_CHAR_UUID))
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("[bt] MIDI characteristic not found: {:?}", e);
                return;
            }
        };
        let mut listener = match client.subscribe(&ch, false).await {
            Ok(l) => l,
            Err(e) => {
                warn!("[bt] subscribe failed: {:?}", e);
                return;
            }
        };
        info!("[bt] BLE-MIDI subscribed");
        loop {
            let n = listener.next().await;
            parse_ble_midi(n.as_ref());
        }
    };

    // `client.task()` ends when the link drops; `select` then drops `work` so we can rescan.
    let _ = select(client.task(), work).await;
}

/// Core0 Bluetooth entry point: bring up CYW43, run the TrouBLE host, and keep a BLE-MIDI
/// keyboard connected, reconnecting on drop.
#[embassy_executor::task]
pub async fn bluetooth_task(
    pwr: Peri<'static, PIN_23>,
    cs: Peri<'static, PIN_25>,
    dio: Peri<'static, PIN_24>,
    clk: Peri<'static, PIN_29>,
    pio: Peri<'static, PIO1>,
    dma: Peri<'static, DMA_CH2>,
    trng: Peri<'static, TRNG>,
) {
    let fw = aligned_bytes!("../../cyw43-firmware/43439A0.bin");
    let btfw = aligned_bytes!("../../cyw43-firmware/43439A0_btfw.bin");
    let clm = aligned_bytes!("../../cyw43-firmware/43439A0_clm.bin");
    let nvram = aligned_bytes!("../../cyw43-firmware/nvram_rp2040.bin");

    let pwr = Output::new(pwr, Level::Low);
    let cs = Output::new(cs, Level::High);
    let mut pio = Pio::new(pio, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        dio,
        clk,
        embassy_rp::dma::Channel::new(dma, Irqs),
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (_net, bt_driver, mut control, runner) =
        cyw43::new_with_bluetooth(state, pwr, spi, fw, btfw, nvram).await;

    let run_cyw43 = runner.run();

    let app = async {
        control.init(clm).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        let controller = ExternalController::<_, BT_SLOTS>::new(bt_driver);
        let address = Address::random([0x4d, 0x49, 0x44, 0x49, 0x01, 0xff]);
        let mut resources =
            HostResources::<_, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>::new();
        // Seed the security manager's CSPRNG from the RP2350 hardware TRNG (CryptoRng).
        // We have no display or keypad, so advertise NoInputNoOutput → JustWorks pairing.
        let mut trng = Trng::new(trng, Irqs, embassy_rp::trng::Config::default());
        let stack = trouble_host::new(controller, &mut resources)
            .set_random_address(address)
            .set_random_generator_seed(&mut trng)
            .set_io_capabilities(IoCapabilities::NoInputNoOutput)
            .build();
        let mut runner = stack.runner();
        let handler = MidiScanHandler::new();

        let central_loop = async {
            let mut central = stack.central();
            loop {
                handler.reset();
                // Scan until a MIDI peripheral is found, then recover `central` to connect.
                // The scanner owns `central` while scanning; `into_inner` only runs once the
                // scan session has been dropped, so the borrows don't overlap.
                let mut scanner = Scanner::new(central);
                let target = loop {
                    // Duty-cycle the scan (window < interval) instead of listening 100% of
                    // the time. Continuous scanning keeps the radio + SPI maximally busy,
                    // which contends with the core1 DSP over the shared bus; 50 ms in every
                    // 1 s still finds an advertising keyboard within a few seconds.
                    let scan_cfg = ScanConfig {
                        active: true,
                        interval: Duration::from_secs(1),
                        window: Duration::from_millis(50),
                        ..Default::default()
                    };
                    match scanner.scan(&scan_cfg).await {
                        Ok(session) => {
                            info!("[bt] scanning for a BLE-MIDI keyboard…");
                            let addr = handler.wait().await;
                            drop(session);
                            break addr;
                        }
                        Err(_) => {
                            warn!("[bt] scan start failed");
                            Timer::after(Duration::from_secs(1)).await;
                        }
                    }
                };
                central = scanner.into_inner();

                info!("[bt] candidate found, connecting…");
                let conn_cfg = ConnectConfig {
                    connect_params: Default::default(),
                    scan_config: ScanConfig {
                        active: true,
                        filter_accept_list: core::slice::from_ref(&target),
                        ..Default::default()
                    },
                };
                match central.connect(&conn_cfg).await {
                    Ok(conn) => {
                        info!("[bt] connected");
                        // The keyboard's MIDI characteristic requires an encrypted link, so
                        // pair (JustWorks) before touching GATT. If pairing fails or the link
                        // drops we fall through and rescan rather than spinning on a 0x05
                        // (Insufficient Authentication) subscribe.
                        if pair(&stack, &conn).await {
                            run_gatt(&stack, &conn).await;
                        }
                        info!("[bt] disconnected, rescanning");
                    }
                    Err(_) => warn!("[bt] connect failed"),
                }
            }
        };

        join(
            led_loop(control),
            join(runner.run_with_handler(&handler), central_loop),
        )
        .await;
    };

    join(run_cyw43, app).await;
}
