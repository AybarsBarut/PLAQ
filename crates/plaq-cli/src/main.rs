#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use plaq_core::{
    simulation::{StylusParameters, simulate_axis},
    transform::{from_components, to_components},
};
use plaq_format::{
    CodecStats, PlaqHeader, canonical_pcm_sha256, decode_from_slice, encode_to_vec, hex_sha256,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "plaq",
    version,
    about = "Experimental physical-trajectory lossless audio codec"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Encode 16/24-bit mono/stereo PCM WAV to lossless PLAQ.
    Encode {
        input: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 4_096)]
        block_frames: u32,
    },
    /// Decode lossless PLAQ to a canonical PCM WAV.
    Decode { input: PathBuf, output: PathBuf },
    /// Decode and compare source PCM metadata and SHA-256.
    Verify { input: PathBuf, encoded: PathBuf },
    /// Validate and print container metadata, checksums, and predictors.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Measure PLAQ encode/decode and optionally invoke a local FLAC binary.
    Benchmark {
        input: PathBuf,
        #[arg(long, default_value = "wav,flac")]
        compare: String,
        #[arg(long)]
        json: bool,
    },
    /// Render waveform, virtual trajectory, residual histograms, and predictors.
    Visualize {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Run the separate, explicitly lossy physical stylus simulation.
    Simulate {
        input: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 0.02)]
        mass: f64,
        #[arg(long, default_value_t = 0.8)]
        compliance: f64,
        #[arg(long, default_value_t = 0.15)]
        damping: f64,
        #[arg(long, default_value_t = 1.0)]
        max_displacement: f64,
        #[arg(long, default_value_t = 50.0)]
        max_velocity: f64,
        #[arg(long, default_value_t = 0.0)]
        tracking_error: f64,
    },
    /// Send an existing `.plaq` file over the TCP or UDP demo transport.
    StreamSend {
        input: PathBuf,
        #[arg(long, value_enum)]
        transport: Transport,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 1)]
        stream_id: u32,
    },
    /// Receive a `.plaq` file; incomplete UDP data is never concealed.
    StreamRecv {
        output: PathBuf,
        #[arg(long, value_enum)]
        transport: Transport,
        #[arg(long)]
        connect: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = 3_000)]
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    Tcp,
    Udp,
}

#[derive(Debug)]
struct WavData {
    spec: hound::WavSpec,
    samples: Vec<i32>,
}

#[derive(Debug, Serialize)]
struct Inspection<'a> {
    format_version: u16,
    profile: &'a str,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    total_frames: u64,
    block_frames: u32,
    blocks: u64,
    payload_bytes: u64,
    file_bytes: u64,
    pcm_bytes: u64,
    plaq_to_pcm_ratio: f64,
    pcm_sha256: String,
    checksums: &'a str,
    predictor_counts: PredictorCounts,
}

#[derive(Debug, Serialize)]
struct PredictorCounts {
    raw: u64,
    delta: u64,
    linear2: u64,
    cubic3: u64,
    cross_axis: u64,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    input: String,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    frames: u64,
    pcm_bytes: u64,
    wav_bytes: u64,
    plaq_bytes: u64,
    plaq_to_pcm_ratio: f64,
    encode_mib_per_second: f64,
    decode_mib_per_second: f64,
    approximate_working_memory_bytes: u64,
    bit_perfect: bool,
    predictor_counts: PredictorCounts,
    flac_bytes: Option<u64>,
    flac_note: String,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Encode {
            input,
            output,
            block_frames,
        } => encode_file(&input, &output, block_frames),
        Commands::Decode { input, output } => decode_file(&input, &output),
        Commands::Verify { input, encoded } => verify_file(&input, &encoded),
        Commands::Inspect { input, json } => inspect_file(&input, json),
        Commands::Benchmark {
            input,
            compare,
            json,
        } => benchmark_file(&input, &compare, json),
        Commands::Visualize { input, out } => visualize_file(&input, &out),
        Commands::Simulate {
            input,
            output,
            mass,
            compliance,
            damping,
            max_displacement,
            max_velocity,
            tracking_error,
        } => simulate_file(
            &input,
            &output,
            StylusParameters {
                mass,
                compliance,
                damping,
                max_displacement,
                max_velocity,
                tracking_error,
            },
        ),
        Commands::StreamSend {
            input,
            transport,
            bind,
            target,
            stream_id,
        } => stream_send(
            &input,
            transport,
            bind.as_deref(),
            target.as_deref(),
            stream_id,
        ),
        Commands::StreamRecv {
            output,
            transport,
            connect,
            bind,
            timeout_ms,
        } => stream_recv(
            &output,
            transport,
            connect.as_deref(),
            bind.as_deref(),
            timeout_ms,
        ),
    }
}

