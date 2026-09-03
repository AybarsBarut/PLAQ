use plaq_format::{PlaqHeader, canonical_pcm_sha256, decode_from_slice, encode_to_vec};

#[derive(Clone, Copy)]
enum Signal {
    Silence,
    Impulse,
    Sweep,
    WhiteNoise,
    PinkNoise,
    LowAmplitude,
    MaximumAmplitude,
}

fn mono_signal(kind: Signal, frames: usize, peak: i32) -> Vec<i32> {
    let mut state = 0x504c_4151_u32;
    let mut pink = [0_i64; 5];
    (0..frames)
        .map(|index| match kind {
            Signal::Silence => 0,
            Signal::Impulse => {
                if index == frames / 2 {
                    peak
                } else {
                    0
                }
            }
            Signal::Sweep => {
                let phase = index as f64 * index as f64 * 0.000_19;
                (phase.sin() * f64::from(peak) * 0.8) as i32
            }
            Signal::WhiteNoise => {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                ((state as i32) >> 2).clamp(-peak, peak)
            }
            Signal::PinkNoise => {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = i64::from(state as i32);
                let bucket = index.trailing_zeros().min(4) as usize;
                pink[bucket] = white;
                (pink.iter().sum::<i64>() / (5 * (i64::from(i32::MAX) / i64::from(peak))))
                    .clamp(i64::from(-peak), i64::from(peak)) as i32
            }
            Signal::LowAmplitude => (index as i32 % 5) - 2,
            Signal::MaximumAmplitude => {
                if index.is_multiple_of(2) {
                    peak
                } else {
                    -peak - 1
                }
            }
        })
        .collect()
}

#[test]
fn required_signal_and_format_matrix_is_bit_perfect() {
    let signals = [
        Signal::Silence,
        Signal::Impulse,
        Signal::Sweep,
        Signal::WhiteNoise,
        Signal::PinkNoise,
        Signal::LowAmplitude,
        Signal::MaximumAmplitude,
    ];
    for bits in [16_u8, 24] {
        let peak = (1_i32 << (bits - 1)) - 1;
        for channels in [1_u8, 2] {
            for sample_rate in [8_000_u32, 44_100, 96_000] {
                for signal in signals {
                    let mono = mono_signal(signal, 513, peak);
                    let samples = if channels == 1 {
                        mono
                    } else {
                        mono.iter()
                            .enumerate()
                            .flat_map(|(index, &sample)| {
                                [sample, if index % 3 == 0 { sample } else { sample / 2 }]
                            })
                            .collect()
                    };
                    let hash = canonical_pcm_sha256(&samples, bits).unwrap();
                    let header = PlaqHeader::lossless(channels, bits, sample_rate, 513, 128, hash);
                    let (encoded, _) = encode_to_vec(&header, &samples).unwrap();
                    let decoded = decode_from_slice(&encoded).unwrap();
                    assert_eq!(decoded.samples, samples);
                    assert_eq!(decoded.header.pcm_sha256, hash);
                }
            }
        }
    }
}
