use alloc::format;
use alloc::string::{String, ToString};
use core::marker::Copy;

/// A file format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FileType {
    // compression
    /// Gz/Gzip compression container
    Gzip,
    /// Bz/Bzip compression container
    Bzip,
    /// Xz/Lzma compression container
    Lzma,
    /// Zstd compression container
    Zstd,
    // bioinformatics
    /// "Binary alignment map" data. Compressed version of SAM.
    Bam,
    /// FASTA sequence data
    Fasta,
    /// FASTQ sequence data
    Fastq,
    /// Flow-cytometry data
    Facs,
    /// "Sequence alignment map" data.
    Sam,
    /// DNA sequencing trace format
    Scf, // http://staden.sourceforge.net/manual/formats_unix_2.html
    /// DNA sequencing chromatogram trace format
    Ztr, // http://staden.sourceforge.net/manual/formats_unix_12.html
    // chemoinformatics
    /// Agilent format used for MS-MS trace data
    AgilentMsMsScan, // bin   0x01, 0x01
    /// Agilent format used for flame ionization trace data
    AgilentChemstationFid,
    /// Agilent format used for mass spectrometry trace data
    AgilentChemstationMs,
    /// Agilent format used for moving wavelength detector trace data
    AgilentChemstationMwd,
    /// Agilent format used for UV-visible detector trace data
    AgilentChemstationUv,
    /// Agilent format used for diode array detector trace data
    AgilentDad, // sd
    /// Bruker format
    BrukerBaf,
    /// Bruker format
    BrukerMsms,
    /// Inficon mass spectrometry format
    InficonHapsite,
    /// Thermo/Bruker mass spectrometry format
    ThermoRaw,
    /// Thermo isotope mass spectrometry format
    ThermoCf,
    /// Thermo isotope mass spectrometry format
    ThermoDxf,
    /// Waters isotope mass spectrometry format
    WatersAutospec,
    /// Vendor-independent MS file format based on HDF
    NetCdf,
    /// Vendor-independent MS file format based on XML
    MzXml,
    // geology
    /// "Log ASCII Standard" format for well log information
    Las,
    // catch all
    /// Portable Network Graphics image format
    Png,
    /// Generic scientific data format
    Hdf5,
    /// Tab- or comma-seperated value format
    DelimitedText(u8),
    /// Unknown file type
    Unknown,
}

impl FileType {
    /// Given a slice from the beginning of the file, try to guess which file
    /// format that file is in.
    #[must_use]
    pub fn from_magic(magic: &[u8]) -> FileType {
        if magic.len() > 8 {
            match &magic[..8] {
                b"FCS2.0  " | b"FCS3.0  " | b"FCS3.1  " => return FileType::Facs,
                b"~VERSION" | b"~Version" => return FileType::Las,
                b"\x89PNG\r\n\x1A\n" => return FileType::Png,
                b"\x89HDF\r\n\x1A\n" => return FileType::Hdf5,
                b"\x04\x03\x02\x01SPAH" => return FileType::InficonHapsite,
                b"\xAEZTR\x0D\x0A\x1A\x0A" => return FileType::Ztr,
                b"\x01\xA1F\x00i\x00n\x00" => return FileType::ThermoRaw,
                _ => {}
            }
        }
        if magic.len() > 4 {
            match &magic[..4] {
                b"BAM\x01" => return FileType::Bam,
                b"@HD\t" | b"@SQ\t" => return FileType::Sam,
                b"\x2Escf" => return FileType::Scf,
                [0x02, 0x38, 0x31, 0x00] => return FileType::AgilentChemstationFid,
                [0x01, 0x32, 0x00, 0x00] => return FileType::AgilentChemstationMs,
                [0x02, 0x33, 0x30, 0x00] => return FileType::AgilentChemstationMwd,
                [0x03, 0x31, 0x33, 0x31] => return FileType::AgilentChemstationUv,
                [0x28, 0xB5, 0x2F, 0xFD] => return FileType::Zstd,
                [0xFF, 0xFF, 0x06 | 0x05, 0x00] => {
                    if magic.len() >= 78 && &magic[52..64] == b"C\x00I\x00s\x00o\x00G\x00C\x00" {
                        return FileType::ThermoCf;
                    }
                    return FileType::ThermoDxf;
                }
                _ => {}
            }
        }
        if magic.len() < 2 {
            return FileType::Unknown;
        }
        match &magic[..2] {
            [0x0F | 0x1F, 0x8B] => return FileType::Gzip,
            [0x42, 0x5A] => return FileType::Bzip,
            [0xFD, 0x37] => return FileType::Lzma,
            [0x24, 0x00] => return FileType::BrukerBaf,
            [0x43, 0x44] => return FileType::NetCdf,
            _ => {}
        }
        match &magic[..1] {
            b">" => FileType::Fasta,
            b"@" => FileType::Fastq,
            _ => FileType::Unknown,
        }
    }

