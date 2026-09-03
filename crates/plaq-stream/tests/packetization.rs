use plaq_stream::{JitterBuffer, Packet, StreamError, packetize};

#[test]
fn reordered_and_duplicated_packets_reassemble() {
    let input: Vec<u8> = (0..10_000).map(|value| (value % 251) as u8).collect();
    let packets = packetize(&input, 7, 1_024, 200).unwrap();
    let mut delivery = packets.clone();
    delivery.reverse();
    delivery.push(packets[0].clone());

    let mut jitter = JitterBuffer::new(128, 1_024);
    let mut output = Vec::new();
    for packet in delivery {
        for block in jitter.push(packet).unwrap() {
            output.extend(block.bytes);
        }
    }
    assert_eq!(output, input);
    assert!(jitter.stats.reordered_packets > 0);
    assert!(jitter.stats.duplicate_packets > 0 || jitter.stats.late_packets > 0);
}

#[test]
fn missing_fragment_is_not_concealed() {
    let input = vec![42_u8; 4_000];
    let mut packets = packetize(&input, 9, 1_024, 200).unwrap();
    packets.remove(2);
    let mut jitter = JitterBuffer::new(128, 1_024);
    let mut output = Vec::new();
    for packet in packets {
        for block in jitter.push(packet).unwrap() {
            output.extend(block.bytes);
        }
    }
    assert!(jitter.is_incomplete());
    assert!(output.len() < input.len());
    assert!(jitter.stats.estimated_lost_packets >= 1);
}

#[test]
fn corrupted_packet_is_rejected() {
    let packet = Packet {
        flags: 0,
        stream_id: 1,
        sequence: 0,
        timestamp_micros: 0,
        block_id: 0,
        fragment_index: 0,
        fragment_count: 1,
        payload: vec![1, 2, 3],
    };
    let mut encoded = packet.encode().unwrap();
    *encoded.last_mut().unwrap() ^= 1;
    assert!(matches!(
        Packet::decode(&encoded),
        Err(StreamError::PacketChecksum)
    ));
}
