#![allow(dead_code)]

use std::collections::HashMap;
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps,
    HidP_Input, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::Input::{
    GetRawInputDeviceInfoW, RIDI_PREPARSEDDATA,
};

pub const UP_GENERIC: u16 = 0x01;
pub const U_X: u16 = 0x30;
pub const U_Y: u16 = 0x31;
pub const UP_DIGITIZER: u16 = 0x0D;
pub const U_TOUCHPAD: u16 = 0x05;
pub const U_TIPSWITCH: u16 = 0x42;
pub const U_CONTACTID: u16 = 0x51;
pub const U_CONTACTCNT: u16 = 0x54;

pub struct DeviceLayout {
    pub preparsed_data: Vec<u8>,
    pub input_len: u16,
    pub finger_links: Vec<u16>,
    pub contactcount_link: Option<u16>,
}

impl DeviceLayout {
    pub fn from_device_handle(device: HANDLE) -> Option<Self> {
        unsafe {
            let mut size: u32 = 0;
            GetRawInputDeviceInfoW(device, RIDI_PREPARSEDDATA, None, &mut size);
            if size == 0 {
                return None;
            }

            let mut buf = vec![0u8; size as usize];
            let res = GetRawInputDeviceInfoW(
                device,
                RIDI_PREPARSEDDATA,
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
            );
            if res == u32::MAX {
                return None;
            }

            let preparsed = PHIDP_PREPARSED_DATA(buf.as_ptr() as isize);

            let mut caps = HIDP_CAPS::default();
            if HidP_GetCaps(preparsed, &mut caps) != HIDP_STATUS_SUCCESS {
                return None;
            }

            let n = caps.NumberInputValueCaps as usize;
            let mut value_caps = vec![HIDP_VALUE_CAPS::default(); n];
            let mut count = caps.NumberInputValueCaps;

            if HidP_GetValueCaps(HidP_Input, value_caps.as_mut_ptr(), &mut count, preparsed)
                != HIDP_STATUS_SUCCESS
            {
                return None;
            }

            let mut finger_links = Vec::new();
            let mut contactcount_link = None;

            for vc in value_caps.iter().take(count as usize) {
                let usage = if vc.IsRange.as_bool() {
                    vc.Anonymous.Range.UsageMin
                } else {
                    vc.Anonymous.NotRange.Usage
                };

                if vc.UsagePage == UP_GENERIC && usage == U_X {
                    if !finger_links.contains(&vc.LinkCollection) {
                        finger_links.push(vc.LinkCollection);
                    }
                } else if vc.UsagePage == UP_DIGITIZER && usage == U_CONTACTCNT {
                    contactcount_link = Some(vc.LinkCollection);
                }
            }

            if finger_links.is_empty() {
                return None;
            }

            Some(Self {
                preparsed_data: buf,
                input_len: caps.InputReportByteLength,
                finger_links,
                contactcount_link,
            })
        }
    }

    pub fn parse_report(&self, report: &[u8]) -> HashMap<u32, (f64, f64, bool)> {
        let mut contacts = HashMap::new();
        let preparsed = PHIDP_PREPARSED_DATA(self.preparsed_data.as_ptr() as isize);
        let mut report_buf = report.to_vec();

        unsafe {
            for &link in &self.finger_links {
                let mut usages = [0u16; 16];
                let mut ulen: u32 = 16;

                let _ = HidP_GetUsages(
                    HidP_Input,
                    UP_DIGITIZER,
                    link,
                    usages.as_mut_ptr(),
                    &mut ulen,
                    preparsed,
                    &mut report_buf,
                );

                let down = usages[..ulen as usize].iter().any(|&u| u == U_TIPSWITCH);

                let mut x: u32 = 0;
                let mut y: u32 = 0;
                let mut cid: u32 = 0;

                let _ = HidP_GetUsageValue(
                    HidP_Input,
                    UP_GENERIC,
                    link,
                    U_X,
                    &mut x,
                    preparsed,
                    &report_buf,
                );

                let _ = HidP_GetUsageValue(
                    HidP_Input,
                    UP_GENERIC,
                    link,
                    U_Y,
                    &mut y,
                    preparsed,
                    &report_buf,
                );

                let _ = HidP_GetUsageValue(
                    HidP_Input,
                    UP_DIGITIZER,
                    link,
                    U_CONTACTID,
                    &mut cid,
                    preparsed,
                    &report_buf,
                );

                contacts.insert(cid, (x as f64, y as f64, down));
            }
        }

        contacts
    }
}
