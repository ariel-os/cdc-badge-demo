use usbd_hid::descriptor::{AsInputReport, SerializedDescriptor, gen_hid_descriptor};

/// HID Keypad descriptor.
#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = KEYBOARD) = {
        (usage_page = LEDS, usage_min = 0x01, usage_max = 0x05) = {
            #[packed_bits = 5] #[item_settings(data,variable,absolute)] leds=output;
        };
        (usage_page = KEYBOARD, usage_min = 0x58, usage_max = 0x62) = {
            #[item_settings(data,array,absolute)] keycodes=input;
        };
    }
)]
#[allow(dead_code)]
#[derive(Default)]
#[cfg_attr(context = "defmt", derive(defmt::Format))]
pub struct KeypadReport {
    pub leds: u8, // LedIndicator
    pub keycodes: [u8; 6],
}
