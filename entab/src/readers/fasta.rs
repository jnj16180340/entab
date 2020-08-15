use alloc::vec::Vec;

use memchr::{memchr, memchr_iter};

use crate::buffer::ReadBuffer;
use crate::readers::RecordReader;
use crate::record::Record;
use crate::EtError;

use alloc::borrow::Cow;

pub struct FastaRecord<'r> {
    id: &'r str,
    sequence: Cow<'r, [u8]>,
}

use crate::buffer::FromBuffer;


impl<'r> FromBuffer<'r> for Option<FastaRecord<'r>> {
    type State = ();

    fn get(rb: &'r mut ReadBuffer, _amt: Self::State) -> Result<Self, EtError> {
        if rb.is_empty() {
            return Ok(None);
        }
        if rb[0] != b'>' {
            return Err(EtError::new("Valid FASTA records start with '>'"));
        }
        let mut seq_newlines: Vec<usize> = Vec::new();
        let (header_range, seq_range, rec_end) = loop {
            let (header_end, seq_start) = if let Some(p) = memchr(b'\n', &rb[..]) {
                if p > 0 && rb[p - 1] == b'\r' {
                    // strip out the \r too if this is a \r\n ending
                    (p - 1, p + 1)
                } else {
                    (p, p + 1)
                }
            } else if rb.eof() {
                return Err("Incomplete record".into());
            } else {
                rb.refill()?;
                continue;
            };
            let mut found_end = false;
            for raw_pos in memchr_iter(b'\n', &rb[seq_start..]) {
                let pos = seq_start + raw_pos;
                if pos > 0 && rb[pos - 1] == b'\r' {
                    seq_newlines.push(raw_pos - 1);
                }
                seq_newlines.push(raw_pos);
                if pos + 1 < rb.len() && rb[pos + 1] == b'>' {
                    found_end = true;
                    break;
                }
            }
            if !found_end && !rb.eof() {
                rb.refill()?;
                seq_newlines.truncate(0);
                continue;
            }
            let (seq_end, rec_end) = if found_end {
                // found_end only happens if we added a newline
                // so the pop is safe to unwrap
                let mut endpos = seq_newlines.pop().unwrap();
                let rec_end = seq_start + endpos + 1;

                // remove trailing consecutive newlines (e.g. \r\n)
                // from the end
                while endpos > 0 && seq_newlines.last() == Some(endpos - 1).as_ref() {
                    endpos = seq_newlines.pop().unwrap();
                }
                (seq_start + endpos, rec_end)
            } else {
                // at eof; just return the end
                (rb.len(), rb.len())
            };
            break (1..header_end, seq_start..seq_end, rec_end);
        };

        let record = rb.consume(rec_end);

        let header = &record[header_range];
        let raw_sequence = &record[seq_range];
        let sequence = if seq_newlines.is_empty() {
            raw_sequence.into()
        } else {
            let mut new_buf = Vec::with_capacity(raw_sequence.len() - seq_newlines.len());
            let mut start = 0;
            for pos in seq_newlines {
                new_buf.extend_from_slice(&raw_sequence[start..pos]);
                start = pos + 1;
            }
            new_buf.extend_from_slice(&raw_sequence[start..]);
            new_buf.into()
        };

        Ok(Some(FastaRecord {
            id: alloc::str::from_utf8(header)?,
            sequence,
        }))
    }
}

pub struct FastaReader<'r> {
    rb: ReadBuffer<'r>,
}

impl<'r> FastaReader<'r> {
    pub fn new(rb: ReadBuffer<'r>) -> Result<Self, EtError> {
        Ok(FastaReader { rb })
    }
}

