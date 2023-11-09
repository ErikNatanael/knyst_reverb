use std::time::SystemTime;

use knyst::{
    prelude::{
        delay::{SampleDelay, StaticSampleDelay},
        impl_gen, Gen, GenState,
    },
    xorrng::XOrShift32Rng,
    BlockSize, Sample,
};
use rand::{seq::SliceRandom, thread_rng, Rng};
struct Diffuser<const CHANNELS: usize> {
    delays: [StaticSampleDelay; CHANNELS],
    flip_polarity: [Sample; CHANNELS],
    hadamard_matrix: [[Sample; CHANNELS]; CHANNELS],
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
        let mut flip_polarity = [1.0; CHANNELS];
        flip_polarity[CHANNELS / 2..].fill(-1.);
        flip_polarity.shuffle(&mut rng);
        let delays = std::array::from_fn(|i| {
            let time_min = (max_delay_length_in_samples / CHANNELS * i) as usize + 1;
            let time_max = max_delay_length_in_samples / CHANNELS * (i + 1);
            let delay_time = rng.gen_range(time_min..time_max);
            dbg!(delay_time);
            StaticSampleDelay::new(delay_time)
        });

        Self {
            flip_polarity,
            delays,
            hadamard_matrix: hadamard(),
        }
    }
    /// Init internal buffers to the block size. Not real time safe.
    pub fn init(&mut self, block_size: usize) {}
    pub fn process_block(
        &mut self,
        input: &[Vec<Sample>; CHANNELS],
        output: &mut [Vec<Sample>; CHANNELS],
    ) {
        let block_size = input.len();
        for f in 0..block_size {
            // Get the output of the delay
            let mut sig = [0.0; CHANNELS];
            for channel in 0..CHANNELS {
                sig[channel] = self.delays[channel].read() * self.flip_polarity[channel];
                self.delays[channel].write(input[channel][f]);
            }
            let mut sig2 = [0.0; CHANNELS];
            // Apply Hadamard matrix
            for row in 0..CHANNELS {
                for column in 0..CHANNELS {
                    // TODO: Vectorise
                    sig2[row] += sig[column] * self.hadamard_matrix[row][column];
                }
            }
            for channel in 0..CHANNELS {
                output[channel][f] = sig2[channel];
            }
        }
    }
}

/// Tail block of a reverb. Simply a relatively long feedback delay.
struct Tail<const CHANNELS: usize> {
    feedback_gain: Sample,
    /// Size is the length of the delay
    delays: [StaticSampleDelay; CHANNELS],
    /// One block of samples
    process_temp_buffers: [Vec<Sample>; CHANNELS],
}

impl<const CHANNELS: usize> Tail<CHANNELS> {
    pub fn new(delay_length_in_samples: usize, feedback: Sample) -> Self {
        let time_min = delay_length_in_samples / 2;
        let time_max = delay_length_in_samples;
        let mut rng = thread_rng();
        let delays = std::array::from_fn(|i| {
            let delay_time = rng.gen_range(time_min..time_max);
            StaticSampleDelay::new(delay_time)
        });
        Self {
            feedback_gain: feedback,
            process_temp_buffers: std::array::from_fn(|_| vec![0.0; 0]),
            delays,
        }
    }
    /// Init internal buffers to the block size. Not real time safe.
    pub fn init(&mut self, block_size: usize) {
        self.process_temp_buffers = std::array::from_fn(|_| vec![0.0; block_size]);
    }
    pub fn process_block(
        &mut self,
        input: &[Vec<Sample>; CHANNELS],
        output: &mut [Vec<Sample>; CHANNELS],
    ) {
        // Get the output of the delay
        for (i, delay) in self.delays.iter_mut().enumerate() {
            delay.read_block(&mut self.process_temp_buffers[i]);
        }
        // Set output to the output of the delay
        for channel in 0..CHANNELS {
            output[channel].copy_from_slice(&self.process_temp_buffers[channel]);
        }
        // apply feedback to output of delay
        for i in 0..CHANNELS {
            for sample in &mut self.process_temp_buffers[i] {
                *sample *= self.feedback_gain;
            }
        }
        // TODO: Combine gain and matrix
        // mix matrix, householder
        // todo!("Mix householder");
        // add together with input
        for (process_channel, input_channel) in self.process_temp_buffers.iter_mut().zip(input) {
            for (process_s, input_s) in process_channel.iter_mut().zip(input_channel) {
                *process_s += *input_s;
            }
        }
        // Pipe back into the delay
        for (channel, delay) in self.delays.iter_mut().enumerate() {
            delay.write_block(&self.process_temp_buffers[channel]);
        }
    }
}

const CHANNELS: usize = 8;
const DIFFUSERS: usize = 8;
pub struct LuffVerb {
    diffusers: [Diffuser<CHANNELS>; DIFFUSERS],
    tail: Tail<CHANNELS>,
    buffer0: [Vec<Sample>; CHANNELS],
    buffer1: [Vec<Sample>; CHANNELS],
}
#[impl_gen]
// impl<const DIFFUSERS: usize, const CHANNELS: usize> LuffVerb<{DIFFUSERS}, {CHANNELS}> {
impl LuffVerb {
    pub fn new(tail_delay: usize) -> Self {
        let diffusers = std::array::from_fn(|i| Diffuser::new(tail_delay / DIFFUSERS));
        Self {
            diffusers,
            tail: Tail::new(tail_delay, 0.2),
            buffer0: std::array::from_fn(|_| Vec::new()),
            buffer1: std::array::from_fn(|_| Vec::new()),
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
    pub fn process(&mut self, input: &[Sample], output: &mut [Sample]) -> GenState {
        // Use buffer0 and buffer1 as input and output buffers every other time to cut down on the number of buffers needed.
        let mut in_buf = &mut self.buffer0;
        let mut out_buf = &mut self.buffer1;
        // Fill all channels of buffer0 with the input
        for channel in in_buf.iter_mut() {
            channel.copy_from_slice(input);
        }
        for diffuser in &mut self.diffusers {
            diffuser.process_block(in_buf, out_buf);
            std::mem::swap(in_buf, out_buf);
        }
        let early_reflections_amount = 0.3;
        for (out_sample, out_channel) in output.iter_mut().zip(out_buf.iter()) {
            *out_sample = out_channel.iter().sum::<f32>() * early_reflections_amount;
        }
        // self.tail.process_block(in_buf, out_buf);
        // // Sum output channels
        // let compensation_amp = 1.0 / CHANNELS as f32;
        // for (f, out_sample) in output.iter_mut().enumerate() {
        //     for channel in out_buf.iter_mut() {
        //         *out_sample += channel[f];
        //     }
        // }
        GenState::Continue
    }
}

// 1. Separate Tails, one per channel, each processing a block, into a multichannel mix matrix which scrambles the channels
// 2. Process each

// At low channel counts, processing one tail per channel may be more efficient. But on a system with poor cpu perf SIMD won't have large registers anyway.

#[cfg(test)]
mod tests {
    use crate::{hadamard, Tail};

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
    #[test]
    fn test_hadamard() {
        let _m1 = hadamard::<1>();
        let _m2 = hadamard::<2>();
        let _m4 = hadamard::<4>();
        let _m16 = hadamard::<16>();
    }
}
