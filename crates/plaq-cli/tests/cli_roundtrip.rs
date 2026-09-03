use std::process::Command;

#[test]
fn cli_encode_decode_verify_and_inspect() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.wav");
    let encoded = temp.path().join("audio.plaq");
    let output = temp.path().join("output.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&input, spec).unwrap();
    for index in 0..2_000 {
        let left = (((index as f64 * 0.11).sin()) * 20_000.0) as i16;
        let right = (((index as f64 * 0.07).sin()) * 12_000.0) as i16;
        writer.write_sample(left).unwrap();
        writer.write_sample(right).unwrap();
    }
    writer.finalize().unwrap();

    let plaq = env!("CARGO_BIN_EXE_plaq");
    assert!(
        Command::new(plaq)
            .arg("encode")
            .arg(&input)
            .arg(&encoded)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(plaq)
            .arg("decode")
            .arg(&encoded)
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(plaq)
            .arg("verify")
            .arg(&input)
            .arg(&encoded)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(plaq)
            .arg("inspect")
            .arg(&encoded)
            .arg("--json")
            .status()
            .unwrap()
            .success()
    );

    let source: Vec<i32> = hound::WavReader::open(&input)
        .unwrap()
        .samples::<i32>()
        .map(Result::unwrap)
        .collect();
    let decoded: Vec<i32> = hound::WavReader::open(&output)
        .unwrap()
        .samples::<i32>()
        .map(Result::unwrap)
        .collect();
    assert_eq!(source, decoded);
}
