// TODO: Replace with real Opus encoding/decoding (e.g. via the `opus` or `audiopus` crate).
// Currently this is a pass-through that ships raw PCM as little-endian i16 bytes.
// At 48 kHz mono, each 20 ms frame is 960 samples × 2 bytes = 1920 bytes (~768 kbps),
// which works for local/synthetic testing but wastes bandwidth over real MOQ relays
// where Opus would compress to ~16–32 kbps.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusPacket(pub Vec<u8>);

#[derive(Debug, Clone, Default)]
pub struct OpusCodec;

impl OpusCodec {
    pub fn encode_pcm_i16(&self, pcm: &[i16]) -> OpusPacket {
        // Raw PCM pass-through — not actual Opus. See module-level TODO.
        OpusPacket(
            pcm.iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<u8>>(),
        )
    }

    pub fn decode_to_pcm_i16(&self, packet: &OpusPacket) -> Vec<i16> {
        packet
            .0
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_roundtrip_preserves_samples() {
        let codec = OpusCodec;
        let pcm: Vec<i16> = vec![-32768, -1024, -1, 0, 1, 1024, 32767];
        let packet = codec.encode_pcm_i16(&pcm);
        let decoded = codec.decode_to_pcm_i16(&packet);
        assert_eq!(decoded, pcm);
    }
}
