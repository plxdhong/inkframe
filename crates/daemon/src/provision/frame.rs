//! Message fragmentation over GATT writes/notifications.
//!
//! Each fragment is one header byte followed by payload. Header bit 0 set
//! means more fragments follow; all other bits are reserved and must be 0.

pub const MORE: u8 = 0x01;
/// Largest reassembled message; anything bigger is a protocol error.
pub const MAX_MESSAGE: usize = 4096;
/// Fragment payload when the MTU is unknown (default ATT MTU 23 − 3 − header).
pub const MIN_CHUNK: usize = 19;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("empty fragment")]
    Empty,
    #[error("reserved header bits set")]
    Reserved,
    #[error("message exceeds {MAX_MESSAGE} bytes")]
    TooLong,
}

/// Splits `msg` into fragments of at most `max_fragment` bytes (header included).
pub fn fragment(msg: &[u8], max_fragment: usize) -> Vec<Vec<u8>> {
    let chunk = max_fragment.saturating_sub(1).max(MIN_CHUNK);
    let pieces: Vec<&[u8]> = if msg.is_empty() {
        vec![&[][..]]
    } else {
        msg.chunks(chunk).collect()
    };
    let last = pieces.len() - 1;
    pieces
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut f = Vec::with_capacity(p.len() + 1);
            f.push(if i < last { MORE } else { 0 });
            f.extend_from_slice(p);
            f
        })
        .collect()
}

#[derive(Default)]
pub struct Reassembler {
    buf: Vec<u8>,
}

impl Reassembler {
    /// Feeds one fragment; returns the full message once the last one arrives.
    /// On error the partial message is discarded.
    pub fn push(&mut self, fragment: &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
        let Some((&header, payload)) = fragment.split_first() else {
            self.buf.clear();
            return Err(FrameError::Empty);
        };
        if header & !MORE != 0 {
            self.buf.clear();
            return Err(FrameError::Reserved);
        }
        if self.buf.len() + payload.len() > MAX_MESSAGE {
            self.buf.clear();
            return Err(FrameError::TooLong);
        }
        self.buf.extend_from_slice(payload);
        if header & MORE != 0 {
            Ok(None)
        } else {
            Ok(Some(std::mem::take(&mut self.buf)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_various_sizes() -> Result<(), FrameError> {
        for len in [0, 1, 18, 19, 20, 243, 244, 1000, MAX_MESSAGE] {
            let msg: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let frags = fragment(&msg, 244);
            assert!(frags.iter().all(|f| f.len() <= 244));
            let mut r = Reassembler::default();
            let mut out = None;
            for f in &frags {
                out = r.push(f)?;
            }
            assert_eq!(out.as_deref(), Some(msg.as_slice()), "len {len}");
        }
        Ok(())
    }

    #[test]
    fn rejects_bad_input_and_recovers() {
        let mut r = Reassembler::default();
        assert_eq!(r.push(&[]), Err(FrameError::Empty));
        assert_eq!(r.push(&[0x80, 1]), Err(FrameError::Reserved));
        let big = vec![0u8; 250];
        let mut err = None;
        for _ in 0..20 {
            let mut f = vec![MORE];
            f.extend_from_slice(&big);
            if let Err(e) = r.push(&f) {
                err = Some(e);
                break;
            }
        }
        assert_eq!(err, Some(FrameError::TooLong));
        assert_eq!(r.push(&[0, b'x']), Ok(Some(b"x".to_vec())));
    }
}
