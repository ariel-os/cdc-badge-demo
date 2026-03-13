use ariel_os::debug::log::debug;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    watch::{Receiver, Sender},
};
use embedded_graphics::{
    Pixel,
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget},
};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};
use ssd1680_rs::driver_async::SSD1680;

pub const WIDTH: usize = 296;
pub const HEIGHT: usize = 128;

pub const FRAME_BUFFER_SIZE: usize = (WIDTH * HEIGHT) / 8;

pub struct SsdTargetManager<
    'a,
    RST: OutputPin,
    DC: OutputPin,
    BUSY: InputPin + Wait,
    DELAY: DelayNs,
    SPI: SpiDevice,
> {
    refresh_count: u8,
    driver: SSD1680<RST, DC, BUSY, DELAY, SPI>,
    receiver: Receiver<'a, CriticalSectionRawMutex, [u8; FRAME_BUFFER_SIZE], 1>,
}
impl<'a, RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    SsdTargetManager<'a, RST, DC, BUSY, DELAY, SPI>
{
    const REFRESH_AFTER: u8 = 10;
    pub fn new(
        driver: SSD1680<RST, DC, BUSY, DELAY, SPI>,
        receiver: Receiver<'a, CriticalSectionRawMutex, [u8; FRAME_BUFFER_SIZE], 1>,
    ) -> Self {
        Self {
            refresh_count: Self::REFRESH_AFTER, // force a full refresh on first flush
            driver,
            receiver,
        }
    }

    pub async fn run(&mut self) {
        let mut frame_buffer;
        loop {
            frame_buffer = self.receiver.changed().await;
            debug!("flushing to display");

            // driver.hw_init().await.unwrap();
            // driver.wait_for_busy().await.unwrap();

            self.driver.write_bw_bytes(&frame_buffer).await.unwrap();
            self.driver.wait_for_busy().await.unwrap();

            if self.refresh_count >= Self::REFRESH_AFTER {
                debug!("Doing a full refresh");
                // Somehow the full refresh reads from the RED memory.
                self.driver.write_red_bytes(&frame_buffer).await.unwrap();
                self.driver.wait_for_busy().await.unwrap();
                self.driver.full_refresh().await.unwrap();

                self.refresh_count = 0;
            } else {
                debug!("Doing a partial refresh");

                self.driver.partial_refresh().await.unwrap();

                self.refresh_count += 1;
            }

            debug!("Refresh count: {}", self.refresh_count);

            // driver.enter_deep_sleep().await.unwrap();
        }
    }
}

pub struct SsdTarget<'a> {
    frame_buffer_changed: bool,
    sender: Sender<'a, CriticalSectionRawMutex, [u8; FRAME_BUFFER_SIZE], 1>,
    frame_buffer: [u8; FRAME_BUFFER_SIZE],
}
impl<'a> SsdTarget<'a> {
    pub fn new(sender: Sender<'a, CriticalSectionRawMutex, [u8; FRAME_BUFFER_SIZE], 1>) -> Self {
        Self {
            sender,
            frame_buffer_changed: true,
            frame_buffer: [0u8; FRAME_BUFFER_SIZE],
        }
    }

    pub fn flush(&mut self) {
        if self.frame_buffer_changed {
            self.sender.send(self.frame_buffer);
            self.frame_buffer_changed = false;
        }
    }
}

impl<'a> DrawTarget for SsdTarget<'a> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.frame_buffer_changed = true;

        debug!("drawing pixels (Embedded Graphics)");

        for Pixel(coord, color) in pixels {
            // Flipping and rotating the screen.
            // TODO: should be handled in the SSD1680 driver.
            let x = (HEIGHT - 1) - coord.y as usize;
            let y = coord.x as usize;

            let index = y * HEIGHT / 8 + x / 8;

            if color == BinaryColor::On {
                self.frame_buffer[index] |= 0x80 >> (x % 8);
            } else {
                self.frame_buffer[index] &= !(0x80 >> (x % 8));
            }
        }

        Ok(())
    }
}
impl<'a> Dimensions for SsdTarget<'a> {
    fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
        embedded_graphics::primitives::Rectangle::new(
            embedded_graphics::prelude::Point::new(0, 0),
            embedded_graphics::prelude::Size::new(WIDTH as u32, HEIGHT as u32),
        )
    }
}
