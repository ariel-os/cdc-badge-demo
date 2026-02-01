use ariel_os::hal::{peripherals, spi};

ariel_os::hal::define_peripherals!(I2cPeripherals {
    i2c_sda: GPIO17,
    i2c_scl: GPIO18,
});

ariel_os::hal::define_peripherals!(EpdLight { led: GPIO8 });

ariel_os::hal::define_peripherals!(Epd {
    busy: GPIO42,
    dc: GPIO45,
    reset: GPIO46,

    spi_miso: GPIO11,
    spi_sck: GPIO12,
    spi_mosi: GPIO13,
    spi_cs: GPIO41,
});

pub type EpdSpi = spi::main::SPI2;

pub type SensorI2c = ariel_os::hal::i2c::controller::I2C0;

ariel_os::hal::define_peripherals!(I2cBus {
    i2c_sda: GPIO17,
    i2c_scl: GPIO18,
});
