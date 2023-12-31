

use knyst::{
    gen::filter::one_pole::*,
    prelude::{
        delay::{SampleDelay, StaticSampleDelay},
        impl_gen, Gen, GenState,
    },
    xorrng::XOrShift32Rng,
    BlockSize, Sample, SampleRate,
};
use rand::{seq::SliceRandom, thread_rng, Rng};
struct Diffuser<const CHANNELS: usize> {
    delays: [StaticSampleDelay; CHANNELS],
    flip_polarity: [Sample; CHANNELS],
}

/// Produces hadamard matrices for powers of 2. 
///
/// # Panic
/// Panics if N is not a power of 2
fn hadamard<const N: usize>() -> [[Sample; N]; N] {
    let mut matrix = [[0.0; N]; N];
    // Assert that N is a power of 2
    assert_eq!(N & (N - 1), 0);
    matrix[0][0] = 1.0;
    let mut k = 1;
    while k < N {
        for i in 0..k {
            for j in 0..k {
                matrix[i + k][j] = matrix[i][j];
                matrix[i][j + k] = matrix[i][j];
                matrix[i + k][j + k] = -matrix[i][j];
            }
        }
        k += k;
    }
    matrix
}

// TODO: CHange from tail to diffuser logic
impl<const CHANNELS: usize> Diffuser<CHANNELS> {
    pub fn new(max_delay_length_in_samples: usize) -> Self {
        let mut rng = thread_rng();
        let mut flip_polarity = [-1.0; CHANNELS];
        flip_polarity[CHANNELS / 2..].fill(1.);
        flip_polarity.shuffle(&mut rng);
        let delays = std::array::from_fn(|i| {
            let time_min = (max_delay_length_in_samples / CHANNELS * i) as usize + 1;
            let time_max = max_delay_length_in_samples / CHANNELS * (i + 1);
            let delay_time = rng.gen_range(time_min..time_max);
            StaticSampleDelay::new(delay_time)
        });

        Self {
            flip_polarity,
            delays,
        }
    }
    /// Init internal buffers to the block size. Not real time safe.
    pub fn init(&mut self, block_size: usize) {}
    pub fn process_block(
        &mut self,
        input: &[Vec<Sample>; CHANNELS],
        output: &mut [Vec<Sample>; CHANNELS],
    ) {
        let block_size = input[0].len();
        for f in 0..block_size {
            // Get the output of the delay
            let mut sig = [0.0; CHANNELS];
            for channel in 0..CHANNELS {
                sig[channel] = self.delays[channel].read() * self.flip_polarity[channel];
                self.delays[channel].write_and_advance(input[channel][f]);
            }
            matrix::hadamard_recursive(&mut sig);
            // let mut sig2 = [0.0; CHANNELS];
            // // Apply Hadamard matrix
            // for row in 0..CHANNELS {
            //     for column in 0..CHANNELS {
            //         // TODO: Vectorise
            //         sig2[row] += sig[column] * self.hadamard_matrix[row][column];
            //     }
            // }
            for channel in 0..CHANNELS {
                output[channel][f] = sig[channel];
                // output[channel][f] = input[channel][f];
            }
        }
    }
}

/// Tail block of a reverb. Simply a relatively long feedback delay.
struct Tail<const CHANNELS: usize> {
    feedback_gain: Sample,
    /// Size is the length of the delay
    delays: [StaticSampleDelay; CHANNELS],
    lowpasses: [OnePoleLpf; CHANNELS],
    /// One block of samples
    process_temp_buffers: [Vec<Sample>; CHANNELS],
    process_temp_buffers1: [Vec<Sample>; CHANNELS],
}

