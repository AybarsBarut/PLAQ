#![forbid(unsafe_code)]

//! Transport-independent PLAQ packet framing plus TCP and UDP demos.

use std::{
    collections::{BTreeMap, HashSet},
    io::{Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PACKET_MAGIC: &[u8; 4] = b"PPK1";
pub const TCP_MAGIC: &[u8; 4] = b"PTC1";
pub const PACKET_VERSION: u8 = 1;
pub const PACKET_HEADER_LEN: usize = 44;
pub const FLAG_END: u8 = 1;
pub const DEFAULT_PACKET_PAYLOAD: usize = 1_200;
pub const DEFAULT_BLOCK_BYTES: usize = 32 * 1024;
pub const MAX_STREAM_BYTES: usize = 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid transport data: {0}")]
    Invalid(&'static str),
    #[error("packet payload checksum mismatch")]
    PacketChecksum,
    #[error("stream SHA-256 mismatch")]
    StreamChecksum,
    #[error("UDP receive timed out with incomplete blocks")]
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub flags: u8,
    pub stream_id: u32,
    pub sequence: u64,
    pub timestamp_micros: u64,
    pub block_id: u32,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn encode(&self) -> Result<Vec<u8>, StreamError> {
        if self.fragment_count == 0 || self.fragment_index >= self.fragment_count {
            return Err(StreamError::Invalid("invalid fragment index/count"));
        }
        let payload_len = u16::try_from(self.payload.len())
            .map_err(|_| StreamError::Invalid("packet payload exceeds u16"))?;
        let mut bytes = Vec::with_capacity(PACKET_HEADER_LEN + self.payload.len());
        bytes.extend_from_slice(PACKET_MAGIC);
        bytes.push(PACKET_VERSION);
        bytes.push(self.flags);
        bytes.extend_from_slice(&(PACKET_HEADER_LEN as u16).to_le_bytes());
        bytes.extend_from_slice(&self.stream_id.to_le_bytes());
        bytes.extend_from_slice(&self.sequence.to_le_bytes());
        bytes.extend_from_slice(&self.timestamp_micros.to_le_bytes());
        bytes.extend_from_slice(&self.block_id.to_le_bytes());
        bytes.extend_from_slice(&self.fragment_index.to_le_bytes());
        bytes.extend_from_slice(&self.fragment_count.to_le_bytes());
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&crc32c::crc32c(&self.payload).to_le_bytes());
        bytes.extend_from_slice(&self.payload);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, StreamError> {
        if bytes.len() < PACKET_HEADER_LEN {
            return Err(StreamError::Invalid("truncated packet header"));
        }
        if &bytes[0..4] != PACKET_MAGIC {
            return Err(StreamError::Invalid("bad packet magic"));
        }
        if bytes[4] != PACKET_VERSION {
            return Err(StreamError::Invalid("unsupported packet version"));
        }
        if usize::from(u16::from_le_bytes([bytes[6], bytes[7]])) != PACKET_HEADER_LEN {
            return Err(StreamError::Invalid("bad packet header length"));
        }
        let payload_len = usize::from(u16::from_le_bytes([bytes[36], bytes[37]]));
        if PACKET_HEADER_LEN.checked_add(payload_len) != Some(bytes.len()) {
            return Err(StreamError::Invalid("packet payload length mismatch"));
        }
        let fragment_index = u16::from_le_bytes([bytes[32], bytes[33]]);
        let fragment_count = u16::from_le_bytes([bytes[34], bytes[35]]);
        if fragment_count == 0 || fragment_index >= fragment_count {
            return Err(StreamError::Invalid("invalid fragment index/count"));
        }
        let payload = bytes[PACKET_HEADER_LEN..].to_vec();
        let checksum = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        if crc32c::crc32c(&payload) != checksum {
            return Err(StreamError::PacketChecksum);
        }
        Ok(Self {
            flags: bytes[5],
            stream_id: u32::from_le_bytes(bytes[8..12].try_into().expect("fixed slice")),
            sequence: u64::from_le_bytes(bytes[12..20].try_into().expect("fixed slice")),
            timestamp_micros: u64::from_le_bytes(bytes[20..28].try_into().expect("fixed slice")),
            block_id: u32::from_le_bytes(bytes[28..32].try_into().expect("fixed slice")),
            fragment_index,
            fragment_count,
            payload,
        })
    }
}

pub fn packetize(
    bytes: &[u8],
    stream_id: u32,
    block_bytes: usize,
    packet_payload: usize,
) -> Result<Vec<Packet>, StreamError> {
    if block_bytes == 0 || packet_payload == 0 || packet_payload > usize::from(u16::MAX) {
        return Err(StreamError::Invalid("invalid packetization size"));
    }
    let timestamp_micros = timestamp_micros();
    let blocks: Vec<&[u8]> = if bytes.is_empty() {
        vec![&[]]
    } else {
        bytes.chunks(block_bytes).collect()
    };
    let final_block = blocks.len() - 1;
    let mut packets = Vec::new();
    let mut sequence = 0_u64;
    for (block_id, block) in blocks.into_iter().enumerate() {
        let fragment_count_usize = block.len().max(1).div_ceil(packet_payload);
        let fragment_count = u16::try_from(fragment_count_usize)
            .map_err(|_| StreamError::Invalid("too many fragments in a block"))?;
        for fragment_index in 0..fragment_count {
            let start = usize::from(fragment_index) * packet_payload;
            let end = start.saturating_add(packet_payload).min(block.len());
            packets.push(Packet {
                flags: if block_id == final_block { FLAG_END } else { 0 },
                stream_id,
                sequence,
                timestamp_micros,
                block_id: u32::try_from(block_id)
                    .map_err(|_| StreamError::Invalid("too many stream blocks"))?,
                fragment_index,
                fragment_count,
                payload: block[start..end].to_vec(),
            });
            sequence = sequence
                .checked_add(1)
                .ok_or(StreamError::Invalid("packet sequence overflow"))?;
        }
    }
    Ok(packets)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransportStats {
    pub packets_received: u64,
    pub duplicate_packets: u64,
    pub reordered_packets: u64,
    pub estimated_lost_packets: u64,
    pub late_packets: u64,
    pub checksum_failures: u64,
    pub recovered_packets: u64,
    pub underruns: u64,
    pub end_to_end_latency_micros: u64,
}

#[derive(Debug, Clone)]
pub struct CompletedBlock {
    pub block_id: u32,
    pub timestamp_micros: u64,
    pub end: bool,
    pub bytes: Vec<u8>,
}

struct PendingBlock {
    fragments: Vec<Option<Vec<u8>>>,
    end: bool,
    timestamp_micros: u64,
    byte_count: usize,
}

/// Small block-level jitter buffer with deterministic in-order release.
pub struct JitterBuffer {
    stream_id: Option<u32>,
    next_block: u32,
    pending: BTreeMap<u32, PendingBlock>,
    complete: BTreeMap<u32, CompletedBlock>,
    seen_sequences: HashSet<u64>,
    highest_sequence: Option<u64>,
    max_pending_blocks: usize,
    max_block_bytes: usize,
    pub stats: TransportStats,
}

impl JitterBuffer {
    pub fn new(max_pending_blocks: usize, max_block_bytes: usize) -> Self {
        Self {
            stream_id: None,
            next_block: 0,
            pending: BTreeMap::new(),
            complete: BTreeMap::new(),
            seen_sequences: HashSet::new(),
            highest_sequence: None,
            max_pending_blocks,
            max_block_bytes,
            stats: TransportStats::default(),
        }
    }

    pub fn note_checksum_failure(&mut self) {
        self.stats.checksum_failures += 1;
    }

    pub fn push(&mut self, packet: Packet) -> Result<Vec<CompletedBlock>, StreamError> {
        self.stats.packets_received += 1;
        if let Some(stream_id) = self.stream_id {
            if stream_id != packet.stream_id {
                return Err(StreamError::Invalid("packet belongs to a different stream"));
            }
        } else {
            self.stream_id = Some(packet.stream_id);
        }
        if !self.seen_sequences.insert(packet.sequence) {
            self.stats.duplicate_packets += 1;
            return Ok(Vec::new());
        }
        if self
            .highest_sequence
            .is_some_and(|highest| packet.sequence < highest)
        {
            self.stats.reordered_packets += 1;
        }
        self.highest_sequence = Some(
            self.highest_sequence
                .map_or(packet.sequence, |highest| highest.max(packet.sequence)),
        );
        self.stats.estimated_lost_packets = self
            .highest_sequence
            .map_or(0, |highest| highest + 1 - self.seen_sequences.len() as u64);

        if packet.block_id < self.next_block {
            self.stats.late_packets += 1;
            return Ok(Vec::new());
        }
        if !self.pending.contains_key(&packet.block_id)
            && self.pending.len() + self.complete.len() >= self.max_pending_blocks
        {
            self.stats.underruns += 1;
            return Err(StreamError::Invalid("jitter buffer capacity exceeded"));
        }
        let pending = self
            .pending
            .entry(packet.block_id)
            .or_insert_with(|| PendingBlock {
                fragments: vec![None; usize::from(packet.fragment_count)],
                end: packet.flags & FLAG_END != 0,
                timestamp_micros: packet.timestamp_micros,
                byte_count: 0,
            });
        if pending.fragments.len() != usize::from(packet.fragment_count) {
            return Err(StreamError::Invalid(
                "fragment count changed within a block",
            ));
        }
        let slot = &mut pending.fragments[usize::from(packet.fragment_index)];
        if slot.is_some() {
            self.stats.duplicate_packets += 1;
            return Ok(Vec::new());
        }
        pending.byte_count = pending
            .byte_count
            .checked_add(packet.payload.len())
            .filter(|&size| size <= self.max_block_bytes)
            .ok_or(StreamError::Invalid(
                "reassembled block exceeds safety limit",
            ))?;
        *slot = Some(packet.payload);

        if pending.fragments.iter().all(Option::is_some) {
            let pending = self.pending.remove(&packet.block_id).expect("entry exists");
            let mut bytes = Vec::with_capacity(pending.byte_count);
            for fragment in pending.fragments {
                bytes.extend(fragment.expect("all fragments present"));
            }
            self.complete.insert(
                packet.block_id,
                CompletedBlock {
                    block_id: packet.block_id,
                    timestamp_micros: pending.timestamp_micros,
                    end: pending.end,
                    bytes,
                },
            );
        }
        let mut released = Vec::new();
        while let Some(block) = self.complete.remove(&self.next_block) {
            released.push(block);
            self.next_block = self
                .next_block
                .checked_add(1)
                .ok_or(StreamError::Invalid("block id overflow"))?;
        }
        Ok(released)
    }

    pub fn is_incomplete(&self) -> bool {
        !self.pending.is_empty() || !self.complete.is_empty()
    }
}

pub fn send_tcp_bytes<A: ToSocketAddrs>(bind: A, bytes: &[u8]) -> Result<(), StreamError> {
    if bytes.len() > MAX_STREAM_BYTES {
        return Err(StreamError::Invalid("stream exceeds safety limit"));
    }
    let listener = TcpListener::bind(bind)?;
    let (mut stream, _) = listener.accept()?;
    stream.write_all(TCP_MAGIC)?;
    stream.write_all(&(bytes.len() as u64).to_le_bytes())?;
    stream.write_all(&Sha256::digest(bytes))?;
    stream.write_all(bytes)?;
    stream.flush()?;
    Ok(())
}

pub fn receive_tcp_bytes<A: ToSocketAddrs>(connect: A) -> Result<Vec<u8>, StreamError> {
    let mut stream = TcpStream::connect(connect)?;
    let mut magic = [0_u8; 4];
    stream.read_exact(&mut magic)?;
    if &magic != TCP_MAGIC {
        return Err(StreamError::Invalid("bad TCP stream magic"));
    }
    let mut length = [0_u8; 8];
    stream.read_exact(&mut length)?;
    let length = usize::try_from(u64::from_le_bytes(length))
        .ok()
        .filter(|&size| size <= MAX_STREAM_BYTES)
        .ok_or(StreamError::Invalid(
            "TCP stream length exceeds safety limit",
        ))?;
    let mut expected_hash = [0_u8; 32];
    stream.read_exact(&mut expected_hash)?;
    let mut bytes = vec![0_u8; length];
    stream.read_exact(&mut bytes)?;
    if Sha256::digest(&bytes).as_slice() != expected_hash {
        return Err(StreamError::StreamChecksum);
    }
    Ok(bytes)
}

pub fn send_udp_bytes<A: ToSocketAddrs>(
    target: A,
    bytes: &[u8],
    stream_id: u32,
) -> Result<usize, StreamError> {
    if bytes.len() > MAX_STREAM_BYTES {
        return Err(StreamError::Invalid("stream exceeds safety limit"));
    }
    let target = target
        .to_socket_addrs()?
        .next()
        .ok_or(StreamError::Invalid("target address did not resolve"))?;
    let bind = if target.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = UdpSocket::bind(bind)?;
    let packets = packetize(
        bytes,
        stream_id,
        DEFAULT_BLOCK_BYTES,
        DEFAULT_PACKET_PAYLOAD,
    )?;
    for packet in &packets {
        let datagram = packet.encode()?;
        socket.send_to(&datagram, target)?;
    }
    Ok(packets.len())
}

pub fn receive_udp_bytes<A: ToSocketAddrs>(
    bind: A,
    timeout: Duration,
) -> Result<(Vec<u8>, TransportStats), StreamError> {
    let socket = UdpSocket::bind(bind)?;
    socket.set_read_timeout(Some(timeout))?;
    let mut jitter = JitterBuffer::new(128, DEFAULT_BLOCK_BYTES);
    let mut output = Vec::new();
    let mut datagram = vec![0_u8; 65_535];
    loop {
        let length = match socket.recv(&mut datagram) {
            Ok(length) => length,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(StreamError::Incomplete);
            }
            Err(error) => return Err(error.into()),
        };
        let packet = match Packet::decode(&datagram[..length]) {
            Ok(packet) => packet,
            Err(StreamError::PacketChecksum) => {
                jitter.note_checksum_failure();
                continue;
            }
            Err(error) => return Err(error),
        };
        for block in jitter.push(packet)? {
            output.extend_from_slice(&block.bytes);
            if output.len() > MAX_STREAM_BYTES {
                return Err(StreamError::Invalid("received stream exceeds safety limit"));
            }
            if block.end {
                jitter.stats.end_to_end_latency_micros =
                    timestamp_micros().saturating_sub(block.timestamp_micros);
                return Ok((output, jitter.stats));
            }
        }
    }
}

fn timestamp_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
