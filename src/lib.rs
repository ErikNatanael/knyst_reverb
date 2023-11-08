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
            let time_min = (max_delay_length_in_samples / CHANNELS * i) as usize;
            let time_max = max_delay_length_in_samples / CHANNELS * (i + 1);
            StaticSampleDelay::new(rng.gen_range(time_min..time_max))
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
        input: &Vec<[Sample; CHANNELS]>,
        output: &mut Vec<[Sample; CHANNELS]>,
    ) {
        let block_size = input.len();
        for f in 0..block_size {
            let in_frame = &input[f];
            let out_frame = &mut output[f];
            // Get the output of the delay
            let mut sig = [0.0; CHANNELS];
            for i in 0..CHANNELS {
                sig[i] = self.delays[i].read() * self.flip_polarity[i];
                self.delays[i].write(in_frame[i]);
            }
            let mut sig2 = [0.0; CHANNELS];
            // Apply Hadamard matrix
            for row in 0..CHANNELS {
                for column in 0..CHANNELS {
                    // TODO: Vectorise
                    sig2[row] += sig[column] * self.hadamard_matrix[row][column];
                }
            }
            out_frame.copy_from_slice(&sig2);
        }
    }
}

/// Tail block of a reverb. Simply a relatively long feedback delay.
struct Tail<const CHANNELS: usize> {
    feedback_gain: Sample,
    /// Size is the length of the delay
    delay_buffer: Vec<[Sample; CHANNELS]>,
    /// One block of samples
    process_temp_buffers: Vec<[Sample; CHANNELS]>,
    // in samples
    buffer_write_index: usize,
    buffer_read_index: usize,
}

impl<const CHANNELS: usize> Tail<CHANNELS> {
    pub fn new(delay_length_in_samples: usize, feedback: Sample) -> Self {
        Self {
            feedback_gain: feedback,
            delay_buffer: vec![[0.0; CHANNELS]; delay_length_in_samples],
            process_temp_buffers: vec![],
            buffer_write_index: 0,
            buffer_read_index: delay_length_in_samples,
        }
    }
    /// Init internal buffers to the block size. Not real time safe.
    pub fn init(&mut self, block_size: usize) {
        self.process_temp_buffers = vec![[0.0; CHANNELS]; block_size];
    }
    pub fn process_block(
        &mut self,
        input: &Vec<[Sample; CHANNELS]>,
        output: &mut Vec<[Sample; CHANNELS]>,
    ) {
        // Get the output of the delay
        let block_size = input.len();
        assert!(self.delay_buffer.len() >= block_size);
        let read_end = self.buffer_read_index + block_size;
        if read_end <= self.delay_buffer.len() {
            self.process_temp_buffers
                .copy_from_slice(&self.delay_buffer[self.buffer_read_index..read_end]);
        } else {
            // block wraps around
            let read_end = read_end % self.delay_buffer.len();
            self.process_temp_buffers[0..block_size - read_end]
                .copy_from_slice(&self.delay_buffer[self.buffer_read_index..]);
            self.process_temp_buffers[block_size - read_end..]
                .copy_from_slice(&self.delay_buffer[0..read_end]);
        }
        // Set output to the output of the delay
        output.copy_from_slice(&self.process_temp_buffers);
        // apply feedback to output of delay
        for channel in &mut self.process_temp_buffers {
            for i in 0..CHANNELS {
                channel[i] *= self.feedback_gain;
            }
        }
        // TODO: Combine gain and matrix
        // mix matrix, householder
        // todo!("Mix householder");
        // add together with input
        for (process_channel, input_channel) in self.process_temp_buffers.iter_mut().zip(input) {
            for i in 0..CHANNELS {
                process_channel[i] += input_channel[i];
            }
        }
        // Pipe back into the delay

        let write_end = self.buffer_write_index + block_size;
        if write_end <= self.delay_buffer.len() {
            self.delay_buffer[self.buffer_write_index..write_end]
                .copy_from_slice(&self.process_temp_buffers);
        } else {
            // block wraps around
            let write_end = write_end % self.delay_buffer.len();
            self.delay_buffer[self.buffer_write_index..]
                .copy_from_slice(&self.process_temp_buffers[0..block_size - write_end]);
            self.delay_buffer[0..write_end]
                .copy_from_slice(&self.process_temp_buffers[block_size - write_end..]);
        }

        // Move delay pointers
        self.buffer_read_index = (self.buffer_read_index + block_size) % self.delay_buffer.len();
        self.buffer_write_index = (self.buffer_write_index + block_size) % self.delay_buffer.len();
    }
    /// Max delay time in samples
    fn max_delay_time(&self) -> usize {
        self.delay_buffer.len()
    }
}