impl<const CHANNELS: usize> Tail<CHANNELS> {
    pub fn new(delay_length_in_samples: usize, feedback: Sample) -> Self {
        let time_min = delay_length_in_samples / 10;
        let time_max = delay_length_in_samples;
        let mut rng = thread_rng();
        let delays = std::array::from_fn(|i| {
            let delay_time = rng.gen_range(time_min..time_max);
            StaticSampleDelay::new(delay_time)
        });
        let lowpasses = std::array::from_fn(|_| OnePoleLpf::new());
        Self {
            feedback_gain: feedback,
            process_temp_buffers: std::array::from_fn(|_| vec![0.0; 0]),
            process_temp_buffers1: std::array::from_fn(|_| vec![0.0; 0]),
            delays,
            lowpasses,
        }
    }
    /// Init internal buffers to the block size. Not real time safe.
    pub fn init(&mut self, block_size: usize) {
        self.process_temp_buffers = std::array::from_fn(|_| vec![0.0; block_size]);
        self.process_temp_buffers1 = std::array::from_fn(|_| vec![0.0; block_size]);
    }
    pub fn process_block(
        &mut self,
        input: &[Vec<Sample>; CHANNELS],
        output: &mut [Vec<Sample>; CHANNELS],
        damping: &[Sample],
        sample_rate: SampleRate,
    ) {
        // Get the output of the delay
        for (i, delay) in self.delays.iter_mut().enumerate() {
            delay.read_block(&mut self.process_temp_buffers[i]);
        }
        // Set output to the output of the delay
        for (output_channel, process_channel) in output.iter_mut().zip(&self.process_temp_buffers) {
            output_channel.copy_from_slice(process_channel);
        }
        // TODO: Combine gain and matrix
        // Apply Hadamard matrix
        let block_size = input[0].len();
        for f in 0..block_size {
            let mut chan = [0.0; CHANNELS];
            for (c, channel) in self.process_temp_buffers.iter().enumerate() {
                chan[c] = channel[f];
            }
            matrix::Householder::in_place(&mut chan);
            for (c, channel) in self.process_temp_buffers.iter_mut().enumerate() {
                channel[f] = chan[c];
            }
        }
        // apply feedback to output of delay
        for (i, channel) in self.process_temp_buffers.iter_mut().enumerate() {
            for sample in channel.iter_mut() {
                *sample *= self.feedback_gain;
            }
            self.lowpasses[i].process(
                sample_rate,
                channel,
                damping,
                &mut self.process_temp_buffers1[i],
            );
        }
        // add together with input
        for (process_channel, input_channel) in self.process_temp_buffers1.iter_mut().zip(input) {
            for (process_s, input_s) in process_channel.iter_mut().zip(input_channel) {
                *process_s += *input_s;
            }
        }
        // Pipe back into the delay
        for (channel, delay) in self.delays.iter_mut().enumerate() {
            delay.write_block_and_advance(&self.process_temp_buffers1[channel]);
        }
    }
}

