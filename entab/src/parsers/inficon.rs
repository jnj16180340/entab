use alloc::vec::Vec;
use alloc::{format, vec};
use core::convert::TryFrom;
use core::marker::Copy;

use crate::parsers::common::SeekPattern;
use crate::parsers::{extract, extract_opt, Endian, FromSlice};
use crate::record::StateMetadata;
use crate::EtError;
use crate::{impl_reader, impl_record};

/// The current state of the Inficon reader
#[derive(Clone, Debug, Default)]
pub struct InficonState {
    mz_segments: Vec<Vec<f64>>,
    data_left: usize,
    cur_time: f64,
    cur_mz: f64,
    cur_intensity: f64,
    cur_segment: usize,
    mzs_left: usize,
}

impl StateMetadata for InficonState {
    fn header(&self) -> Vec<&str> {
        vec!["time", "mz", "intensity"]
    }
}

impl<'b: 's, 's> FromSlice<'b, 's> for InficonState {
    type State = (Vec<Vec<f64>>, usize);

    fn parse(
        rb: &[u8],
        eof: bool,
        consumed: &mut usize,
        (mz_segments, data_left): &mut Self::State,
    ) -> Result<bool, EtError> {
        let con = &mut 0;

        if extract::<&[u8]>(rb, con, &mut 4)? != [4, 3, 2, 1] {
            return Err("Inficon file has bad magic bytes".into());
        }

        // probably not super robust, but it works? this appears at the end of
        // the "instrument collection steps" section and it appears to be
        // a constant distance before the "list of mzs" section
        if extract_opt::<SeekPattern>(rb, eof, con, &mut &b"\xFF\xFF\xFF\xFF\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xF6\xFF\xFF\xFF\x00\x00\x00\x00"[..])?.is_none() {
            return Err("Could not find m/z header list".into());
        }
        let _ = extract::<&[u8]>(rb, con, &mut 148)?;
        let n_segments = extract::<u32>(rb, con, &mut Endian::Little)? as usize;
        if n_segments > 10000 {
            return Err("Inficon file has too many segments".into());
        }

        // now read all of the collection segments
        *mz_segments = vec![Vec::new(); n_segments];
        for segment in mz_segments.iter_mut() {
            // first 4 bytes appear to be an name/identifier? not sure what
            // the rest is.
            let _ = extract::<&[u8]>(rb, con, &mut 96)?;
            let n_mzs = extract::<u32>(rb, con, &mut Endian::Little)?;
            if n_mzs > 100_000 {
                return Err("Too many m/z ranges".into());
            }
            for _ in 0..n_mzs {
                let start_mz = extract::<u32>(rb, con, &mut Endian::Little)?;
                let end_mz = extract::<u32>(rb, con, &mut Endian::Little)?;
                if end_mz > 4_000_000_000u32 {
                    // only malformed data should hit this
                    return Err("End of m/z range is invalid".into());
                }
                // then dwell time (u32; microseconds) and three more u32s
                let _ = extract::<&[u8]>(rb, con, &mut 16)?;
                let i_type = extract::<u32>(rb, con, &mut Endian::Little)?;
                let _ = extract::<&[u8]>(rb, con, &mut 4)?;
                if i_type == 0 {
                    // this is a SIM
                    segment.push(f64::from(start_mz) / 100.);
                } else {
                    if start_mz >= end_mz || end_mz - start_mz >= 200_000u32 {
                        return Err("m/z range is too big or invalid".into());
                    }
                    // i_type = 1 appears to be "full scan mode"
                    let mut mz = start_mz;
                    while mz < end_mz + 1 {
                        segment.push(f64::from(mz) / 100.);
                        mz += 100;
                    }
                }
            }
        }
        if extract_opt::<SeekPattern>(rb, eof, con, &mut &b"\xFF\xFF\xFF\xFFHapsGPIR"[..])?
            .is_none()
        {
            return Err("Could not find start of scan data".into());
        }
        // seek to right before the "HapsScan" section because the section
        // length is encoded in the four bytes before the header for that
        let _ = extract::<&[u8]>(rb, con, &mut 180)?;
        let data_length = u64::from(extract::<u32>(rb, con, &mut Endian::Little)?);
        let _ = extract::<&[u8]>(rb, con, &mut 8)?;
        if extract::<&[u8]>(rb, con, &mut 8)? != b"HapsScan" {
            return Err("Data header was malformed".into());
        }
        let _ = extract::<&[u8]>(rb, con, &mut 56)?;
        *data_left = usize::try_from(data_length)?;
        *consumed += *con;
        Ok(true)
    }

    fn get(&mut self, _rb: &[u8], (mz_segments, data_left): &Self::State) -> Result<(), EtError> {
        self.mz_segments = mz_segments.clone();
        self.data_left = *data_left;
        Ok(())
    }
}