const CHANNELS: usize = 8;
const DIFFUSERS: usize = 6;
pub struct LuffVerb {
    diffusers: [Diffuser<CHANNELS>; DIFFUSERS],
    tail: Tail<CHANNELS>,
    buffer0: Vec<[Sample; CHANNELS]>,
    buffer1: Vec<[Sample; CHANNELS]>,
}
#[impl_gen]
// impl<const DIFFUSERS: usize, const CHANNELS: usize> LuffVerb<{DIFFUSERS}, {CHANNELS}> {
impl LuffVerb {
    pub fn new(tail_delay: usize) -> Self {
        let diffusers = std::array::from_fn(|i| Diffuser::new(tail_delay / DIFFUSERS));
        Self {
            diffusers,
            tail: Tail::new(tail_delay, 0.7),
            buffer0: Vec::new(),
            buffer1: Vec::new(),
        }
    }
    pub fn init(&mut self, block_size: BlockSize) {
        self.buffer0 = vec![[0.0; CHANNELS]; *block_size];
        self.buffer1 = vec![[0.0; CHANNELS]; *block_size];
    }
    pub fn process(&mut self, input: &[Sample], output: &mut [Sample]) -> GenState {
        // Use buffer0 and buffer1 as input and output buffers every other time to cut down on the number of buffers needed.
        let mut in_buf = &mut self.buffer0;
        let mut out_buf = &mut self.buffer1;
        // Fill all channels of buffer0 with the input
        for (&in_sample, channel) in input.iter().zip(in_buf.iter_mut()) {
            channel.fill(in_sample);
        }
        for diffuser in &mut self.diffusers {
            diffuser.process_block(&in_buf, &mut out_buf);
            std::mem::swap(in_buf, out_buf);
        }
        self.tail.process_block(&in_buf, &mut out_buf);
        // Sum output channels
        for (out_sample, out_channel) in output.iter_mut().zip(out_buf) {
            *out_sample = out_channel.iter().sum();
        }
        GenState::Continue
    }
}

// 1. Separate Tails, one per channel, each processing a block, into a multichannel mix matrix which scrambles the channels
// 2. Process each

// At low channel counts, processing one tail per channel may be more efficient. But on a system with poor cpu perf SIMD won't have large registers anyway.

#[cfg(test)]
mod tests {
    use crate::{hadamard, Tail};

    #[test]
    fn tail_delay() {
        let block_size = 16;
        let mut tail = Tail::<1>::new(block_size * 2 + 1, 1.0);
        tail.init(block_size);
        let mut output = vec![[0.0; 1]; block_size];
        let mut input = vec![[0.0; 1]; block_size];
        input[0][0] = 1.0;
        tail.process_block(&input, &mut output);
        assert_eq!(output[0][0], 0.0);
        tail.process_block(&input, &mut output);
        assert_eq!(output[0][0], 0.0);
        tail.process_block(&input, &mut output);
        assert_eq!(output[0][0], 0.0);
        assert_eq!(output[1][0], 1.0);
        assert_eq!(output[2][0], 0.0);
    }
    #[test]
    fn test_hadamard() {
        let _m1 = hadamard::<1>();
        let _m2 = hadamard::<2>();
        let _m4 = hadamard::<4>();
        let _m16 = hadamard::<16>();
    }
}