impl<'r> RecordReader for FastaReader<'r> {
    fn next(&mut self) -> Result<Option<Record>, EtError> {
        Ok(self.rb.extract::<Option<FastaRecord>>(())?.map(|r: FastaRecord| {
            Record::Sequence {
                id: r.id,
                sequence: r.sequence,
                quality: None,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;

    use super::*;
    use crate::buffer::ReadBuffer;

    #[test]
    fn test_fasta_reading() -> Result<(), EtError> {
        const TEST_FASTA: &[u8] = b">id\nACGT\n>id2\nTGCA";
        let rb = ReadBuffer::from_slice(TEST_FASTA);
        let mut pt = FastaReader::new(rb)?;

        let mut ix = 0;
        while let Some(Record::Sequence { id, sequence, .. }) = pt.next()? {
            match ix {
                0 => {
                    assert_eq!(id, "id");
                    assert_eq!(sequence, Cow::Borrowed(&b"ACGT"[..]));
                }
                1 => {
                    assert_eq!(id, "id2");
                    assert_eq!(sequence, Cow::Borrowed(&b"TGCA"[..]));
                }
                _ => return Err("bad line".into()),
            }
            ix += 1;
        }
        assert_eq!(ix, 2);
        Ok(())
    }

    #[test]
    fn test_fasta_multiline() -> Result<(), EtError> {
        const TEST_FASTA: &[u8] = b">id\nACGT\nAAAA\n>id2\nTGCA";
        let rb = ReadBuffer::from_slice(TEST_FASTA);
        let mut pt = FastaReader::new(rb)?;

        if let Record::Sequence {
            id,
            sequence,
            quality,
        } = pt.next()?.expect("first record present")
        {
            assert_eq!(id, "id");
            assert_eq!(sequence, Cow::Owned::<[u8]>(b"ACGTAAAA".to_vec()));
            assert_eq!(quality, None);
        } else {
            panic!("FASTA reader returned non-FASTA record");
        }

        if let Record::Sequence {
            id,
            sequence,
            quality,
        } = pt.next()?.expect("second record present")
        {
            assert_eq!(id, "id2");
            assert_eq!(sequence, Cow::Borrowed(b"TGCA"));
            assert_eq!(quality, None);
        } else {
            panic!("FASTA reader returned non-FASTA record");
        }

        assert!(pt.next()?.is_none());
        Ok(())
    }

    #[test]
    fn test_fasta_multiline_extra_newlines() -> Result<(), EtError> {
        const TEST_FASTA: &[u8] = b">id\r\nACGT\r\nAAAA\r\n>id2\r\nTGCA\r\n";
        let rb = ReadBuffer::from_slice(TEST_FASTA);
        let mut pt = FastaReader::new(rb)?;

        if let Record::Sequence {
            id,
            sequence,
            quality,
        } = pt.next()?.expect("first record present")
        {
            assert_eq!(id, "id");
            assert_eq!(sequence, Cow::Owned::<[u8]>(b"ACGTAAAA".to_vec()));
            assert_eq!(quality, None);
        } else {
            panic!("FASTA reader returned non-FASTA record");
        }

        if let Record::Sequence {
            id,
            sequence,
            quality,
        } = pt.next()?.expect("second record present")
        {
            assert_eq!(id, "id2");
            assert_eq!(sequence, Cow::Borrowed(b"TGCA"));
            assert_eq!(quality, None);
        } else {
            panic!("FASTA reader returned non-FASTA record");
        }

        assert!(pt.next()?.is_none());
        Ok(())
    }

    #[test]
    fn test_fasta_empty_fields() -> Result<(), EtError> {
        const TEST_FASTA: &[u8] = b">hd\n\n>\n\n";
        let rb = ReadBuffer::from_slice(TEST_FASTA);
        let mut pt = FastaReader::new(rb)?;

        if let Record::Sequence {
            id,
            sequence,
            quality,
        } = pt.next()?.expect("first record present")
        {
            assert_eq!(id, "hd");
            assert_eq!(sequence, Cow::Borrowed(b""));
            assert_eq!(quality, None);
        } else {
            panic!("FASTA reader returned non-FASTA record");
        }

        if let Record::Sequence {
            id,
            sequence,
            quality,
        } = pt.next()?.expect("second record present")
        {
            assert_eq!(id, "");
            assert_eq!(sequence, Cow::Borrowed(b""));
            assert_eq!(quality, None);
        } else {
            panic!("FASTA reader returned non-FASTA record");
        }

        assert!(pt.next()?.is_none());
        Ok(())
    }
}
