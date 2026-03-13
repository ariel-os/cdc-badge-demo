use core::cell::RefCell;

use ariel_os::debug::log::debug;
use embassy_sync::{
    blocking_mutex::{CriticalSectionMutex, raw::CriticalSectionRawMutex},
    mutex::Mutex,
    signal::Signal,
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
    RST: OutputPin,
    DC: OutputPin,
    BUSY: InputPin + Wait,
    DELAY: DelayNs,
    SPI: SpiDevice,
> {
    frame_buffer: CriticalSectionMutex<RefCell<[u8; FRAME_BUFFER_SIZE]>>,
    refresh_count: Mutex<CriticalSectionRawMutex, RefCell<u8>>,
    driver: Mutex<CriticalSectionRawMutex, SSD1680<RST, DC, BUSY, DELAY, SPI>>,
    signal: Signal<CriticalSectionRawMutex, u8>,
}
impl<RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    SsdTargetManager<RST, DC, BUSY, DELAY, SPI>
{
    const REFRESH_AFTER: u8 = 10;
    pub fn new(driver: SSD1680<RST, DC, BUSY, DELAY, SPI>) -> Self {
        Self {
            frame_buffer: CriticalSectionMutex::new(RefCell::new([0u8; FRAME_BUFFER_SIZE])),
            refresh_count: Mutex::new(RefCell::new(Self::REFRESH_AFTER)), // force a full refresh on first flush
            driver: Mutex::new(driver),
            signal: Signal::new(),
        }
    }

    pub fn flush(&self) {
        self.signal.signal(1);
    }

    pub async fn run(&self) {
        loop {
            self.signal.wait().await;

            self.inner_flush().await;
        }
    }

    async fn inner_flush(&self) {
        // one copy here
        let frame_buffer = self.frame_buffer.lock(|fb| *fb.borrow());
        debug!("flushing to display");
        let mut driver: embassy_sync::mutex::MutexGuard<
            '_,
            CriticalSectionRawMutex,
            SSD1680<RST, DC, BUSY, DELAY, SPI>,
        > = self.driver.lock().await;

        // driver.hw_init().await.unwrap();
        // driver.wait_for_busy().await.unwrap();

        driver.write_bw_bytes(&frame_buffer).await.unwrap();
        driver.wait_for_busy().await.unwrap();

        if self
            .refresh_count
            .lock()
            .await
            .borrow()
            .ge(&Self::REFRESH_AFTER)
        {
            debug!("Doing a full refresh");
            // Somehow the full refresh reads from the RED memory.
            driver.write_red_bytes(&frame_buffer).await.unwrap();
            driver.wait_for_busy().await.unwrap();
            driver.full_refresh().await.unwrap();

            self.refresh_count.lock().await.replace(0);
        } else {
            debug!("Doing a partial refresh");

            driver.partial_refresh().await.unwrap();

            self.refresh_count.lock().await.replace_with(|c| *c + 1);
        }

        debug!(
            "Refresh count: {}",
            *self.refresh_count.lock().await.borrow()
        );

        // driver.enter_deep_sleep().await.unwrap();
    }
}

pub struct SsdTarget<
    'a,
    RST: OutputPin,
    DC: OutputPin,
    BUSY: InputPin + Wait,
    DELAY: DelayNs,
    SPI: SpiDevice,
> {
    frame_buffer_changed: bool,
    manager: &'a SsdTargetManager<RST, DC, BUSY, DELAY, SPI>,
}
impl<'a, RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    SsdTarget<'a, RST, DC, BUSY, DELAY, SPI>
{
    pub fn new(manager: &'a SsdTargetManager<RST, DC, BUSY, DELAY, SPI>) -> Self {
        Self {
            manager,
            frame_buffer_changed: true,
        }
    }

    pub fn flush(&mut self) {
        if self.frame_buffer_changed {
            self.manager.flush();
            self.frame_buffer_changed = false;
        }
    }
}

impl<'a, RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    DrawTarget for SsdTarget<'a, RST, DC, BUSY, DELAY, SPI>
{
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.frame_buffer_changed = true;

        debug!("drawing pixels (Embedded Graphics)");

        self.manager.frame_buffer.lock(|fb| {
            let mut frame_buffer = fb.borrow_mut();

            for Pixel(coord, color) in pixels {
                // Flipping and rotating the screen.
                // TODO: should be handled in the SSD1680 driver.
                let x = (HEIGHT - 1) - coord.y as usize;
                let y = coord.x as usize;

                let index = y * HEIGHT / 8 + x / 8;

                if color == BinaryColor::On {
                    frame_buffer[index] |= 0x80 >> (x % 8);
                } else {
                    frame_buffer[index] &= !(0x80 >> (x % 8));
                }
            }
        });

        Ok(())
    }
}
impl<'a, RST: OutputPin, DC: OutputPin, BUSY: InputPin + Wait, DELAY: DelayNs, SPI: SpiDevice>
    Dimensions for SsdTarget<'a, RST, DC, BUSY, DELAY, SPI>
{
    fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
        embedded_graphics::primitives::Rectangle::new(
            embedded_graphics::prelude::Point::new(0, 0),
            embedded_graphics::prelude::Size::new(WIDTH as u32, HEIGHT as u32),
        )
    }
}
