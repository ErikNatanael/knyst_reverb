use knyst::Sample;
struct Diffuser {}

/// Tail block of a reverb. Simply a relatively long feedback delay.
struct Tail<const CHANNELS: usize> {
    feedback_gain: Sample,
    buffers: Vec<[Sample; CHANNELS]>,
    buffer_write_index: usize,
    // in samples
    max_delay_time: usize,
}

impl<const CHANNELS: usize> Tail<CHANNELS> {
    pub fn process_block(
        &mut self,
        input: &Vec<[Sample; CHANNELS]>,
        output: &mut Vec<[Sample; CHANNELS]>,
    ) {
        // Get the output of the delay

        // apply feedback
        // mix matrix, householder
    }
}
