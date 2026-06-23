use std::io::Read;

use hound::{SampleFormat, WavReader, WavSpec};

/// Read a WAV stream and return its format spec together with the raw,
/// little-endian PCM bytes of the `data` chunk.
///
/// The returned bytes are reconstructed sample-by-sample so that they are
/// byte-for-byte identical to the original `data` chunk: signed little-endian
/// for 16/24-bit integer samples, unsigned for 8-bit (per the WAV convention),
/// and IEEE little-endian for 32-bit float samples.
pub fn read<R>(reader: R) -> hound::Result<(WavSpec, Vec<u8>)>
where
    R: Read,
{
    let mut wav = WavReader::new(reader)?;
    let spec = wav.spec();
    let bytes_per_sample = (spec.bits_per_sample as usize).div_ceil(8);
    let mut data = Vec::with_capacity(wav.len() as usize * bytes_per_sample);

    match spec.sample_format {
        SampleFormat::Int => {
            for sample in wav.samples::<i32>() {
                let sample = sample?;
                if spec.bits_per_sample == 8 {
                    // WAV stores 8-bit PCM as unsigned; hound returns it
                    // centered around zero, so shift it back to [0, 255].
                    data.push((sample + 128) as u8);
                } else {
                    data.extend_from_slice(&sample.to_le_bytes()[..bytes_per_sample]);
                }
            }
        }
        SampleFormat::Float => {
            for sample in wav.samples::<f32>() {
                data.extend_from_slice(&sample?.to_le_bytes());
            }
        }
    }

    Ok((spec, data))
}

/// Map a WAV spec to the integer format code expected by the server.
pub fn convert_format(spec: &WavSpec) -> i32 {
    match spec.sample_format {
        SampleFormat::Int => match spec.bits_per_sample {
            8 => 2,
            16 => 4,
            24 => 8,
            _ => 0,
        },
        SampleFormat::Float => match spec.bits_per_sample {
            32 => 16,
            _ => 0,
        },
    }
}