/// A single record from an Inficon Hapsite file.
#[derive(Clone, Copy, Debug, Default)]
pub struct InficonRecord {
    time: f64,
    mz: f64,
    intensity: f64,
}

impl_record!(InficonRecord: time, mz, intensity);

impl<'b: 's, 's> FromSlice<'b, 's> for InficonRecord {
    type State = InficonState;

    fn parse(
        rb: &[u8],
        _eof: bool,
        consumed: &mut usize,
        state: &mut Self::State,
    ) -> Result<bool, EtError> {
        if state.data_left == 0 {
            return Ok(false);
        }
        let con = &mut 0;
        let mut mzs_left = state.mzs_left;
        if mzs_left == 0 {
            // the first u32 is the number of the record (i.e. from 1 to r_scans)
            let _ = extract::<u32>(rb, con, &mut Endian::Little)?;
            state.cur_time = f64::from(extract::<i32>(rb, con, &mut Endian::Little)?) / 60000.;
            // next value always seems to be 1
            let _ = extract::<u16>(rb, con, &mut Endian::Little)?;
            let n_mzs = usize::from(extract::<u16>(rb, con, &mut Endian::Little)?);
            // next value always seems to be 0xFFFF
            let _ = extract::<u16>(rb, con, &mut Endian::Little)?;
            // the segment is only contained in the top nibble? the bottom is
            // F (e.g. values seem to be 0x0F, 0x1F, 0x2F...)
            state.cur_segment = usize::from(extract::<u16>(rb, con, &mut Endian::Little)? >> 4);
            if state.cur_segment >= state.mz_segments.len() {
                return Err(
                    format!("Invalid segment number ({}) specified", state.cur_segment).into(),
                );
            }
            if n_mzs != state.mz_segments[state.cur_segment].len() {
                return Err(format!(
                    "Number of intensities ({}) doesn't match number of mzs ({})",
                    n_mzs,
                    state.mz_segments[state.cur_segment].len()
                )
                .into());
            }
            mzs_left = n_mzs;
        }
        state.cur_intensity = f64::from(extract::<f32>(rb, con, &mut Endian::Little)?);
        let cur_mz_segment = &state.mz_segments[state.cur_segment];
        if mzs_left > cur_mz_segment.len() {
            // i think this is probably more likely an error where mz_segments have 0 length, but I
            // don't know enough about the format above to know if we should error when we parse
            // the initial state instead of here.
            return Err("Invalid m/z segment".into());
        }
        state.cur_mz = cur_mz_segment[cur_mz_segment.len() - mzs_left];
        state.mzs_left = mzs_left - 1;
        state.data_left = state.data_left.saturating_sub(*con);
        *consumed += *con;
        Ok(true)
    }

    fn get(&mut self, _rb: &[u8], state: &Self::State) -> Result<(), EtError> {
        self.time = state.cur_time;
        self.mz = state.cur_mz;
        self.intensity = state.cur_intensity;
        Ok(())
    }
}