    /// Return the list of possible file extensions a given file format
    /// could have.
    #[must_use]
    pub fn from_extension(ext: &str) -> &[Self] {
        match ext {
            "gz" | "gzip" => &[FileType::Gzip],
            "bz" | "bz2" | "bzip" => &[FileType::Bzip],
            "xz" => &[FileType::Lzma],
            "zstd" => &[FileType::Zstd],
            "ch" => &[
                FileType::AgilentChemstationFid,
                FileType::AgilentChemstationMwd,
            ],
            "ms" => &[FileType::AgilentChemstationMs],
            "uv" => &[FileType::AgilentChemstationUv],
            "bam" => &[FileType::Bam],
            "baf" => &[FileType::BrukerBaf],
            "ami" => &[FileType::BrukerMsms],
            "fcs" | "lmd" => &[FileType::Facs],
            "fa" | "faa" | "fasta" | "fna" => &[FileType::Fasta],
            "faq" | "fastq" | "fq" => &[FileType::Fastq],
            "hdf" => &[FileType::Hdf5],
            "raw" => &[FileType::ThermoRaw],
            "mzxml" => &[FileType::MzXml],
            "cdf" => &[FileType::NetCdf],
            "png" => &[FileType::Png],
            "hps" => &[FileType::InficonHapsite],
            "sam" => &[FileType::Sam],
            "scf" => &[FileType::Scf],
            "cf" => &[FileType::ThermoCf],
            "dxf" => &[FileType::ThermoDxf],
            "idx" => &[FileType::WatersAutospec],
            "ztr" => &[FileType::Ztr],
            _ => &[FileType::Unknown],
        }
    }

    /// Returns the "best" parser for a given file
    #[must_use]
    pub fn from_parser_name(parser_name: &str) -> Self {
        match parser_name {
            "chemstation_fid" => FileType::AgilentChemstationFid,
            "chemstation_ms" => FileType::AgilentChemstationMs,
            "chemstation_mwd" => FileType::AgilentChemstationMwd,
            "chemstation_uv" => FileType::AgilentChemstationUv,
            "csv" => FileType::DelimitedText(b','),
            "bam" => FileType::Bam,
            "fcs" => FileType::Facs,
            "fasta" => FileType::Fasta,
            "fastq" => FileType::Fastq,
            "inficon" => FileType::InficonHapsite,
            "png" => FileType::Png,
            "sam" => FileType::Sam,
            "thermo_cf" => FileType::ThermoCf,
            "thermo_dxf" => FileType::ThermoDxf,
            "tsv" => FileType::DelimitedText(b'\t'),
            _ => FileType::Unknown,
        }
    }

    /// Returns the "parser name" associated with this file type
    #[must_use]
    pub fn to_parser_name(&self) -> String {
        match self {
            FileType::AgilentChemstationFid => "chemstation_fid",
            FileType::AgilentChemstationMs => "chemstation_ms",
            FileType::AgilentChemstationMwd => "chemstation_mwd",
            FileType::AgilentChemstationUv => "chemstation_uv",
            FileType::Bam => "bam",
            FileType::Facs => "fcs",
            FileType::Fasta => "fasta",
            FileType::Fastq => "fastq",
            FileType::InficonHapsite => "inficon",
            FileType::Png => "png",
            FileType::Sam => "sam",
            FileType::ThermoCf => "thermo_cf",
            FileType::ThermoDxf => "thermo_dxf",
            FileType::DelimitedText(b',') => "csv",
            FileType::DelimitedText(b'\t') => "tsv",
            _ => return format!("unsupported/{:?}", self),
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_names() {
        let filetypes = [
            FileType::AgilentChemstationFid,
            FileType::AgilentChemstationMs,
            FileType::AgilentChemstationMwd,
            FileType::AgilentChemstationUv,
            FileType::Bam,
            FileType::Facs,
            FileType::Fasta,
            FileType::Fastq,
            FileType::InficonHapsite,
            FileType::Png,
            FileType::Sam,
            FileType::ThermoCf,
            FileType::ThermoDxf,
            FileType::DelimitedText(b','),
            FileType::DelimitedText(b'\t'),
        ];
        for ft in filetypes {
            assert_eq!(FileType::from_parser_name(&ft.to_parser_name()), ft);
        }
    }
}
