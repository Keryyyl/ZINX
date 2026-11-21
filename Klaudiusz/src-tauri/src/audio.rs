//! Nagrywanie z mikrofonu z wykrywaniem mowy: zaczyna się, gdy wykryje głos,
//! kończy po krótkiej ciszy (albo po maksymalnym czasie). Zwraca próbki 16 kHz mono.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// Ustawiane przez `cancel_recording`, przerywa trwające nagrywanie
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Przerywa trwające nagrywanie (np. po wyłączeniu czatu głosowego). Nagranie zwraca wtedy `None`.
pub fn cancel_recording() {
    CANCEL.store(true, Ordering::SeqCst);
}

pub const TARGET_RATE: u32 = 16_000;
const FRAME_MS: usize = 30;
/// Cisza po mowie, która kończy nagranie
const SILENCE_END_MS: usize = 900;
/// Ile czekamy na początek mowy, zanim uznamy że nikt nic nie powiedział
const WAIT_START_MS: usize = 6_000;
/// Najdłuższe pojedyncze polecenie
const MAX_MS: usize = 12_000;
/// Próg głośności (RMS) odróżniający mowę od szumu w pokoju
const SPEECH_RMS: f32 = 0.008;

/// Nagrywa jedną wypowiedź. `Ok(None)` gdy nikt nic nie powiedział.
pub fn record_utterance() -> Result<Option<Vec<f32>>, String> {
    CANCEL.store(false, Ordering::SeqCst);
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("Nie znaleziono mikrofonu")?;
    let config = device.default_input_config().map_err(|e| format!("Mikrofon: {e}"))?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config.into(), channels, tx)?,
        SampleFormat::I16 => build::<i16>(&device, &config.into(), channels, tx)?,
        SampleFormat::U16 => build::<u16>(&device, &config.into(), channels, tx)?,
        other => return Err(format!("Nieobsługiwany format mikrofonu: {other:?}")),
    };
    stream.play().map_err(|e| format!("Mikrofon: {e}"))?;

    let frame = rate as usize * FRAME_MS / 1000;
    let mut all: Vec<f32> = Vec::new();
    let mut pos = 0usize;
    let mut speech = false;
    let mut silent_ms = 0usize;
    loop {
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(chunk) => all.extend(chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while pos + frame <= all.len() {
            let rms = (all[pos..pos + frame].iter().map(|s| s * s).sum::<f32>() / frame as f32).sqrt();
            pos += frame;
            if rms >= SPEECH_RMS {
                speech = true;
                silent_ms = 0;
            } else if speech {
                silent_ms += FRAME_MS;
            }
        }
        let elapsed_ms = pos * 1000 / rate as usize;
        if CANCEL.load(Ordering::SeqCst) {
            break;
        }
        if (speech && silent_ms >= SILENCE_END_MS) || elapsed_ms >= MAX_MS || (!speech && elapsed_ms >= WAIT_START_MS) {
            break;
        }
    }
    drop(stream);

    if !speech || CANCEL.load(Ordering::SeqCst) {
        return Ok(None);
    }
    Ok(Some(resample(&all, rate)))
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, channels: usize, tx: mpsc::Sender<Vec<f32>>) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // mono: uśredniamy kanały
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32)
                    .collect();
                let _ = tx.send(mono);
            },
            |err| eprintln!("Mikrofon: {err}"),
            None,
        )
        .map_err(|e| format!("Mikrofon: {e}"))
}

/// Prosta zmiana częstotliwości próbkowania (wystarcza dla mowy)
pub fn resample(input: &[f32], from: u32) -> Vec<f32> {
    if from == TARGET_RATE || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / TARGET_RATE as f64;
    let out_len = (input.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let i0 = pos as usize;
            let frac = (pos - i0 as f64) as f32;
            let a = input[i0.min(input.len() - 1)];
            let b = *input.get(i0 + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// Zapis WAV 16-bit PCM mono, który rozumie whisper.cpp
pub fn write_wav(path: &std::path::Path, samples: &[f32]) -> Result<(), String> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&TARGET_RATE.to_le_bytes());
    out.extend_from_slice(&(TARGET_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, out).map_err(|e| format!("Zapis nagrania: {e}"))
}

#[cfg(test)]
mod tests {
    /// Sprawdza, czy mikrofon się otwiera i czy cisza nie zwraca nagrania (ok. 6 s).
    /// Uruchom ręcznie: `KLAUDIUSZ_MIC_TEST=1 cargo test mic_opens -- --nocapture`. Potrzebny dostęp do mikrofonu.
    #[test]
    fn mic_opens_and_silence_gives_none() {
        if std::env::var("KLAUDIUSZ_MIC_TEST").is_err() {
            return;
        }
        let result = super::record_utterance().expect("mikrofon powinien się otworzyć");
        println!("nagranie: {:?}", result.as_ref().map(|s| s.len()));
        assert!(result.is_none());
    }

    #[test]
    fn resample_halves_length_from_32k() {
        let out = super::resample(&vec![0.5; 32_000], 32_000);
        assert_eq!(out.len(), 16_000);
    }
}
