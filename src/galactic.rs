//! Galactic reverb
//! 
//! ported from airwindows Galactic plugin
//! License: MIT
// Original code: Copyright (c) 2016 airwindows, Airwindows uses the MIT license
// Ported code: Copyright 2023 Erik Natanael Gustafsson

// .h
// Buffers a[A-M][R/L]
// feedback[A-D][R/L]
// iir[A-B][L/R]
// vibM[L/R], depthM, vibM
// parameters A-E

// All buffers are set up with a constant size. This is compensated for by scaling by the actual sample rate in the processing code.
// Scale all the delay lengths linearly by the bigness parameter.
//
// # Per sample:
// - If the input is very faint, use the fpd values instead (floating point dither, similar to the last output sample)
// - vibM cycles 0. - TAU, speed depending on drift (Detune) and the fpdL value last time it reset
// - set the fixed size delay (256 frames) to the inputSample at the current position
// - Get a sample from the aM buffer (lin interp)
// - Apply a lowpass filter to the output from the M delay (iirA variable)
// - Only calculate a new reverb sample once every 4 samples if SR is 44100*4

// Reverb sample:
// Set I-L delays for the input + respective feedback from last cycle for the opposite channel (left for right, right for left) 
// Get the output from I-L delays
// Set A-D delays to a mixing configuration of the I-L outputs e.g. I - (J+K+L);
// Same thing for E-H
// Feedback delays are this same mixing of the outputs of E-H
// For large sample rates, use linear interpolation to the new value, otherwise the sum of EFGH/8.
//
// Apply another lowpass to the reverbed value

// Apply float dither

use knyst::gen::GenState;
use knyst::gen::delay::StaticSampleDelay;
use knyst::{Sample, SampleRate};

pub struct Galactic {
  delays_left: [StaticSampleDelay; 12],
  delays_right: [StaticSampleDelay; 12],
  detune_delay_left: StaticSampleDelay,
  detune_delay_right: StaticSampleDelay,
  lowpass_pre: [Sample; 2],
  lowpass_post: [Sample; 2],
}

impl Galactic {
  pub fn new() -> Self {
    Self {
        delays_left: std::array::from_fn(|_| StaticSampleDelay::new(0)),
        delays_right: std::array::from_fn(|_| StaticSampleDelay::new(0)),
        detune_delay_left: StaticSampleDelay::new(0),
        detune_delay_right: StaticSampleDelay::new(0),
        lowpass_pre: [0., 0.],
        lowpass_post: [0., 0.],
    }
  }
  pub fn init(&mut self, sample_rate: SampleRate) {
    let delay_times = [6480, 3660, 1720, 680, 9700, 6000, 2320, 940, 15220, 8460, 4540, 3200];
    for (delay, time) in self.delays_left.iter_mut().zip(delay_times) {
      let time = (time as f32 / 44100.) * *sample_rate;
      *delay = StaticSampleDelay::new(time as usize);
    }
    for (delay, time) in self.delays_right.iter_mut().zip(delay_times) {
      let time = (time as f32 / 44100.) * *sample_rate;
      *delay = StaticSampleDelay::new(time as usize);
    }
    self.detune_delay_left = StaticSampleDelay::new((0.07054421768707483 * *sample_rate) as usize);
    self.detune_delay_right = StaticSampleDelay::new((0.07054421768707483 * *sample_rate) as usize);
    self.lowpass_pre = [0., 0.];
    self.lowpass_post = [0., 0.];

  }
  pub fn process(&mut self, size: &[Sample], replace: &[Sample], brightness: &[Sample], detune: &[Sample]) -> GenState {
    GenState::Continue
  }
}