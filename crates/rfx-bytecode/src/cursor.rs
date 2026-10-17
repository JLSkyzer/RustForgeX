//! Lecteur d'octets gros-boutiste qui refuse de lire au-dela de la fin.

use crate::ParseError;

pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], ParseError> {
        let end = self.at.checked_add(len).ok_or(ParseError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(ParseError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ParseError> {
        let raw = self.take(2)?;
        Ok(u16::from_be_bytes([raw[0], raw[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ParseError> {
        let raw = self.take(4)?;
        Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    /// Capacite a reserver pour `count` elements d'au moins `min_size` octets chacun :
    /// jamais plus que ce que les octets restants peuvent contenir. Un compteur invente
    /// ne fait donc pas reserver des gigaoctets.
    pub(crate) fn capacity_for(&self, count: usize, min_size: usize) -> usize {
        count.min(self.remaining() / min_size.max(1))
    }
}