fn encode_file(input: &Path, output: &Path, block_frames: u32) -> Result<()> {
    let wav = read_wav(input)?;
    let channels = u8::try_from(wav.spec.channels).context("channel count exceeds u8")?;
    let bits = u8::try_from(wav.spec.bits_per_sample).context("bit depth exceeds u8")?;
    let hash = canonical_pcm_sha256(&wav.samples, bits)?;
    let frames = wav.samples.len() / usize::from(channels);
    let header = PlaqHeader::lossless(
        channels,
        bits,
        wav.spec.sample_rate,
        frames as u64,
        block_frames,
        hash,
    );
    let (encoded, stats) = encode_to_vec(&header, &wav.samples)?;
    fs::write(output, &encoded).with_context(|| format!("failed to write {}", output.display()))?;
    println!(
        "encoded {} frames into {} bytes ({} blocks, PCM SHA-256 {})",
        frames,
        encoded.len(),
        stats.blocks,
        hex_sha256(&hash)
    );
    Ok(())
}

fn decode_file(input: &Path, output: &Path) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("failed to read {}", input.display()))?;
    let decoded = decode_from_slice(&bytes)?;
    write_wav(
        output,
        decoded.header.channels.into(),
        decoded.header.bits_per_sample.into(),
        decoded.header.sample_rate,
        &decoded.samples,
    )?;
    println!(
        "decoded {} frames; {} block checksums and PCM SHA-256 verified",
        decoded.header.total_frames, decoded.stats.checksums_verified
    );
    Ok(())
}

fn verify_file(input: &Path, encoded: &Path) -> Result<()> {
    let wav = read_wav(input)?;
    let bytes =
        fs::read(encoded).with_context(|| format!("failed to read {}", encoded.display()))?;
    let decoded = decode_from_slice(&bytes)?;
    let source_hash = canonical_pcm_sha256(&wav.samples, wav.spec.bits_per_sample as u8)?;
    let metadata_matches = u16::from(decoded.header.channels) == wav.spec.channels
        && u16::from(decoded.header.bits_per_sample) == wav.spec.bits_per_sample
        && decoded.header.sample_rate == wav.spec.sample_rate
        && decoded.header.total_frames
            == (wav.samples.len() / usize::from(wav.spec.channels)) as u64;
    if !metadata_matches
        || source_hash != decoded.header.pcm_sha256
        || wav.samples != decoded.samples
    {
        bail!("verification failed: decoded PCM or required metadata differs from source WAV");
    }
    println!(
        "bit-perfect: yes; PCM SHA-256 {}; checksums verified: {}",
        hex_sha256(&source_hash),
        decoded.stats.checksums_verified
    );
    Ok(())
}

fn inspect_file(input: &Path, json: bool) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("failed to read {}", input.display()))?;
    let decoded = decode_from_slice(&bytes)?;
    let report = inspection(&decoded.header, &decoded.stats, bytes.len() as u64);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("PLAQ v{} {}", report.format_version, report.profile);
        println!(
            "{} Hz, {} channel(s), {}-bit, {} frames",
            report.sample_rate, report.channels, report.bits_per_sample, report.total_frames
        );
        println!(
            "{} blocks, {} compressed payload bytes, checksums {}",
            report.blocks, report.payload_bytes, report.checksums
        );
        println!(
            "{} file bytes / {} PCM bytes = {:.4}x",
            report.file_bytes, report.pcm_bytes, report.plaq_to_pcm_ratio
        );
        println!("PCM SHA-256 {}", report.pcm_sha256);
        println!(
            "predictors raw={} delta={} linear2={} cubic3={} cross-axis={}",
            report.predictor_counts.raw,
            report.predictor_counts.delta,
            report.predictor_counts.linear2,
            report.predictor_counts.cubic3,
            report.predictor_counts.cross_axis
        );
    }
    Ok(())
}