impl_reader!(
    InficonReader,
    InficonRecord,
    InficonRecord,
    InficonState,
    (Vec<Vec<f64>>, usize)
);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn bad_inficon_fuzzes() -> Result<(), EtError> {
        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0,
            0, 0, 14, 14, 14, 14, 14, 14, 14, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            248, 10, 10, 10, 10, 35, 4, 0, 0, 0, 0, 0, 0, 10, 10, 10, 10, 10, 62, 10, 10, 26, 0, 0,
            0, 42, 42, 4, 0, 0, 0, 0, 0, 0, 10, 10, 10, 10, 10, 62, 10, 10, 10, 0, 0, 0, 0, 0, 0,
            0, 16, 42, 42, 42, 10, 62, 10, 10, 26, 0, 0, 0, 42, 42, 4, 0, 0, 0, 0, 0, 0, 10, 10,
            10, 10, 10, 62, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 16, 42, 42, 42,
        ];
        assert!(InficonReader::new(&data[..], None).is_err());

        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 4, 1, 10, 255, 255, 255, 0, 3, 197, 65, 77, 1, 62, 1, 0, 0,
            255, 255, 255, 255, 255, 255, 62, 10, 10, 10, 10, 62, 10, 10, 10, 8, 10, 62, 10, 10,
            62, 10, 10, 10, 9, 10, 62, 10, 10, 62, 10, 10, 62, 26, 10, 10, 10, 45, 10, 59, 9, 0,
            255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 71, 71, 71, 71, 71, 38,
            200, 62, 10, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 116, 116, 116, 116, 246,
            245, 245, 240, 255, 255, 241, 0, 0, 0, 0, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 62, 10, 227, 205, 10, 10, 62, 10, 0, 62, 10, 10, 1, 0, 62, 10, 10, 34, 0, 0, 0,
            0, 0, 0, 0, 10, 10, 10, 10, 8, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 245, 10, 10, 10, 10, 240, 10, 62, 10, 10, 10, 42, 10, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 134, 134, 14,
            62, 10, 10, 62, 59, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0, 13, 10, 10,
            227, 59, 10, 10, 0, 10, 10, 62, 41, 0, 13, 10, 10, 10, 227, 10, 10, 62, 0, 13, 10, 10,
            10, 62, 10, 10, 8, 10, 62, 10, 10, 10, 10, 10, 62, 10, 10, 10, 62, 10, 10, 10, 10, 62,
            10, 10, 10, 9, 10, 62, 10, 10, 255, 255, 255, 175, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 10, 10, 10, 9, 10, 62, 45, 10, 59, 9, 0,
        ];
        assert!(InficonReader::new(&data[..], None).is_err());

        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 62, 1, 230, 255, 255, 251, 254, 254, 254,
            254, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 0, 10, 62, 10, 59, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 10, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255,
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13,
            10, 13, 227, 5, 62, 10, 227, 134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13,
            10, 10, 227, 10, 10, 62, 0, 13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13,
            10, 10, 227, 43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10,
            10, 227, 59, 10, 10, 0, 10, 10, 10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10,
            10, 10, 10, 14, 10, 255, 255, 255, 255, 176, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 175, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 245, 240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35,
            116, 116, 116, 246, 245, 245, 240, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227, 43, 10,
            10, 10, 10, 10, 10, 47, 59, 10, 10, 4, 3, 2, 1, 83, 80, 181, 181, 181, 181, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255,
            255, 255, 255, 255, 58, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 122, 255, 255, 255,
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10,
            10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 116, 116, 246, 245, 245, 240,
        ];
        assert!(InficonReader::new(&data[..], None).is_err());

        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 168, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 10, 26, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            246, 255, 255, 255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13, 10, 13, 227,
            5, 62, 10, 227, 134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227,
            10, 10, 62, 0, 13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227,
            43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 227, 59,
            10, 10, 0, 10, 10, 10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10,
            14, 10, 255, 10, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 181, 181, 181, 181,
            0, 0, 0, 0, 0, 0, 0, 83, 55, 159, 159, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 227, 43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 10, 10, 62, 42, 10,
            10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0, 13, 10, 10, 227, 59, 10, 10, 250, 255,
            10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10, 10, 0, 10, 10, 10, 10, 26, 10, 10, 41,
            0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10, 10, 10, 10, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 245, 240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35,
            116, 116, 116, 246, 245, 245, 240, 10, 10, 10, 10, 14, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 245, 240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35, 116, 246, 245,
            245, 240,
        ];
        assert!(InficonReader::new(&data[..], None).is_err());

        Ok(())
    }

    #[test]
    fn slow_inficon_fuzzes() -> Result<(), EtError> {
        let test_data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 140, 130, 127, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255,
            255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13, 10, 13, 227, 5, 62, 10, 227,
            134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0,
            13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10,
            10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 227, 59, 10, 10, 0, 10, 10,
            10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10,
            10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 181, 181, 181, 181, 0, 0, 0, 0, 0, 0,
            0, 83, 51, 159, 159, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 175, 255, 255, 255, 10, 10, 62, 0,
            13, 10, 10, 220, 227, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 10, 59, 10, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            15, 230, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 227, 59, 10, 10,
            250, 255, 10, 62, 41, 0, 13, 10, 10, 39, 212, 245, 245, 10, 10, 10, 10, 47, 59, 10, 10,
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 62, 1, 0, 0, 0, 6, 2, 254, 254, 254, 168,
            168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168,
            168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 1,
            0, 0, 0, 0, 0, 3, 70, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168,
            240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35, 116, 116, 116, 246, 245,
            245, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 39, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 240,
        ];
        assert!(InficonReader::new(&test_data[..], None).is_err());

        let test_data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 140, 130, 127, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 127, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0,
            0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13, 10, 13, 227, 5, 62, 10, 227, 134,
            134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0, 13, 10,
            10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10, 10, 10,
            10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 227, 59, 10, 10, 0, 10, 10, 10, 10,
            26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10, 10, 10,
            10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 181, 181, 181, 181, 0, 0, 0, 0, 0, 0, 0, 83,
            51, 159, 159, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 175, 255, 255, 255, 10, 10, 62, 0, 13, 10, 10,
            220, 227, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 10, 59, 10, 10, 10, 10, 10, 10,
            10, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 15, 0, 0,
            230, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 227, 59, 10, 10, 250, 255,
            10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 4, 3, 2, 1,
            83, 80, 65, 72, 66, 65, 77, 1, 62, 1, 0, 0, 0, 6, 2, 254, 254, 254, 168, 168, 168, 168,
            168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168,
            168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 1, 0, 0, 3, 0, 0,
            0, 70, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 240, 255, 255,
            255, 255, 255, 169, 77, 86, 139, 139, 116, 35, 116, 116, 116, 246, 245, 245, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 64, 19, 2, 0, 87, 10, 10, 43, 10, 10, 64, 62, 0, 87, 10, 10, 43, 10, 10,
            64, 62, 62, 0, 0, 87, 10, 10, 43, 10, 10, 64, 62, 62, 87, 42, 10, 42, 2, 10, 43, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 138, 2, 10, 64, 138, 116, 64, 39, 10, 10, 43, 10, 10, 64, 62, 62, 87,
            42, 0, 87, 10, 10, 43, 10, 10, 64, 10, 10, 43, 10, 237, 237, 237, 240, 10, 64, 62, 91,
            62, 87, 42, 10, 42, 2, 10, 43, 138, 116, 115, 2, 10, 64, 138, 116, 64, 39, 10, 10, 43,
            10, 10, 64, 62, 62, 62, 87, 0, 87, 0, 42, 10, 10, 43, 10, 10, 64, 62, 62, 42, 10, 42,
            2, 10, 43, 2, 10, 64, 138, 116, 64, 39, 10, 10, 43, 10, 10, 64, 62, 62, 87, 0, 87, 255,
            255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13, 10, 13, 227, 5, 62, 10, 227,
            134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0,
            13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10,
            10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 227, 59, 10, 10, 0, 10, 10,
            10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10,
            10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 181, 181, 181, 181, 0, 0, 0, 0, 0, 0,
            0, 83, 51, 159, 159, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 62, 87, 0, 87, 10, 10, 43, 10, 10,
            64, 62, 62, 0, 87, 10, 10, 43, 10, 10, 64, 42, 10, 42, 2, 10, 43, 138, 116, 115, 2, 10,
            64, 138, 116, 64, 39, 10, 10, 43, 10, 10, 231, 62, 62, 87, 116, 115, 2,
        ];
        assert!(InficonReader::new(&test_data[..], None).is_err());

        let test_data = [
            4, 3, 2, 1, 255, 255, 255, 255, 0, 0, 0, 0, 203, 203, 203, 203, 203, 203, 203, 203,
            203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203, 203,
            203, 203, 203, 199, 10, 0, 2, 0, 0, 0, 0, 0, 0, 248, 255, 255, 255, 255, 255, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            246, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 92, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 248, 255, 53, 161, 161, 0, 161, 161, 161, 161, 161,
            161, 161, 161, 161, 161, 161, 1, 64, 0, 0, 0, 0, 0, 0, 169, 161, 235, 203, 1, 0, 22, 0,
            0, 203, 203, 203, 203, 203, 255, 255, 203, 203, 203, 40, 203, 0, 0, 0, 0, 0, 0, 169,
            161, 235, 203, 1, 0, 22, 0, 0, 203, 203, 203, 203, 203, 255, 255, 203, 203, 203, 203,
            203, 203, 203, 203, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 169, 161, 235, 203, 1, 0, 22,
            0, 0, 203, 203, 203, 203, 203, 255, 255, 203, 203, 203, 203, 203, 203, 139, 203, 0, 0,
            0, 0, 0, 0, 203, 1, 0, 0, 0, 0, 0, 0, 16, 203, 139, 139, 203, 1, 0, 0, 0, 0, 0, 1, 20,
            203, 235, 93, 0, 0, 227, 227, 227, 227, 227, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0,
            203, 1, 0, 0, 0, 0, 0, 0, 16, 203, 139, 139, 203, 1, 0, 0, 0, 0, 0, 1, 20, 172, 203,
            235, 93, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 8, 0, 0, 248, 2, 0, 0,
            0, 0, 0, 0, 0, 161, 161, 161, 161, 161, 161, 161, 161, 1, 64, 0, 0, 0, 0, 0, 0, 169,
            161, 235, 203, 1, 0, 22, 0, 0, 203, 203, 203, 203, 203, 255, 255, 203, 203, 203, 40,
            203, 0, 0, 0, 0, 0, 0, 169, 161, 235, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0,
            248, 255, 53, 161, 161, 0, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 1,
            64, 0, 0, 0, 0, 0, 0, 169, 161, 235, 203, 1, 0, 22, 0, 0, 203, 203, 203, 203, 203, 255,
            255, 203, 203, 203, 40, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 248, 2, 0, 0, 0, 0, 0,
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 48, 161, 161, 0, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161, 161,
            1, 64, 0, 0, 0, 0, 0, 0, 169, 161, 235, 203, 1, 0, 14, 0, 0, 203, 203, 203, 203, 203,
            255, 255, 203, 203, 203, 40, 203, 0, 0, 0, 0, 0, 92, 0, 0, 9, 0, 0, 0, 0, 0, 0, 246,
            255, 255, 255, 0, 0, 0, 0, 2, 0,
        ];
        assert!(InficonReader::new(&test_data[..], None).is_err());

        let test_data = [
            4, 3, 2, 1, 10, 0, 0, 0, 0, 0, 0, 0, 0, 14, 14, 7, 0, 250, 0, 0, 0, 6, 0, 0, 0, 0, 255,
            255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 203, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 246, 2, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 47,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 45, 0, 0, 0, 0, 0, 0, 255, 2,
            0, 0, 0, 10, 183, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 163, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 72, 97,
            112, 115, 71, 80, 73, 82, 0, 0, 0, 0, 0, 0, 154, 154, 154, 154, 154, 154, 154, 154,
            154, 154, 154, 154, 154, 154, 154, 154, 154, 0, 0, 0, 0, 0, 161, 161, 161, 161, 161,
            161, 161, 161, 161, 161, 161, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 255, 254, 0, 246, 255, 0, 0, 0, 0, 154, 255, 255, 255, 255, 255, 255, 255, 255,
            154, 154, 154, 154, 154, 154, 154, 154, 154, 161, 161, 161, 161, 161, 161, 161, 161, 0,
            0, 0, 0, 6, 0, 0,
        ];
        assert!(InficonReader::new(&test_data[..], None).is_err());

        let test_data = [
            4, 3, 2, 1, 54, 54, 54, 93, 54, 54, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255,
            0, 0, 0, 0, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250,
            250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 218, 250, 250, 250,
            250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250,
            250, 250, 250, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 3, 2, 1, 0, 0, 0, 0, 4, 3,
            2, 255, 245, 255, 0, 84, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 1, 64, 43, 43, 64, 0, 54, 54, 54, 93, 54, 54, 255,
            255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 2, 1, 93,
            54, 54, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 250, 250, 250, 0,
            0, 250, 250, 126, 250, 250, 250, 162, 1, 0, 0, 0, 0, 0, 0, 250, 250, 250, 250, 250,
            250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 254, 255, 255, 4, 250, 250, 250,
            4, 3, 2, 43, 43, 42, 37, 124, 40, 10, 10, 53, 254, 255, 255, 4, 54, 54, 54, 54, 54, 54,
            54, 54, 54, 54, 54, 49, 54, 54, 54, 93, 54, 55, 255, 255, 253, 255, 0, 33, 0, 0, 0, 0,
            5, 0, 0, 0, 0, 0, 0, 251, 0, 0, 0, 0, 0, 0, 244, 255, 255, 255, 0, 0, 0, 0, 250, 0,
            134, 160, 255, 255, 255, 255, 72, 97, 112, 115, 71, 80, 73, 82, 0, 63, 4, 3, 2, 255,
            245, 40, 54, 255, 93, 54, 54, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40, 10,
            10, 53, 254, 255, 255, 4, 54, 54, 54, 54, 54, 54, 54, 54, 54, 54, 54, 0, 0, 246, 255,
            255, 255, 0, 0, 0, 0, 250, 250, 250, 0, 0, 250, 250, 250, 250, 126, 250, 250, 250, 250,
            250, 250, 250, 250, 218, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250,
            250, 250, 250, 250, 254, 255, 255, 4, 250, 250, 250, 4, 3, 2, 43, 43, 42, 37, 124, 40,
            10, 10, 53, 254, 255, 255, 4, 54, 54, 54, 54, 54, 54, 54, 54, 54, 54, 54, 49, 54, 54,
            54, 93, 54, 54, 255, 2, 255, 255, 247, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 33, 0, 0, 0, 0, 0, 0, 57, 57, 57, 57, 0, 0, 0, 0, 72, 97, 112, 115, 83, 99,
            97, 110, 0, 0, 0, 0, 0, 250, 251, 0, 4, 0, 250, 250, 250, 250, 250, 250, 250, 250, 250,
            250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 250, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 4, 3, 2, 1, 0, 0, 0, 0, 4, 3, 2, 255, 245, 255, 0, 84, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 54, 55, 255, 255, 253, 255, 0, 33, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 251,
            0, 0, 0, 0, 0, 0, 244, 255, 255, 255, 0, 0, 0, 0, 250, 0, 134, 160, 0, 0, 0, 0, 0, 0,
            0, 0, 4, 0, 49, 54, 0, 0, 0, 0, 0, 250, 0, 0, 0,
        ];
        let mut reader = InficonReader::new(&test_data[..], None)?;
        while reader.next()?.is_some() {}

        Ok(())
    }
}
