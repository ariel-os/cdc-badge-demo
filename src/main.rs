#![no_main]
#![no_std]

mod buttons;
mod pins;

use ariel_os::{
    debug::log::{Debug2Format, debug, info},
    gpio, hal, i2c,
    spi::{self, main::SpiDevice},
    time::{Delay, Instant, Timer},
};
use async_tca9535::{
    Tca9535,
    registers::{Configuration, Polarity},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, pubsub::PubSubChannel,
};
use embedded_graphics::{
    Drawable,
    image::Image,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
};
use embedded_hal_async::i2c::I2c as _;
use tinybmp::Bmp;

use crate::buttons::ButtonsStatus;

const TARGET_I2C_ADDR: u8 = 0x6A;
const WHO_AM_I_REG_ADDR: u8 = 0x14;

// Channel with one publisher and maximum 4 subscribers.
static BUTTONS_CHANNEL: PubSubChannel<
    CriticalSectionRawMutex,
    (buttons::Button, buttons::ButtonSatuChange),
    { buttons::Button::COUNT },
    4,
    1,
> = PubSubChannel::new();

#[ariel_os::task(autostart, peripherals)]
async fn main(peripherals: pins::I2cBus) {
    info!("Hello World!");

    // set up i2c bus
    let mut i2c_config = hal::i2c::controller::Config::default();
    i2c_config.frequency = const {
        i2c::controller::highest_freq_in(
            i2c::controller::Kilohertz::kHz(100)..=i2c::controller::Kilohertz::kHz(400),
        )
    };

    let i2c_bus = pins::SensorI2c::new(peripherals.i2c_sda, peripherals.i2c_scl, i2c_config);
    let mut i2c_device = i2c_bus;

    //    let mut i2c_device = I2cDevice::new(&i2c_bus);
    let mut id = [0];
    i2c_device
        .write_read(TARGET_I2C_ADDR, &[WHO_AM_I_REG_ADDR], &mut id)
        .await
        .unwrap();

    let who_am_i = id[0];
    info!("WHO_AM_I_COMMAND register value: 0x{:x}", who_am_i);
    assert_eq!(who_am_i & 0b111111, 0x39);

    // set reg 4 to 0x08 (512mAh fast charging rate)
    i2c_device
        .write(TARGET_I2C_ADDR, &[0x04, 0x08])
        .await
        .unwrap();

    let mut tmp = [0x0];
    i2c_device
        .write_read(TARGET_I2C_ADDR, &[0x03], &mut tmp)
        .await
        .unwrap();

    // set reg 3[1-3] to 0b101 (3.3v min voltage)
    let reg03_target = (tmp[0] & 0b11110001) | (0b101 << 1);
    i2c_device
        .write(TARGET_I2C_ADDR, &[0x03, reg03_target])
        .await
        .unwrap();

    let mut tmp = [0x0];
    i2c_device
        .write_read(TARGET_I2C_ADDR, &[0x03], &mut tmp)
        .await
        .unwrap();
    info!("0x03 register value: 0b{:b}", tmp[0]);

    // We'll probably want to use embedded_hal_bus later on
    let mut io_expander = Tca9535::new(i2c_device, async_tca9535::DeviceAddress::LLL);
    io_expander
        .set_configuration(Configuration::new(
            true, true, true, true, true, true, true, true, true, true, true, true, false, false,
            false, false,
        ))
        .await
        .unwrap();

    // Invert the polarity so true = pressed.
    io_expander
        .set_polarity_inversion(Polarity::new(
            true, true, true, true, true, true, true, true, true, true, true, true, false, false,
            false, false,
        ))
        .await
        .unwrap();

    let mut buttons_status = ButtonsStatus::new();

    let publisher = BUTTONS_CHANNEL.publisher().unwrap();

    loop {
        Timer::after_millis(50).await;
        let input = io_expander.read_input().await.unwrap();

        let instant = Instant::now();

        let changes = buttons_status.update(input, instant);

        for update in changes.iter() {
            if update.1.was_presed {
                debug!(
                    "{:?} was held for {}ms",
                    Debug2Format(&update.0),
                    update.1.duration.as_millis()
                );
            } else {
                debug!("{:?} pressed", Debug2Format(&update.0));
            }

            // This will delete older events if the channel is full.
            // That allows us to not lag behind too much at the cost of losing some inputs.
            publisher.publish_immediate(*update);
        }
    }
}

const FRAME_BUFFER_SIZE: usize = 128 * 296;

