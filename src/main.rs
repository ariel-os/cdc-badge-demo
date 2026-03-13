#![no_main]
#![no_std]

mod app;
mod buttons;
mod drawer;
mod pins;

extern crate alloc;

use alloc::boxed::Box;
use ariel_os::{
    debug::log::{Debug2Format, debug, info},
    gpio::{self},
    hal::{self, group_peripherals},
    i2c,
    spi::{self, main::SpiDevice},
    time::{Delay, Instant, Timer},
};
use async_tca9535::{
    Tca9535,
    registers::{Configuration, Polarity},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, pubsub::PubSubChannel, watch::Watch,
};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget};
use embedded_hal_async::i2c::I2c as _;
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig};
use ratatui::Terminal;

use crate::{
    app::App,
    buttons::ButtonsStatus,
    drawer::{SsdTarget, SsdTargetManager},
};

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

group_peripherals!(Screen {
    epd: pins::Epd,
    light: pins::EpdLight
});

static WATCH: Watch<CriticalSectionRawMutex, [u8; 4736], 1> = Watch::new();

#[ariel_os::task(autostart, peripherals)]
async fn screen(peripherals: Screen) {
    static SPI_BUS: once_cell::sync::OnceCell<
        Mutex<embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, hal::spi::main::Spi>,
    > = once_cell::sync::OnceCell::new();

    info!("Starting EPD demo");
    let mut spi_config = hal::spi::main::Config::default();
    spi_config.frequency = const {
        spi::main::highest_freq_in(spi::main::Kilohertz::MHz(1)..=spi::main::Kilohertz::MHz(20))
    };

    info!("Configured SPI");

    let spi_bus = pins::EpdSpi::new(
        peripherals.epd.spi_sck,
        peripherals.epd.spi_miso,
        peripherals.epd.spi_mosi,
        spi_config,
    );

    info!("Created SPI bus");

    let _ = SPI_BUS.set(Mutex::new(spi_bus));

    let cs_output: gpio::Output = gpio::Output::new(peripherals.epd.spi_cs, gpio::Level::High);
    let dc = gpio::Output::new(peripherals.epd.dc, gpio::Level::High);
    let busy = gpio::Input::builder(peripherals.epd.busy, gpio::Pull::Up)
        .build_with_interrupt()
        .unwrap();
    let reset = gpio::Output::new(peripherals.epd.reset, gpio::Level::High);
    let backlight = gpio::Output::new(peripherals.light.led, gpio::Level::Low);

    let spi_device = SpiDevice::new(SPI_BUS.get().unwrap(), cs_output);

    let config = ssd1680_rs::config::DisplayConfig::epd_290_t94();

    let mut epd_controller: ssd1680_rs::driver_async::SSD1680<_, _, _, _, _> =
        ssd1680_rs::driver_async::SSD1680::new(reset, dc, busy, Delay, spi_device, config);

    epd_controller.hw_init().await.unwrap();

    let sender = WATCH.sender();
    let receiver = WATCH.receiver().unwrap();

    let mut manager = SsdTargetManager::new(epd_controller, receiver);

    let mut draw_target = drawer::SsdTarget::new(sender);

    // Off = black
    draw_target.clear(BinaryColor::Off);
    draw_target.flush();

    info!("entering main loop");

    let config = EmbeddedBackendConfig {
        flush_callback: Box::new(|d: &mut SsdTarget| {
            d.flush();
        }),
        ..Default::default()
    };

    let mut app = App::new(backlight);
    let backend = EmbeddedBackend::new(&mut draw_target, config);

    let mut terminal = Terminal::new(backend).unwrap();

    let receiver = BUTTONS_CHANNEL.subscriber().unwrap();

    embassy_futures::join::join(manager.run(), async {
        app.run(&mut terminal, receiver).await;
    })
    .await;
}