fn benchmark_file(input: &Path, compare: &str, json: bool) -> Result<()> {
    let wav = read_wav(input)?;
    let channels = wav.spec.channels as usize;
    let bits = wav.spec.bits_per_sample as u8;
    let frames = wav.samples.len() / channels;
    let pcm_bytes = wav.samples.len() as u64 * u64::from(bits / 8);
    let hash = canonical_pcm_sha256(&wav.samples, bits)?;
    let header = PlaqHeader::lossless(
        wav.spec.channels as u8,
        bits,
        wav.spec.sample_rate,
        frames as u64,
        4_096,
        hash,
    );

    let encode_start = Instant::now();
    let (encoded, stats) = encode_to_vec(&header, &wav.samples)?;
    let encode_elapsed = encode_start.elapsed();
    let decode_start = Instant::now();
    let decoded = decode_from_slice(&encoded)?;
    let decode_elapsed = decode_start.elapsed();
    let bit_perfect = decoded.samples == wav.samples && decoded.header.pcm_sha256 == hash;
    let (flac_bytes, flac_note) = if compare.split(',').any(|item| item.trim() == "flac") {
        benchmark_flac(input)?
    } else {
        (None, "not requested".to_owned())
    };
    let report = BenchmarkReport {
        input: input.display().to_string(),
        sample_rate: wav.spec.sample_rate,
        channels: wav.spec.channels,
        bits_per_sample: wav.spec.bits_per_sample,
        frames: frames as u64,
        pcm_bytes,
        wav_bytes: fs::metadata(input)?.len(),
        plaq_bytes: encoded.len() as u64,
        plaq_to_pcm_ratio: ratio(encoded.len() as u64, pcm_bytes),
        encode_mib_per_second: throughput(pcm_bytes, encode_elapsed),
        decode_mib_per_second: throughput(pcm_bytes, decode_elapsed),
        approximate_working_memory_bytes: (wav.samples.len() as u64 * 4)
            .saturating_add(encoded.len() as u64)
            .saturating_add(decoded.samples.len() as u64 * 4),
        bit_perfect,
        predictor_counts: predictor_counts(&stats),
        flac_bytes,
        flac_note,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("PCM bytes: {}", report.pcm_bytes);
        println!(
            "PLAQ bytes: {} ({:.4}x PCM)",
            report.plaq_bytes, report.plaq_to_pcm_ratio
        );
        match report.flac_bytes {
            Some(size) => println!("FLAC bytes: {size}"),
            None => println!("FLAC: {}", report.flac_note),
        }
        println!(
            "encode {:.2} MiB/s; decode {:.2} MiB/s; bit-perfect: {}",
            report.encode_mib_per_second, report.decode_mib_per_second, report.bit_perfect
        );
    }
    if !bit_perfect {
        bail!("benchmark round-trip was not bit-perfect");
    }
    Ok(())
}

fn visualize_file(input: &Path, output: &Path) -> Result<()> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("visualize_groove.py");
    let status = Command::new("python")
        .arg(&script)
        .arg(input)
        .arg("--out")
        .arg(output)
        .status()
        .with_context(|| format!("failed to launch Python visualizer at {}", script.display()))?;
    if !status.success() {
        bail!("visualizer failed with status {status}");
    }
    println!("wrote {}", output.display());
    Ok(())
}

fn simulate_file(input: &Path, output: &Path, params: StylusParameters) -> Result<()> {
    let wav = read_wav(input)?;
    if !(params.mass > 0.0
        && params.compliance > 0.0
        && params.max_displacement > 0.0
        && params.max_velocity > 0.0
        && (0.0..=1.0).contains(&params.tracking_error))
    {
        bail!(
            "mass, compliance, displacement, and velocity must be positive; tracking error must be 0..1"
        );
    }
    let mut components = to_components(&wav.samples, wav.spec.channels as u8)?;
    let full_scale = ((1_i64 << (wav.spec.bits_per_sample - 1)) - 1) as f64;
    for (index, component) in components.iter_mut().enumerate() {
        let scale = if wav.spec.channels == 2 && index == 1 {
            full_scale * 2.0
        } else {
            full_scale
        };
        let normalized: Vec<f64> = component
            .iter()
            .map(|&value| f64::from(value) / scale)
            .collect();
        let simulated = simulate_axis(&normalized, wav.spec.sample_rate, params);
        for (sample, value) in component.iter_mut().zip(simulated) {
            *sample = (value * scale)
                .round()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        }
    }
    let mut samples = from_components(&components, wav.spec.channels as u8)?;
    let limit = 1_i32 << (wav.spec.bits_per_sample - 1);
    for sample in &mut samples {
        *sample = (*sample).clamp(-limit, limit - 1);
    }
    write_wav(
        output,
        wav.spec.channels,
        wav.spec.bits_per_sample,
        wav.spec.sample_rate,
        &samples,
    )?;
    println!(
        "wrote lossy physical-stylus simulation to {} (not bit-perfect)",
        output.display()
    );
    Ok(())
}

fn stream_send(
    input: &Path,
    transport: Transport,
    bind: Option<&str>,
    target: Option<&str>,
    stream_id: u32,
) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("failed to read {}", input.display()))?;
    decode_from_slice(&bytes).context("refusing to transmit an invalid PLAQ file")?;
    match transport {
        Transport::Tcp => {
            let bind = bind.context("--bind is required for TCP send")?;
            println!("waiting for one TCP receiver on {bind}");
            plaq_stream::send_tcp_bytes(bind, &bytes)?;
            println!("sent {} bytes over TCP", bytes.len());
        }
        Transport::Udp => {
            let target = target.context("--target is required for UDP send")?;
            let packets = plaq_stream::send_udp_bytes(target, &bytes, stream_id)?;
            println!("sent {} bytes in {} UDP packets", bytes.len(), packets);
        }
    }
    Ok(())
}

