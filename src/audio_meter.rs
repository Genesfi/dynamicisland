#![allow(unsafe_op_in_unsafe_fn)]

use windows::{
    core::*,
    Win32::{
        Media::Audio::{
            Endpoints::IAudioMeterInformation,
            eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
        },
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
    },
};

pub struct AudioMeter {
    meter: Option<IAudioMeterInformation>,
    smoothed_peak: f32,
}

impl AudioMeter {
    pub fn new() -> Self {
        let meter = unsafe {
            let enumerator: Result<IMMDeviceEnumerator> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER);
            if let Ok(enum_dev) = enumerator {
                if let Ok(dev) = enum_dev.GetDefaultAudioEndpoint(eRender, eMultimedia) {
                    dev.Activate(CLSCTX_INPROC_SERVER, None).ok()
                } else {
                    None
                }
            } else {
                None
            }
        };

        Self {
            meter,
            smoothed_peak: 0.0,
        }
    }

    /// Reads real-time peak audio volume (0.0 to 1.0) and applies smooth decay physics.
    pub fn update(&mut self, dt: f32) -> f32 {
        let raw_peak = if let Some(meter) = &self.meter {
            if let Ok(peak) = unsafe { meter.GetPeakValue() } {
                peak.clamp(0.0, 1.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Smooth attack, exponential decay for pleasant organic visualizer motion
        if raw_peak > self.smoothed_peak {
            self.smoothed_peak = self.smoothed_peak + (raw_peak - self.smoothed_peak) * (dt * 20.0).min(1.0);
        } else {
            self.smoothed_peak = (self.smoothed_peak - dt * 1.5).max(0.0);
        }

        self.smoothed_peak
    }
}