#[ariel_os::task(autostart, peripherals)]
async fn screen(peripherals: pins::Epd) {
    static SPI_BUS: once_cell::sync::OnceCell<
        Mutex<embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, hal::spi::main::Spi>,
    > = once_cell::sync::OnceCell::new();

    info!("Starting EPD demo");
    let mut spi_config = hal::spi::main::Config::default();
    spi_config.frequency = const {
        spi::main::highest_freq_in(spi::main::Kilohertz::MHz(1)..=spi::main::Kilohertz::MHz(20))
    };
    // spi_config.mode = spi::Mode::Mode0;

    info!("Configured SPI");
    // spi_config.frequency = const {
    //     spi::main::highest_freq_in(
    //         spi::main::Kilohertz::kHz(1000)..=spi::main::Kilohertz::kHz(2000),
    //     )
    // };

    let spi_bus = pins::EpdSpi::new(
        peripherals.spi_sck,
        peripherals.spi_miso,
        peripherals.spi_mosi,
        spi_config,
    );

    info!("Created SPI bus");

    let _ = SPI_BUS.set(Mutex::new(spi_bus));

    let cs_output = gpio::Output::new(peripherals.spi_cs, gpio::Level::High);
    let dc = gpio::Output::new(peripherals.dc, gpio::Level::High);
    let busy = gpio::Input::builder(peripherals.busy, gpio::Pull::Up)
        .build_with_interrupt()
        .unwrap();
    let reset = gpio::Output::new(peripherals.reset, gpio::Level::High);

    let spi_device = SpiDevice::new(SPI_BUS.get().unwrap(), cs_output);

    // let config = ssd1680_rs::config::DisplayConfig {
    //     width: 128,
    //     height: 296,
    //     gate_scanning_gd: false,
    //     gate_scanning_sm: false,
    //     gate_scanning_tb: false,
    //     partial_refresh_sequence: 0xFC,
    //     full_refresh_sequence: 0xF7,
    //     border_waveform_control: VDBMode::GSTransition(true, LUTSelect::LUT1),
    //     ram_content_for_display_update: UpdateRamOption::Normal,
    //     s8_source_output_mode: true,
    //     use_internal_temperature_sensor: true,
    // };

    let config = ssd1680_rs::config::DisplayConfig::epd_290_t94();

    let mut epd_controller =
        ssd1680_rs::driver_async::SSD1680::new(reset, dc, busy, Delay, spi_device, config);

    let mut draw_target = drawer::SsdTarget::new();

    // Include the BMP file data.
    let bmp_data = include_bytes!("../hexacube.bmp");

    // Parse the BMP file.
    let bmp: Bmp<'_, BinaryColor> = Bmp::from_slice(bmp_data).unwrap();

    draw_target.clear(BinaryColor::On);

    info!("flushing display");
    draw_target.flush(&mut epd_controller).await;

    info!("full refresh");

    // epd_controller.full_refresh().await.unwrap();
    info!("full refresh done");

    Timer::after_millis(500).await;

    info!("entering main loop");

    loop {
        draw_target.clear(BinaryColor::On);
        // Draw the image with the top left corner at (10, 20) by wrapping it in
        // an embedded-graphics `Image`.
        Image::new(&bmp, Point::new(92, 0))
            .draw(&mut draw_target)
            .unwrap();

        draw_target.flush(&mut epd_controller).await;

        Timer::after(ariel_os::time::Duration::from_millis(10000)).await;
    }
}

mod drawer {
    use embedded_graphics::{
        Pixel,
        pixelcolor::BinaryColor,
        prelude::{Dimensions, DrawTarget},
    };
    use embedded_hal::digital::{InputPin, OutputPin};
    use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};
    use ssd1680_rs::driver_async::SSD1680;

    pub struct SsdTarget {
        pub frame_buffer: [u8; super::FRAME_BUFFER_SIZE / 8],
    }
    impl SsdTarget {
        pub fn new() -> Self {
            Self {
                frame_buffer: [0u8; super::FRAME_BUFFER_SIZE / 8],
            }
        }
        pub async fn flush<
            RST: OutputPin,
            DC: OutputPin,
            BUSY: InputPin + Wait,
            DELAY: DelayNs,
            SPI: SpiDevice,
        >(
            &self,
            driver: &mut SSD1680<RST, DC, BUSY, DELAY, SPI>,
        ) {
            driver.hw_init().await.unwrap();

            driver
                .write_bw_bytes(&self.frame_buffer[0..(128 * 296 / 8) as usize])
                .await
                .unwrap();
            driver.full_refresh().await.unwrap();
            driver.enter_deep_sleep().await.unwrap();
        }
    }

    impl DrawTarget for SsdTarget {
        type Color = BinaryColor;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(coord, color) in pixels {
                let x = 127 - coord.y;
                let y = coord.x;

                let index = (y as usize) * 128 / 8 + (x as usize) / 8;

                if color == BinaryColor::On {
                    self.frame_buffer[index] |= 0x80 >> (x % 8);
                } else {
                    self.frame_buffer[index] &= !(0x80 >> (x % 8));
                }
            }
            Ok(())
        }
    }
    impl Dimensions for SsdTarget {
        fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
            embedded_graphics::primitives::Rectangle::new(
                embedded_graphics::prelude::Point::new(0, 0),
                embedded_graphics::prelude::Size::new(296, 128),
            )
        }
    }
}