fn stream_recv(
    output: &Path,
    transport: Transport,
    connect: Option<&str>,
    bind: Option<&str>,
    timeout_ms: u64,
) -> Result<()> {
    let (bytes, stats) = match transport {
        Transport::Tcp => {
            let connect = connect.context("--connect is required for TCP receive")?;
            (plaq_stream::receive_tcp_bytes(connect)?, None)
        }
        Transport::Udp => {
            let bind = bind.context("--bind is required for UDP receive")?;
            let (bytes, stats) =
                plaq_stream::receive_udp_bytes(bind, Duration::from_millis(timeout_ms))?;
            (bytes, Some(stats))
        }
    };
    decode_from_slice(&bytes).context("received bytes are not a valid, complete PLAQ file")?;
    fs::write(output, &bytes).with_context(|| format!("failed to write {}", output.display()))?;
    println!("received and verified {} bytes", bytes.len());
    if let Some(stats) = stats {
        println!(
            "UDP packets={} reordered={} duplicate={} estimated_lost={} late={} checksum_failures={} recovered={} underruns={} latency_ms={:.3}",
            stats.packets_received,
            stats.reordered_packets,
            stats.duplicate_packets,
            stats.estimated_lost_packets,
            stats.late_packets,
            stats.checksum_failures,
            stats.recovered_packets,
            stats.underruns,
            stats.end_to_end_latency_micros as f64 / 1_000.0
        );
    }
    Ok(())
}

fn read_wav(path: &Path) -> Result<WavData> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open WAV {}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int {
        bail!("only integer PCM WAV is supported");
    }
    if !matches!(spec.channels, 1 | 2) {
        bail!("only mono and stereo WAV is supported");
    }
    if !matches!(spec.bits_per_sample, 16 | 24) {
        bail!("only 16-bit and 24-bit WAV is supported");
    }
    if spec.sample_rate == 0 || spec.sample_rate > 768_000 {
        bail!("sample rate is outside the supported range");
    }
    let samples = reader
        .samples::<i32>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to decode PCM samples")?;
    if !samples.len().is_multiple_of(usize::from(spec.channels)) {
        bail!("WAV has a partial final frame");
    }
    Ok(WavData { spec, samples })
}

fn write_wav(
    path: &Path,
    channels: u16,
    bits_per_sample: u16,
    sample_rate: u32,
    samples: &[i32],
) -> Result<()> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("failed to create WAV {}", path.display()))?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

fn inspection<'a>(header: &PlaqHeader, stats: &CodecStats, file_bytes: u64) -> Inspection<'a> {
    let pcm_bytes = header
        .total_frames
        .saturating_mul(u64::from(header.channels))
        .saturating_mul(u64::from(header.bits_per_sample / 8));
    Inspection {
        format_version: plaq_format::FORMAT_VERSION,
        profile: "lossless-trajectory",
        sample_rate: header.sample_rate,
        channels: header.channels,
        bits_per_sample: header.bits_per_sample,
        total_frames: header.total_frames,
        block_frames: header.block_frames,
        blocks: stats.blocks,
        payload_bytes: stats.payload_bytes,
        file_bytes,
        pcm_bytes,
        plaq_to_pcm_ratio: ratio(file_bytes, pcm_bytes),
        pcm_sha256: hex_sha256(&header.pcm_sha256),
        checksums: "valid",
        predictor_counts: predictor_counts(stats),
    }
}

fn predictor_counts(stats: &CodecStats) -> PredictorCounts {
    PredictorCounts {
        raw: stats.predictor_counts[0],
        delta: stats.predictor_counts[1],
        linear2: stats.predictor_counts[2],
        cubic3: stats.predictor_counts[3],
        cross_axis: stats.predictor_counts[4],
    }
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn throughput(bytes: u64, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        return 0.0;
    }
    (bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64()
}

fn benchmark_flac(input: &Path) -> Result<(Option<u64>, String)> {
    let temp_path =
        std::env::temp_dir().join(format!("plaq-benchmark-{}.flac", std::process::id()));
    let result = Command::new("flac")
        .arg("-f")
        .arg("-s")
        .arg("-o")
        .arg(&temp_path)
        .arg(input)
        .status();
    match result {
        Ok(status) if status.success() => {
            let bytes = fs::metadata(&temp_path)?.len();
            let _ = fs::remove_file(&temp_path);
            Ok((Some(bytes), "measured with system flac".to_owned()))
        }
        Ok(status) => Ok((None, format!("system flac exited with {status}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((None, "system flac executable not available".to_owned()))
        }
        Err(error) => Err(error.into()),
    }
}
