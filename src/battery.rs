use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

#[derive(Debug, Clone, Copy)]
pub struct BatteryStatus {
    pub percent: u8,
    pub is_charging: bool,
    pub has_battery: bool,
}

impl BatteryStatus {
    pub fn query() -> Self {
        unsafe {
            let mut status = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut status).is_ok() {
                // BatteryFlag: 128 = No system battery, 255 = Unknown status
                let has_battery = status.BatteryFlag != 128 && status.BatteryLifePercent <= 100;
                let percent = if status.BatteryLifePercent <= 100 {
                    status.BatteryLifePercent
                } else {
                    100
                };
                // ACLineStatus: 1 = Online (charging / AC plugged)
                // BatteryFlag: bit 3 (8) = Charging
                let is_charging = status.ACLineStatus == 1 || (status.BatteryFlag & 8) != 0;
                BatteryStatus {
                    percent,
                    is_charging,
                    has_battery,
                }
            } else {
                BatteryStatus {
                    percent: 100,
                    is_charging: true,
                    has_battery: false,
                }
            }
        }
    }
}
