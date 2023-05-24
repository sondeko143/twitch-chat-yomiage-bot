use std::io::{self, Read};
use wav::Header;

pub fn read<R>(reader: &mut R) -> io::Result<(Header, Vec<u8>)>
where
    R: Read + io::Seek,
{
    let header = read_header(reader)?;
    Ok((header, read_data(reader)?))
}

fn read_header<R>(reader: &mut R) -> io::Result<Header>
where
    R: Read + io::Seek,
{
    let wav = verify_wav_file(reader)?;

    let c = wav.iter(reader).find(|c| c.id().as_str() == "fmt ");
    match c {
        Some(c) => {
            let header_bytes = c.read_contents(reader)?;
            let header = Header::try_from(header_bytes.as_slice())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            // Return error if not using PCM
            match header.audio_format {
                wav::WAV_FORMAT_PCM | wav::WAV_FORMAT_IEEE_FLOAT => Ok(header),
                _ => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Unsupported data format, data is not in uncompressed PCM format, aborting",
                )),
            }
        }
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "RIFF data is missing the \"fmt \" chunk, aborting",
        )),
    }
}

pub fn convert_format(header: &Header) -> i32 {
    match header.audio_format {
        wav::WAV_FORMAT_PCM => match header.bits_per_sample {
            8 => 2,
            16 => 4,
            24 => 8,
            _ => 0,
        },
        wav::WAV_FORMAT_IEEE_FLOAT => match header.bits_per_sample {
            32 => 16,
            _ => 0,
        },
        _ => 0,
    }
}

fn read_data<R>(reader: &mut R) -> io::Result<Vec<u8>>
where
    R: Read + io::Seek,
{
    let wav = verify_wav_file(reader)?;
    let c = wav.iter(reader).find(|c| c.id().as_str() == "data");
    match c {
        Some(c) => c.read_contents(reader),
        None => Err(io::Error::new(
            io::ErrorKind::Other,
            "Could not parse audio data",
        )),
    }
}

fn verify_wav_file<R>(reader: &mut R) -> io::Result<riff::Chunk>
where
    R: Read + io::Seek,
{
    let wav = riff::Chunk::read(reader, 0)?;

    let form_type = wav.read_type(reader)?;

    if form_type.as_str() == "WAVE" {
        Ok(wav)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "RIFF file type not \"WAVE\"",
        ))
    }
}
