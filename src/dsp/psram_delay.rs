//! Delay line whose ring buffer lives in external PSRAM.
//!
//! Faithful port of `infinitedsp_core::effects::time::delay::Delay` with the
//! parameters fixed to static values (which is how the synth uses them) and the
//! buffer backed by `crate::psram` instead of the SRAM heap. The per-sample
//! buffer access pattern — two interpolation reads + one write head store — is
//! identical to the stock delay, so swapping this in isolates the cost of
//! putting that traffic on the QMI/PSRAM bus.

use infinitedsp_core::FrameProcessor;
use infinitedsp_core::core::channels::Mono;

pub struct PsramDelay {
    buffer: &'static mut [f32],
    write_ptr: usize,
    delay_time: f32,
    feedback: f32,
    mix: f32,
    sample_rate: f32,
}

impl PsramDelay {
    /// `max_delay_seconds` sizes the ring buffer; `delay_time`/`feedback`/`mix`
    /// are static (seconds, 0..1, 0..1). The buffer is allocated from PSRAM.
    pub fn new(
        max_delay_seconds: f32,
        delay_time: f32,
        feedback: f32,
        mix: f32,
        sample_rate: f32,
    ) -> Self {
        let size = (max_delay_seconds * sample_rate) as usize;
        let buffer = crate::psram::alloc_f32_slice(size);
        Self {
            buffer,
            write_ptr: 0,
            delay_time,
            feedback,
            mix,
            sample_rate,
        }
    }
}

impl FrameProcessor<Mono> for PsramDelay {
    // RAM-resident: the delay's PSRAM reads go through the same 16 KiB XIP cache
    // as instruction fetch. Executing this loop from flash makes every inner
    // iteration's fetch evict the delay's cached data lines (measured ~340 ns/read).
    // Putting the function in SRAM removes flash fetch from the QMI bus during the
    // loop, so the two linear cursors stay cached. (`.data.ram_func` is copied to
    // RAM by cortex-m-rt at boot — same mechanism embassy's PSRAM driver uses.)
    #[unsafe(link_section = ".data.ram_func")]
    #[inline(never)]
    fn process(&mut self, buffer: &mut [f32], _sample_index: u64) {
        let len = self.buffer.len();
        if len == 0 {
            return;
        }
        let len_f = len as f32;
        let delay_samples = self.delay_time * self.sample_rate;
        let fb = self.feedback;
        let mix = self.mix;

        for sample in buffer.iter_mut() {
            let input = *sample;

            let read_ptr_f = self.write_ptr as f32 - delay_samples;
            let mut read_ptr_norm = read_ptr_f;
            while read_ptr_norm < 0.0 {
                read_ptr_norm += len_f;
            }
            while read_ptr_norm >= len_f {
                read_ptr_norm -= len_f;
            }

            let idx_a = read_ptr_norm as usize;
            let idx_b = (idx_a + 1) % len;
            let frac = read_ptr_norm - idx_a as f32;

            // PSRAM traffic: two reads (interpolation) + one write (write head).
            let delayed = self.buffer[idx_a] * (1.0 - frac) + self.buffer[idx_b] * frac;
            let next_val = input + delayed * fb;
            self.buffer[self.write_ptr] = next_val;

            *sample = input * (1.0 - mix) + delayed * mix;
            self.write_ptr = (self.write_ptr + 1) % len;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_ptr = 0;
    }
}
