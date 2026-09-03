#![forbid(unsafe_code)]

//! Versioned, bounded `.plaq` container implementation.

use std::io::{self, Cursor, Read, Write};

use plaq_core::{
    CoreError,
    predictor::{Predictor, choose_with_reference, reconstruct_with_reference},
    rice::{RiceDecoder, RiceEncoder},
    transform::{from_components, to_components},
};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const FILE_MAGIC: &[u8; 4] = b"PLAQ";
pub const BLOCK_MAGIC: &[u8; 4] = b"BLK1";
pub const FORMAT_VERSION: u16 = 1;
pub const FIXED_HEADER_SIZE: u16 = 64;
pub const MAX_METADATA_BYTES: usize = 1_048_576;
pub const MAX_BLOCK_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TOTAL_SAMPLES: usize = 268_435_456;

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("codec error: {0}")]
    Core(#[from] CoreError),
    #[error("invalid PLAQ file: {0}")]
    Invalid(&'static str),
    #[error("unsupported PLAQ format version {0}")]
    UnsupportedVersion(u16),
    #[error("block {block_id} CRC32C mismatch")]
    ChecksumMismatch { block_id: u32 },
    #[error("sample value {value} does not fit signed {bits}-bit PCM")]
    SampleOutOfRange { value: i32, bits: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataField {
    pub kind: u16,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaqHeader {
    pub profile: u8,
    pub channels: u8,
    pub bits_per_sample: u8,
    pub sample_rate: u32,
    pub total_frames: u64,
    pub block_frames: u32,
    pub pcm_sha256: [u8; 32],
    pub metadata: Vec<MetadataField>,
}

impl PlaqHeader {
    pub fn lossless(
        channels: u8,
        bits_per_sample: u8,
        sample_rate: u32,
        total_frames: u64,
        block_frames: u32,
        pcm_sha256: [u8; 32],
    ) -> Self {
        Self {
            profile: 0,
            channels,
            bits_per_sample,
            sample_rate,
            total_frames,
            block_frames,
            pcm_sha256,
            metadata: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodecStats {
    pub blocks: u64,
    pub payload_bytes: u64,
    pub predictor_counts: [u64; 5],
    pub checksums_verified: u64,
}

#[derive(Debug, Clone)]
pub struct DecodedFile {
    pub header: PlaqHeader,
    pub samples: Vec<i32>,
    pub stats: CodecStats,
}

pub fn canonical_pcm_sha256(samples: &[i32], bits: u8) -> Result<[u8; 32], FormatError> {
    if bits != 16 && bits != 24 {
        return Err(FormatError::Invalid(
            "only 16-bit and 24-bit PCM are supported",
        ));
    }
    let bytes_per_sample = usize::from(bits / 8);
    let mut hasher = Sha256::new();
    for &sample in samples {
        validate_sample(sample, bits)?;
        let bytes = sample.to_le_bytes();
        hasher.update(&bytes[..bytes_per_sample]);
    }
    Ok(hasher.finalize().into())
}

pub fn encode_to_vec(
    header: &PlaqHeader,
    samples: &[i32],
) -> Result<(Vec<u8>, CodecStats), FormatError> {
    let mut bytes = Vec::new();
    let stats = encode(&mut bytes, header, samples)?;
    Ok((bytes, stats))
}

pub fn encode<W: Write>(
    mut writer: W,
    header: &PlaqHeader,
    samples: &[i32],
) -> Result<CodecStats, FormatError> {
    validate_header(header)?;
    let expected_samples = usize::try_from(header.total_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(header.channels)))
        .ok_or(FormatError::Invalid("sample count overflows this platform"))?;
    if expected_samples != samples.len() {
        return Err(FormatError::Invalid(
            "header frame count does not match PCM samples",
        ));
    }
    for &sample in samples {
        validate_sample(sample, header.bits_per_sample)?;
    }
    if canonical_pcm_sha256(samples, header.bits_per_sample)? != header.pcm_sha256 {
        return Err(FormatError::Invalid(
            "header PCM SHA-256 does not match samples",
        ));
    }

    let metadata = encode_metadata(&header.metadata)?;
    write_file_header(&mut writer, header, &metadata)?;

    let channels = usize::from(header.channels);
    let block_samples = usize::try_from(header.block_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(channels))
        .ok_or(FormatError::Invalid("block size overflows this platform"))?;
    let mut stats = CodecStats::default();
    for (block_id, block) in samples.chunks(block_samples).enumerate() {
        let components = to_components(block, header.channels)?;
        let mut rice = RiceEncoder::new();
        let mut descriptors = Vec::with_capacity(components.len());
        for (component_index, component) in components.iter().enumerate() {
            let reference = if component_index > 0 {
                Some(components[0].as_slice())
            } else {
                None
            };
            let selected = choose_with_reference(component, reference)?;
            rice.write_values(&selected.residuals, selected.rice_k)?;
            descriptors.push((selected.predictor, selected.rice_k));
            stats.predictor_counts[selected.predictor as usize] += 1;
        }
        let payload = rice.finish();
        if payload.len() > MAX_BLOCK_PAYLOAD_BYTES {
            return Err(FormatError::Invalid("encoded block exceeds payload limit"));
        }
        let frame_count = u32::try_from(block.len() / channels)
            .map_err(|_| FormatError::Invalid("block frame count exceeds u32"))?;
        write_block(
            &mut writer,
            u32::try_from(block_id).map_err(|_| FormatError::Invalid("too many blocks"))?,
            frame_count,
            &descriptors,
            &payload,
        )?;
        stats.blocks += 1;
        stats.payload_bytes += payload.len() as u64;
    }
    Ok(stats)
}

pub fn decode_from_slice(bytes: &[u8]) -> Result<DecodedFile, FormatError> {
    decode(Cursor::new(bytes))
}

pub fn decode<R: Read>(mut reader: R) -> Result<DecodedFile, FormatError> {
    let header = read_file_header(&mut reader)?;
    let total_samples = usize::try_from(header.total_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(header.channels)))
        .ok_or(FormatError::Invalid("sample count overflows this platform"))?;
    if total_samples > MAX_TOTAL_SAMPLES {
        return Err(FormatError::Invalid(
            "declared sample count exceeds safety limit",
        ));
    }
    let mut samples = Vec::with_capacity(total_samples);
    let mut stats = CodecStats::default();
    let mut expected_block_id = 0_u32;

    while samples.len() < total_samples {
        let block = read_block(&mut reader, header.channels, expected_block_id)?;
        let frame_count = usize::try_from(block.frame_count)
            .map_err(|_| FormatError::Invalid("frame count exceeds platform limits"))?;
        let block_sample_count = frame_count
            .checked_mul(usize::from(header.channels))
            .ok_or(FormatError::Invalid("block sample count overflow"))?;
        if frame_count == 0 || frame_count > header.block_frames as usize {
            return Err(FormatError::Invalid("invalid block frame count"));
        }
        if samples
            .len()
            .checked_add(block_sample_count)
            .is_none_or(|n| n > total_samples)
        {
            return Err(FormatError::Invalid("block exceeds declared total frames"));
        }

        let mut rice = RiceDecoder::new(&block.payload);
        let mut components: Vec<Vec<i32>> = Vec::with_capacity(block.descriptors.len());
        for (component_index, &(predictor, k)) in block.descriptors.iter().enumerate() {
            let residuals = rice.read_values(frame_count, k)?;
            let reference = if component_index > 0 {
                Some(components[0].as_slice())
            } else {
                None
            };
            components.push(reconstruct_with_reference(
                &residuals, predictor, reference,
            )?);
            stats.predictor_counts[predictor as usize] += 1;
        }
        samples.extend(from_components(&components, header.channels)?);
        stats.blocks += 1;
        stats.payload_bytes += block.payload.len() as u64;
        stats.checksums_verified += 1;
        expected_block_id = expected_block_id
            .checked_add(1)
            .ok_or(FormatError::Invalid("block id overflow"))?;
    }

    let mut trailing = [0_u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err(FormatError::Invalid("trailing data after final block"));
    }
    if canonical_pcm_sha256(&samples, header.bits_per_sample)? != header.pcm_sha256 {
        return Err(FormatError::Invalid("decoded PCM SHA-256 mismatch"));
    }
    Ok(DecodedFile {
        header,
        samples,
        stats,
    })
}

struct EncodedBlock {
    frame_count: u32,
    descriptors: Vec<(Predictor, u8)>,
    payload: Vec<u8>,
}

fn write_file_header<W: Write>(
    writer: &mut W,
    header: &PlaqHeader,
    metadata: &[u8],
) -> Result<(), FormatError> {
    writer.write_all(FILE_MAGIC)?;
    write_u16(writer, FORMAT_VERSION)?;
    let header_len = usize::from(FIXED_HEADER_SIZE)
        .checked_add(metadata.len())
        .and_then(|value| u16::try_from(value).ok())
        .ok_or(FormatError::Invalid("header is too large"))?;
    write_u16(writer, header_len)?;
    writer.write_all(&[header.profile, header.channels, header.bits_per_sample, 0])?;
    write_u32(writer, header.sample_rate)?;
    write_u64(writer, header.total_frames)?;
    write_u32(writer, header.block_frames)?;
    write_u32(
        writer,
        u32::try_from(metadata.len()).map_err(|_| FormatError::Invalid("metadata is too large"))?,
    )?;
    writer.write_all(&header.pcm_sha256)?;
    writer.write_all(metadata)?;
    Ok(())
}

fn read_file_header<R: Read>(reader: &mut R) -> Result<PlaqHeader, FormatError> {
    let mut magic = [0_u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != FILE_MAGIC {
        return Err(FormatError::Invalid("bad file magic"));
    }
    let version = read_u16(reader)?;
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let header_len = read_u16(reader)?;
    let mut profile = [0_u8; 4];
    reader.read_exact(&mut profile)?;
    let sample_rate = read_u32(reader)?;
    let total_frames = read_u64(reader)?;
    let block_frames = read_u32(reader)?;
    let metadata_len = read_u32(reader)? as usize;
    let mut pcm_sha256 = [0_u8; 32];
    reader.read_exact(&mut pcm_sha256)?;
    if header_len as usize != usize::from(FIXED_HEADER_SIZE) + metadata_len {
        return Err(FormatError::Invalid(
            "header length and metadata length disagree",
        ));
    }
    if metadata_len > MAX_METADATA_BYTES {
        return Err(FormatError::Invalid("metadata exceeds safety limit"));
    }
    let mut metadata = vec![0_u8; metadata_len];
    reader.read_exact(&mut metadata)?;
    let header = PlaqHeader {
        profile: profile[0],
        channels: profile[1],
        bits_per_sample: profile[2],
        sample_rate,
        total_frames,
        block_frames,
        pcm_sha256,
        metadata: decode_metadata(&metadata)?,
    };
    validate_header(&header)?;
    Ok(header)
}

fn write_block<W: Write>(
    writer: &mut W,
    block_id: u32,
    frame_count: u32,
    descriptors: &[(Predictor, u8)],
    payload: &[u8],
) -> Result<(), FormatError> {
    writer.write_all(BLOCK_MAGIC)?;
    write_u32(writer, block_id)?;
    write_u32(writer, frame_count)?;
    writer.write_all(&[
        u8::try_from(descriptors.len()).map_err(|_| FormatError::Invalid("too many components"))?,
        1,
    ])?;
    write_u16(
        writer,
        u16::try_from(descriptors.len() * 4)
            .map_err(|_| FormatError::Invalid("descriptor size overflow"))?,
    )?;
    write_u32(
        writer,
        u32::try_from(payload.len()).map_err(|_| FormatError::Invalid("payload is too large"))?,
    )?;
    write_u32(writer, crc32c::crc32c(payload))?;
    for &(predictor, k) in descriptors {
        writer.write_all(&[predictor as u8, k, 0, 0])?;
    }
    writer.write_all(payload)?;
    Ok(())
}

fn read_block<R: Read>(
    reader: &mut R,
    channels: u8,
    expected_block_id: u32,
) -> Result<EncodedBlock, FormatError> {
    let mut magic = [0_u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != BLOCK_MAGIC {
        return Err(FormatError::Invalid("bad block magic"));
    }
    let block_id = read_u32(reader)?;
    if block_id != expected_block_id {
        return Err(FormatError::Invalid("non-sequential block id"));
    }
    let frame_count = read_u32(reader)?;
    let mut small = [0_u8; 2];
    reader.read_exact(&mut small)?;
    let component_count = small[0];
    if component_count != channels || small[1] & 1 == 0 {
        return Err(FormatError::Invalid(
            "invalid component count or missing reset flag",
        ));
    }
    let descriptor_len = read_u16(reader)? as usize;
    if descriptor_len != usize::from(component_count) * 4 {
        return Err(FormatError::Invalid("invalid descriptor length"));
    }
    let payload_len = read_u32(reader)? as usize;
    if payload_len > MAX_BLOCK_PAYLOAD_BYTES {
        return Err(FormatError::Invalid("block payload exceeds safety limit"));
    }
    let expected_crc = read_u32(reader)?;
    let mut descriptors = Vec::with_capacity(usize::from(component_count));
    for _ in 0..component_count {
        let mut descriptor = [0_u8; 4];
        reader.read_exact(&mut descriptor)?;
        descriptors.push((Predictor::from_id(descriptor[0])?, descriptor[1]));
    }
    let mut payload = vec![0_u8; payload_len];
    reader.read_exact(&mut payload)?;
    if crc32c::crc32c(&payload) != expected_crc {
        return Err(FormatError::ChecksumMismatch { block_id });
    }
    Ok(EncodedBlock {
        frame_count,
        descriptors,
        payload,
    })
}

fn validate_header(header: &PlaqHeader) -> Result<(), FormatError> {
    if header.profile != 0 {
        return Err(FormatError::Invalid("only lossless profile 0 is supported"));
    }
    if !matches!(header.channels, 1 | 2) {
        return Err(FormatError::Invalid("only mono and stereo are supported"));
    }
    if !matches!(header.bits_per_sample, 16 | 24) {
        return Err(FormatError::Invalid(
            "only 16-bit and 24-bit PCM are supported",
        ));
    }
    if header.sample_rate == 0 || header.sample_rate > 768_000 {
        return Err(FormatError::Invalid(
            "sample rate is outside the supported range",
        ));
    }
    if header.block_frames == 0 || header.block_frames > 1_048_576 {
        return Err(FormatError::Invalid(
            "block frame count is outside the supported range",
        ));
    }
    Ok(())
}

fn validate_sample(value: i32, bits: u8) -> Result<(), FormatError> {
    let limit = 1_i64 << (bits - 1);
    if i64::from(value) < -limit || i64::from(value) >= limit {
        return Err(FormatError::SampleOutOfRange { value, bits });
    }
    Ok(())
}

fn encode_metadata(fields: &[MetadataField]) -> Result<Vec<u8>, FormatError> {
    let mut encoded = Vec::new();
    for field in fields {
        encoded.extend_from_slice(&field.kind.to_le_bytes());
        let len = u32::try_from(field.value.len())
            .map_err(|_| FormatError::Invalid("metadata field is too large"))?;
        encoded.extend_from_slice(&len.to_le_bytes());
        encoded.extend_from_slice(&field.value);
        if encoded.len() > MAX_METADATA_BYTES {
            return Err(FormatError::Invalid("metadata exceeds safety limit"));
        }
    }
    Ok(encoded)
}

fn decode_metadata(bytes: &[u8]) -> Result<Vec<MetadataField>, FormatError> {
    let mut cursor = Cursor::new(bytes);
    let mut fields = Vec::new();
    while cursor.position() < bytes.len() as u64 {
        let remaining = bytes.len() as u64 - cursor.position();
        if remaining < 6 {
            return Err(FormatError::Invalid("truncated metadata field"));
        }
        let kind = read_u16(&mut cursor)?;
        let len = read_u32(&mut cursor)? as usize;
        let position = usize::try_from(cursor.position())
            .map_err(|_| FormatError::Invalid("metadata position overflow"))?;
        let end = position
            .checked_add(len)
            .filter(|&end| end <= bytes.len())
            .ok_or(FormatError::Invalid("metadata field length exceeds header"))?;
        fields.push(MetadataField {
            kind,
            value: bytes[position..end].to_vec(),
        });
        cursor.set_position(end as u64);
    }
    Ok(fields)
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u64<W: Write>(writer: &mut W, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u16<R: Read>(reader: &mut R) -> io::Result<u16> {
    let mut bytes = [0_u8; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn hex_sha256(hash: &[u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
