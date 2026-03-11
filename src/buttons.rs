use core::fmt::Display;

use ariel_os::time::{Duration, Instant};
use async_tca9535::registers::Input;
use heapless::Vec;

#[derive(Debug, Clone, Copy)]
pub struct ButtonStatus {
    pub pressed: bool,
    pub since: Instant,
}

impl Default for ButtonStatus {
    fn default() -> Self {
        Self {
            pressed: false,
            since: Instant::from_ticks(0),
        }
    }
}
impl ButtonStatus {
    /// Update the status of this button, returns a status change if the status changed.
    pub fn update(&mut self, pressed: bool, instant: Instant) -> Option<ButtonSatuChange> {
        if pressed != self.pressed {
            let status_change = ButtonSatuChange {
                was_presed: self.pressed,
                duration: instant.duration_since(self.since),
            };

            self.pressed = pressed;
            self.since = instant;
            Some(status_change)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ButtonSatuChange {
    pub was_presed: bool,
    pub duration: Duration,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Button {
    Btn0,
    Btn1,
    Btn2,
    Btn3,
    Btn4,
    Btn5,
    Btn6,
    Btn7,
    Btn8,
    Btn9,
    BtnYes,
    BtnNo,
}
impl Button {
    pub const COUNT: usize = 12;

    pub fn name(&self) -> &str {
        match self {
            Button::Btn0 => "0",
            Button::Btn1 => "1",
            Button::Btn2 => "2",
            Button::Btn3 => "3",
            Button::Btn4 => "4",
            Button::Btn5 => "5",
            Button::Btn6 => "6",
            Button::Btn7 => "7",
            Button::Btn8 => "8",
            Button::Btn9 => "9",
            Button::BtnYes => "YES",
            Button::BtnNo => "NO",
        }
    }
}

impl Display for Button {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonsStatus {
    pub btn_0: ButtonStatus,
    pub btn_1: ButtonStatus,
    pub btn_2: ButtonStatus,
    pub btn_3: ButtonStatus,
    pub btn_4: ButtonStatus,
    pub btn_5: ButtonStatus,
    pub btn_6: ButtonStatus,
    pub btn_7: ButtonStatus,
    pub btn_8: ButtonStatus,
    pub btn_9: ButtonStatus,
    pub btn_yes: ButtonStatus,
    pub btn_no: ButtonStatus,
}

impl ButtonsStatus {
    pub fn new() -> Self {
        ButtonsStatus::default()
    }

    /// Update according to the input, should never panic as we allocate the number of buttons.
    pub fn update(
        &mut self,
        input: Input,
        instant: Instant,
    ) -> Vec<(Button, ButtonSatuChange), { Button::COUNT }> {
        let mut changed = Vec::new();

        // FIXME: use a macro

        if let Some(change) = self.btn_0.update(input.P00(), instant) {
            changed.push((Button::Btn0, change)).unwrap();
        }
        if let Some(change) = self.btn_1.update(input.P01(), instant) {
            changed.push((Button::Btn1, change)).unwrap();
        }
        if let Some(change) = self.btn_2.update(input.P02(), instant) {
            changed.push((Button::Btn2, change)).unwrap();
        }
        if let Some(change) = self.btn_3.update(input.P03(), instant) {
            changed.push((Button::Btn3, change)).unwrap();
        }
        if let Some(change) = self.btn_4.update(input.P04(), instant) {
            changed.push((Button::Btn4, change)).unwrap();
        }
        if let Some(change) = self.btn_5.update(input.P05(), instant) {
            changed.push((Button::Btn5, change)).unwrap();
        }
        if let Some(change) = self.btn_6.update(input.P06(), instant) {
            changed.push((Button::Btn6, change)).unwrap();
        }
        if let Some(change) = self.btn_7.update(input.P07(), instant) {
            changed.push((Button::Btn7, change)).unwrap();
        }
        if let Some(change) = self.btn_8.update(input.P10(), instant) {
            changed.push((Button::Btn8, change)).unwrap();
        }
        if let Some(change) = self.btn_9.update(input.P11(), instant) {
            changed.push((Button::Btn9, change)).unwrap();
        }
        if let Some(change) = self.btn_yes.update(input.P12(), instant) {
            changed.push((Button::BtnYes, change)).unwrap();
        }
        if let Some(change) = self.btn_no.update(input.P13(), instant) {
            changed.push((Button::BtnNo, change)).unwrap();
        }

        changed
    }

    pub fn button_status(&self, button: Button) -> ButtonStatus {
        match button {
            Button::Btn0 => self.btn_0,
            Button::Btn1 => self.btn_1,
            Button::Btn2 => self.btn_2,
            Button::Btn3 => self.btn_3,
            Button::Btn4 => self.btn_4,
            Button::Btn5 => self.btn_5,
            Button::Btn6 => self.btn_6,
            Button::Btn7 => self.btn_7,
            Button::Btn8 => self.btn_8,
            Button::Btn9 => self.btn_9,
            Button::BtnYes => self.btn_yes,
            Button::BtnNo => self.btn_no,
        }
    }
}
