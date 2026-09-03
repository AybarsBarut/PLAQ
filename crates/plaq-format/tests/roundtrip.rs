use plaq_format::{PlaqHeader, canonical_pcm_sha256, decode_from_slice, encode_to_vec};

fn round_trip(samples: Vec<i32>, channels: u8, bits: u8) {
    let hash = canonical_pcm_sha256(&samples, bits).unwrap();
    let header = PlaqHeader::lossless(
        channels,
        bits,
        48_000,
        (samples.len() / usize::from(channels)) as u64,
        127,
        hash,
    );
    let (encoded, _) = encode_to_vec(&header, &samples).unwrap();
    let decoded = decode_from_slice(&encoded).unwrap();
    assert_eq!(decoded.samples, samples);
    assert_eq!(decoded.header.pcm_sha256, hash);
}

#[test]
fn stereo_16_with_short_final_block() {
    let samples = (0..2_003)
        .flat_map(|index| {
            let value = ((index * 997) % 65_535) - 32_768;
            [value, value / -2]
        })
        .collect();
    round_trip(samples, 2, 16);
}

#[test]
fn mono_24_single_sample() {
    round_trip(vec![-8_388_608], 1, 24);
}

#[test]
fn empty_audio_is_valid() {
    round_trip(Vec::new(), 1, 16);
}

#[test]
fn corruption_is_rejected() {
    let samples = vec![0_i32; 512];
    let hash = canonical_pcm_sha256(&samples, 16).unwrap();
    let header = PlaqHeader::lossless(1, 16, 44_100, 512, 256, hash);
    let (mut encoded, _) = encode_to_vec(&header, &samples).unwrap();
    let last = encoded.len() - 1;
    encoded[last] ^= 0x40;
    assert!(decode_from_slice(&encoded).is_err());
}

#[test]
fn truncation_is_rejected_without_panic() {
    let samples = vec![1_i32, -1, 2, -2];
    let hash = canonical_pcm_sha256(&samples, 16).unwrap();
    let header = PlaqHeader::lossless(1, 16, 44_100, 4, 4, hash);
    let (encoded, _) = encode_to_vec(&header, &samples).unwrap();
    for length in 0..encoded.len() {
        assert!(decode_from_slice(&encoded[..length]).is_err());
    }
}
