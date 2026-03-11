use ariel_os::debug::log::debug;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
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

pub struct SsdTarget<
    RST: OutputPin,
    DC: OutputPin,
    BUSY: InputPin + Wait,
    DELAY: DelayNs,
    SPI: SpiDevice,
> {
    frame_buffer: [u8; FRAME_BUFFER_SIZE],
    frame_buffer_changed: bool,
    refresh_count: u8,
    driver: Mutex<CriticalSectionRawMutex, SSD1680<RST, DC, BUSY, DELAY, SPI>>,
}
impl<RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    SsdTarget<RST, DC, BUSY, DELAY, SPI>
{
    const REFRESH_AFTER: u8 = 10;
    pub fn new(driver: SSD1680<RST, DC, BUSY, DELAY, SPI>) -> Self {
        Self {
            refresh_count: Self::REFRESH_AFTER, // force a full refresh on first flush
            driver: Mutex::new(driver),
            frame_buffer: [0u8; FRAME_BUFFER_SIZE],
            frame_buffer_changed: true,
        }
    }

    pub async fn flush(&mut self) {
        if !self.frame_buffer_changed {
            return;
        }

        debug!("flushing to display");
        let mut driver: embassy_sync::mutex::MutexGuard<
            '_,
            CriticalSectionRawMutex,
            SSD1680<RST, DC, BUSY, DELAY, SPI>,
        > = self.driver.lock().await;

        // driver.hw_init().await.unwrap();
        // driver.wait_for_busy().await.unwrap();

        driver.write_bw_bytes(&self.frame_buffer).await.unwrap();
        driver.wait_for_busy().await.unwrap();

        if self.refresh_count >= Self::REFRESH_AFTER {
            debug!("Doing a full refresh");
            // Somehow the full refresh reads from the RED memory.
            driver.write_red_bytes(&self.frame_buffer).await.unwrap();
            driver.wait_for_busy().await.unwrap();
            driver.full_refresh().await.unwrap();

            self.refresh_count = 0;
        } else {
            debug!("Doing a partial refresh");

            driver.partial_refresh().await.unwrap();

            self.refresh_count += 1;
        }

        self.frame_buffer_changed = false;
        debug!("Refresh count: {}", self.refresh_count);

        // driver.enter_deep_sleep().await.unwrap();
    }
}

impl<RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    DrawTarget for SsdTarget<RST, DC, BUSY, DELAY, SPI>
{
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
impl<RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    Dimensions for SsdTarget<RST, DC, BUSY, DELAY, SPI>
{
    fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
        embedded_graphics::primitives::Rectangle::new(
            embedded_graphics::prelude::Point::new(0, 0),
            embedded_graphics::prelude::Size::new(WIDTH as u32, HEIGHT as u32),
        )
    }
}
