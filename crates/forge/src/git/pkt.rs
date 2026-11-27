//! Packet-line framing (gix-packetline) for Smart HTTP request and response
//! bodies, which this server holds in memory.

use gix_packetline::blocking_io::encode;
use gix_packetline::{Channel, PacketLineRef};

/// Largest payload of one side-band packet: 65516 data bytes per pkt-line
/// minus the band byte.
const MAX_BAND_DATA: usize = 65515;

#[derive(Debug, thiserror::Error)]
pub enum PktError {
    #[error("malformed pkt-line")]
    Decode(#[source] gix::Error),
    #[error("request body ends inside a pkt-line")]
    Truncated,
    #[error("unexpected pkt-line: {0}")]
    Unexpected(String),
}

/// One decoded packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pkt<'a> {
    Data(&'a [u8]),
    Flush,
    Delim,
    ResponseEnd,
}

impl<'a> Pkt<'a> {
    /// Data with one trailing newline removed, as text lines are framed.
    pub fn line(&self) -> Option<&'a [u8]> {
        match self {
            Pkt::Data(d) => Some(d.strip_suffix(b"\n").unwrap_or(d)),
            _ => None,
        }
    }
}

/// Reads packets from an in-memory body; [`PktReader::rest`] is whatever
/// follows (a raw packfile after receive-pack commands).
pub struct PktReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PktReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        PktReader { data, pos: 0 }
    }

    /// The next packet, or `None` at the end of the body.
    pub fn next_pkt(&mut self) -> Result<Option<Pkt<'a>>, PktError> {
        let remaining = &self.data[self.pos..];
        if remaining.is_empty() {
            return Ok(None);
        }
        match gix_packetline::decode::streaming(remaining).map_err(PktError::Decode)? {
            gix_packetline::decode::Stream::Complete {
                line,
                bytes_consumed,
            } => {
                self.pos += bytes_consumed;
                Ok(Some(match line {
                    PacketLineRef::Data(d) => Pkt::Data(d),
                    PacketLineRef::Flush => Pkt::Flush,
                    PacketLineRef::Delimiter => Pkt::Delim,
                    PacketLineRef::ResponseEnd => Pkt::ResponseEnd,
                }))
            }
            gix_packetline::decode::Stream::Incomplete { .. } => Err(PktError::Truncated),
        }
    }

    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
}

/// Builds a response body of packets.
#[derive(Default)]
pub struct PktWriter {
    buf: Vec<u8>,
}

impl PktWriter {
    pub fn new() -> Self {
        PktWriter::default()
    }

    pub fn data(&mut self, data: &[u8]) -> &mut Self {
        encode::data_to_write(data, &mut self.buf).expect("writing to a Vec cannot fail");
        self
    }

    /// A text line, newline-terminated.
    pub fn line(&mut self, text: impl AsRef<str>) -> &mut Self {
        let mut bytes = text.as_ref().as_bytes().to_vec();
        if bytes.last() != Some(&b'\n') {
            bytes.push(b'\n');
        }
        self.data(&bytes)
    }

    pub fn flush(&mut self) -> &mut Self {
        encode::flush_to_write(&mut self.buf).expect("writing to a Vec cannot fail");
        self
    }

    pub fn delim(&mut self) -> &mut Self {
        encode::delim_to_write(&mut self.buf).expect("writing to a Vec cannot fail");
        self
    }

    /// `data` on a side-band channel, split into maximal packets.
    pub fn band(&mut self, channel: Channel, data: &[u8]) -> &mut Self {
        for chunk in data.chunks(MAX_BAND_DATA) {
            encode::band_to_write(channel, chunk, &mut self.buf)
                .expect("writing to a Vec cannot fail");
        }
        self
    }

    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut w = PktWriter::new();
        w.line("want abc").delim().data(b"x").flush().raw(b"PACK");
        let bytes = w.into_bytes();
        assert_eq!(&bytes[..13], b"000dwant abc\n");
        let mut r = PktReader::new(&bytes);
        assert_eq!(
            r.next_pkt().unwrap().unwrap().line(),
            Some(&b"want abc"[..])
        );
        assert_eq!(r.next_pkt().unwrap(), Some(Pkt::Delim));
        assert_eq!(r.next_pkt().unwrap(), Some(Pkt::Data(b"x")));
        assert_eq!(r.next_pkt().unwrap(), Some(Pkt::Flush));
        assert_eq!(r.rest(), b"PACK");
    }

    #[test]
    fn large_band_data_is_split() {
        let mut w = PktWriter::new();
        w.band(Channel::Data, &vec![7u8; MAX_BAND_DATA + 10]);
        let bytes = w.into_bytes();
        let mut r = PktReader::new(&bytes);
        let first = r.next_pkt().unwrap().unwrap();
        let second = r.next_pkt().unwrap().unwrap();
        assert!(matches!(first, Pkt::Data(d) if d.len() == MAX_BAND_DATA + 1));
        assert!(matches!(second, Pkt::Data(d) if d.len() == 11));
    }
}
