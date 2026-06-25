MEMORY {
    /*
     * The RP2350 has either external or internal flash.
     *
     * 2 MiB is a safe default here, although a Pico 2 has 4 MiB.
     */
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    /*
     * RAM consists of 8 banks, SRAM0-SRAM7, with a striped mapping.
     * This is usually good for performance, as it distributes load on
     * those banks evenly.
     */
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
    /*
     * RAM banks 8 and 9 use a direct mapping. They can be used to have
     * memory areas dedicated for some specific job, improving predictability
     * of access times.
     * Example: Separate stacks for core0 and core1.
     */
    SRAM4 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM5 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    /* ### Boot ROM info
     *
     * Goes after .vector_table, to keep it in the first 4K of flash
     * where the Boot ROM (and picotool) can find it
     */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH

} INSERT AFTER .vector_table;

/* move .text to start /after/ the boot info */
_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    /* ### Picotool 'Binary Info' Entries
     *
     * Picotool looks through this block (as we have pointers to it in our
     * header) to find interesting information.
     */
    .bi_entries : ALIGN(4)
    {
        /* We put this in the header */
        __bi_entries_start = .;
        /* Here are the entries */
        KEEP(*(.bi_entries));
        /* Keep this block a nice round size */
        . = ALIGN(4);
        /* We put this in the header */
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    /* ### Boot ROM extra info
     *
     * Goes after everything in our program, so it can contain a signature.
     */
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH

} INSERT AFTER .uninit;

SECTIONS {
    .sram4 (NOLOAD) : ALIGN(4) {
        *(.sram4 .sram4.*);
        . = ALIGN(4);
    } > SRAM4

    .sram5 (NOLOAD) : ALIGN(4) {
        *(.sram5 .sram5.*);
        . = ALIGN(4);
    } > SRAM5
}

/* RAM-resident code: gather the hot code paths that share the QMI/XIP bus into
 * SRAM so their instruction fetches don't contend with PSRAM data access (the
 * dominant source of DSP timing spikes). Loaded from FLASH and copied to RAM at
 * boot by init_ram_code() before core1 / the BT task run.
 *
 *  - *cyw43*           : the BLE host runner's busy-poll on core0 (also *cyw43_pio*).
 *  - *infinitedsp_core*: the whole DSP chain on core1 — incl. monomorphised
 *                        DspChain/ParallelMixer/Bypass glue and our own
 *                        MoogOscillatorSection / Midi* shims, whose symbols all
 *                        carry "infinitedsp_core" in their `impl FrameProcessor`
 *                        type paths. (PsramDelay::process is already RAM-resident
 *                        via .data.ram_func.)
 *  - *libm*            : per-sample expf (ADSR) and sinf (LFO) range reduction.
 *
 * NOTE: only .text is gathered; rodata jump tables (.Lswitch.table.*) stay in
 * flash, a small residual. */
SECTIONS {
    .ram_code : ALIGN(4) {
        . = ALIGN(4);
        __sram_code_start = .;
        *(.text.*cyw43*)
        *(.text.*infinitedsp_core*)
        *(.text.*libm*)
        /* BLE host stack HOT path: runs on core0 while the radio is on (scanning adv
         * reports even when not connected, GATT/ATT notifications when connected). Its XIP
         * instruction fetches contend with the core1 PSRAM delay over the QMI bus -> audio
         * clicks. We relocate the per-event hot modules but deliberately NOT
         * trouble_host::security_manager (~53 KB, only runs once per pairing — cold). Module
         * prefixes are legacy-mangled "<crate-len>trouble_host<mod-len><mod>". */
        *(.text.*bt_hci*)
        *(.text.*trouble_host4host*)            /* the Runner / HCI event dispatch */
        *(.text.*trouble_host3att*)             /* ATT (GATT notifications) */
        *(.text.*trouble_host15channel_manager*) /* L2CAP channels */
        *(.text.*trouble_host18connection_manager*)
        *(.text.*trouble_host11packet_pool*)
        *(.text.*trouble_host7central*)         /* scan / connect */
        *(.text.*trouble_host10connection*)
        *(.text.*trouble_host4gatt*)
        *(.text.*trouble_host5l2cap*)
        *(.text.*trouble_host3pdu*)
        *(.text.*trouble_host5codec*)
        *(.text.*trouble_host6cursor*)
        *(.text.*trouble_host4scan*)
        /* MIDI hot path in this crate: midi_task + handle_voice_message + NoteStack +
         * midi_to_freq run per MIDI message (USB and BLE) on core0. (parse_ble_midi is
         * tagged .data.ram_func directly.) */
        *(.text.*rp2350_synth7control4midi*)
        /* Embassy runtime on the per-poll/per-wake hot path (both cores' executors, the
         * cyw43 runner's select4 + yield_now every iteration, channel ops, the poll timer,
         * and the PIO/DMA driving cyw43 SPI + I2S out). All ran from flash -> XIP fetches
         * contend with the core1 PSRAM delay over the QMI bus. */
        *(.text.*embassy_futures*)
        *(.text.*embassy_executor*)
        *(.text.*embassy_sync*)
        *(.text.*embassy_time*)
        *(.text.*embassy_rp3pio*)
        *(.text.*embassy_rp3dma*)
        *(.text.*embassy_rp12pio_programs*)
        /* rodata jump tables for the same hot paths (read over XIP during
         * match-arm dispatch); pulled to RAM to remove the last QMI data reads. */
        *(.rodata..Lswitch.table.*cyw43*)
        *(.rodata..Lswitch.table.*infinitedsp_core*)
        *(.rodata..Lswitch.table.*libm*)
        *(.rodata..Lswitch.table.*bt_hci*)
        *(.rodata..Lswitch.table.*trouble_host4host*)
        *(.rodata..Lswitch.table.*trouble_host3att*)
        *(.rodata..Lswitch.table.*trouble_host15channel_manager*)
        /* Select/Join future poll dispatch tables, read over XIP every poll iteration. */
        *(.rodata..Lswitch.table.*embassy_futures*)
        *(.rodata..Lswitch.table.*embassy_executor*)
        *(.rodata..Lswitch.table.*embassy_sync*)
        *(.rodata..Lswitch.table.*rp2350_synth7control4midi*)
        . = ALIGN(4);
        __sram_code_end = .;
    } > RAM AT > FLASH
    __sram_code_load = LOADADDR(.ram_code);
} INSERT AFTER .data;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
