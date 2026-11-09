// SPDX-License-Identifier: GPL-3.0-or-later
//! Battery / charger state.

use isle_logic::peek::PowerState;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

pub fn read() -> PowerState {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return PowerState { on_ac: false, percent: None };
    }
    PowerState { on_ac: s.ACLineStatus == 1, percent: (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent) }
}