const CHANNELS: usize = 8;
const DIFFUSERS: usize = 4;
pub struct LuffVerb {
    diffusers: [Diffuser<CHANNELS>; DIFFUSERS],
    tail: Tail<CHANNELS>,
    input_lpf: OnePoleLpf,
    buffer0: [Vec<Sample>; CHANNELS],
    buffer1: [Vec<Sample>; CHANNELS],
}
#[impl_gen]
// impl<const DIFFUSERS: usize, const CHANNELS: usize> LuffVerb<{DIFFUSERS}, {CHANNELS}> {
impl LuffVerb {
    pub fn new(tail_delay: usize, feedback: Sample) -> Self {
        let diffusers = std::array::from_fn(|i| Diffuser::new(tail_delay / (DIFFUSERS * 2)));
        Self {
            diffusers,
            tail: Tail::new(tail_delay, feedback),
            buffer0: std::array::from_fn(|_| Vec::new()),
            buffer1: std::array::from_fn(|_| Vec::new()),
            input_lpf: OnePoleLpf::new(),
        }
    }
    pub fn init(&mut self, block_size: BlockSize) {
        self.buffer0 = std::array::from_fn(|_| vec![0.0; *block_size]);
        self.buffer1 = std::array::from_fn(|_| vec![0.0; *block_size]);
        self.tail.init(*block_size);
        for d in &mut self.diffusers {
            d.init(*block_size);
        }
    }
    pub fn process(
        &mut self,
        input: &[Sample],
        output: &mut [Sample],
        lowpass: &[Sample],
        damping: &[Sample],
        sample_rate: SampleRate,
    ) -> GenState {
        // Use buffer0 and buffer1 as input and output buffers every other time to cut down on the number of buffers needed.
        let mut in_buf = &mut self.buffer0;
        let mut out_buf = &mut self.buffer1;

        self.input_lpf.process(sample_rate, input, lowpass, output);
        // Fill all channels of buffer0 with the in,
        for channel in in_buf.iter_mut() {
            channel.copy_from_slice(output);
        }
        for (i, diffuser) in &mut self.diffusers.iter_mut().enumerate() {
            diffuser.process_block(in_buf, out_buf);
            std::mem::swap(&mut in_buf, &mut out_buf);
        }
        std::mem::swap(&mut in_buf, &mut out_buf);
        output.fill(0.0);
        let early_reflections_amount = 0.5;
        for (f, out_sample) in output.iter_mut().enumerate() {
            for channel in out_buf.iter_mut() {
                *out_sample += channel[f];
            }
            *out_sample *= early_reflections_amount;
        }
        std::mem::swap(&mut in_buf, &mut out_buf);
        self.tail
            .process_block(in_buf, out_buf, damping, sample_rate);
        // Sum output channels
        let compensation_amp = 1.0 / (CHANNELS as Sample * DIFFUSERS as Sample);
        for (f, out_sample) in output.iter_mut().enumerate() {
            for channel in out_buf.iter_mut() {
                *out_sample += channel[f];
            }
            *out_sample *= compensation_amp;
        }
        // assert_eq_slices(output, &out_buf[0]);
        GenState::Continue
    }
}

fn assert_eq_slices(s0: &[Sample], s1: &[Sample]) {
    for (v0, v1) in s0.iter().zip(s1) {
        assert_eq!(*v0, *v1);
    }
}

mod matrix {
    use std::marker::PhantomData;

    use knyst::Sample;

    pub fn hadamard_recursive(frame: &mut [Sample]) {
        if frame.len() <= 1 {
            return;
        }
        let d = frame.len() / 2;
        hadamard_recursive(&mut frame[..d]);
        hadamard_recursive(&mut frame[d..]);
        for i in 0..d {
            let a = frame[i];
            let b = frame[i + d];
            frame[i] = a + b;
            frame[i + d] = a - b;
        }
    }

    pub struct Householder<const CHANNELS: usize> {
        _channels: PhantomData<[(); CHANNELS]>,
    }
    impl<const CHANNELS: usize> Householder<CHANNELS> {
        const MULTIPLIER: f64 = -2. / CHANNELS as f64;
        #[inline]
        pub fn in_place(frame: &mut [Sample; CHANNELS]) {
            let mut sum: f64 = 0.0;
            for f in frame.iter_mut() {
                sum += *f as f64;
            }
            sum *= Householder::<CHANNELS>::MULTIPLIER;
            for f in frame.iter_mut() {
                *f += sum as Sample;
            }
        }
    }
}

// 1. Separate Tails, one per channel, each processing a block, into a multichannel mix matrix which scrambles the channels
// 2. Process each

// At low channel counts, processing one tail per channel may be more efficient. But on a system with poor cpu perf SIMD won't have large registers anyway.

#[cfg(test)]
mod tests {

    // #[test]
    // fn tail_delay() {
    //     let block_size = 16;
    //     let mut tail = Tail::<1>::new(block_size * 2 + 1, 1.0);
    //     tail.init(block_size);
    //     let mut output = [vec![0.0; block_size]; 1];
    //     let mut input = [vec![0.0; block_size]; 1];
    //     input[0][0] = 1.0;
    //     tail.process_block(&input, &mut output);
    //     assert_eq!(output[0][0], 0.0);
    //     tail.process_block(&input, &mut output);
    //     assert_eq!(output[0][0], 0.0);
    //     tail.process_block(&input, &mut output);
    //     assert_eq!(output[0][0], 0.0);
    //     assert_eq!(output[0][1], 1.0, "{output:?}");
    //     assert_eq!(output[0][2], 0.0);
    // }
}