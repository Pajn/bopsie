//! Cast's wire format: each message a `CastMessage` protobuf behind its length, as four bytes
//! big-endian. Only text payloads are used, so only their fields are written; others are
//! skipped when read.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest message a device sends, by Cast's own limit.
const MAX_LENGTH: u32 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastMessage {
    pub source: String,
    pub destination: String,
    pub namespace: String,
    /// JSON, as every namespace used here speaks it.
    pub payload: String,
}

impl CastMessage {
    pub fn new(source: &str, destination: &str, namespace: &str, payload: String) -> Self {
        Self {
            source: source.to_owned(),
            destination: destination.to_owned(),
            namespace: namespace.to_owned(),
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.payload.len() + 64);
        // protocol_version: CASTV2_1_0.
        varint(&mut out, 1 << 3);
        varint(&mut out, 0);
        text(&mut out, 2, &self.source);
        text(&mut out, 3, &self.destination);
        text(&mut out, 4, &self.namespace);
        // payload_type: STRING.
        varint(&mut out, 5 << 3);
        varint(&mut out, 0);
        text(&mut out, 6, &self.payload);
        out
    }

    pub fn decode(mut bytes: &[u8]) -> Result<Self, String> {
        let mut message = Self::new("", "", "", String::new());
        while !bytes.is_empty() {
            let key = read_varint(&mut bytes)?;
            let (field, wire) = (key >> 3, key & 7);
            match wire {
                0 => {
                    read_varint(&mut bytes)?;
                }
                1 => bytes = bytes.get(8..).ok_or("a field runs past the message")?,
                5 => bytes = bytes.get(4..).ok_or("a field runs past the message")?,
                2 => {
                    let length = usize::try_from(read_varint(&mut bytes)?)
                        .map_err(|_| "a field is too long")?;
                    let value = bytes.get(..length).ok_or("a field runs past the message")?;
                    bytes = &bytes[length..];
                    let value =
                        || String::from_utf8(value.to_vec()).map_err(|_| "a field is not UTF-8");
                    match field {
                        2 => message.source = value()?,
                        3 => message.destination = value()?,
                        4 => message.namespace = value()?,
                        6 => message.payload = value()?,
                        _ => {}
                    }
                }
                _ => return Err(format!("wire type {wire} is not protobuf's")),
            }
        }
        Ok(message)
    }
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn text(out: &mut Vec<u8>, field: u64, value: &str) {
    varint(out, (field << 3) | 2);
    varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn read_varint(bytes: &mut &[u8]) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes
            .split_first()
            .ok_or("a number runs past the message")?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("a number is too long".to_owned())
}

/// Writes `message` with its length.
pub async fn write(
    out: &mut (impl AsyncWrite + Unpin),
    message: &CastMessage,
) -> std::io::Result<()> {
    let body = message.encode();
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    out.write_all(&frame).await?;
    out.flush().await
}

/// Reads one message.
pub async fn read(input: &mut (impl AsyncRead + Unpin)) -> std::io::Result<CastMessage> {
    let length = input.read_u32().await?;
    if length > MAX_LENGTH {
        return Err(std::io::Error::other(format!(
            "a message of {length} bytes"
        )));
    }
    let mut body = vec![0; length as usize];
    input.read_exact(&mut body).await?;
    CastMessage::decode(&body).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_reads_back_as_it_was_written() {
        let message = CastMessage::new(
            "sender-0",
            "receiver-0",
            "urn:x-cast:com.google.cast.tp.heartbeat",
            r#"{"type":"PING"}"#.to_owned(),
        );
        let bytes = message.encode();
        assert_eq!(&bytes[..4], [0x08, 0x00, 0x12, 0x08]);
        assert_eq!(CastMessage::decode(&bytes).unwrap(), message);
    }

    #[test]
    fn fields_it_does_not_use_are_skipped() {
        let mut bytes = Vec::new();
        text(&mut bytes, 4, "urn:x-cast:test");
        // payload_binary, a fixed64 and a fixed32, as a newer device might send.
        text(&mut bytes, 7, "\u{1}\u{2}");
        varint(&mut bytes, (9 << 3) | 1);
        bytes.extend_from_slice(&[0; 8]);
        varint(&mut bytes, (10 << 3) | 5);
        bytes.extend_from_slice(&[0; 4]);
        text(&mut bytes, 6, "{}");
        let message = CastMessage::decode(&bytes).unwrap();
        assert_eq!(message.namespace, "urn:x-cast:test");
        assert_eq!(message.payload, "{}");
    }

    #[test]
    fn a_long_payload_takes_a_longer_length() {
        let payload = "x".repeat(300);
        let message = CastMessage::new("a", "b", "c", payload.clone());
        assert_eq!(
            CastMessage::decode(&message.encode()).unwrap().payload,
            payload
        );
        assert!(CastMessage::decode(&message.encode()[..20]).is_err());
    }

    #[tokio::test]
    async fn frames_carry_their_length() {
        let message = CastMessage::new("a", "b", "c", "{}".to_owned());
        let mut buffer = Vec::new();
        write(&mut buffer, &message).await.unwrap();
        assert_eq!(
            u32::from_be_bytes(buffer[..4].try_into().unwrap()) as usize,
            buffer.len() - 4
        );
        assert_eq!(read(&mut buffer.as_slice()).await.unwrap(), message);
    }
}
