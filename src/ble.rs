use alloc::string::String;
use ariel_os::log::{Debug2Format, error, info, trace, warn};
use ariel_os::time::{Duration, Instant};
use bt_hci::param::{BdAddr, LeAdvReportsIter};
use embassy_futures::join::join;
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use embassy_sync::channel::Channel;
use embassy_sync::watch::Watch;
use heapless::Vec;
use trouble_host::connection::{PhySet, ScanConfig};
use trouble_host::prelude::EventHandler;
use trouble_host::scan::Scanner;
use trouble_host::{
    BleHostError, Controller, Error,
    advertise::{AdStructure, Advertisement, BR_EDR_NOT_SUPPORTED, LE_GENERAL_DISCOVERABLE},
    gap::{GapConfig, PeripheralConfig},
    gatt::{GattConnection, GattConnectionEvent, GattEvent},
    prelude::{DefaultPacketPool, FromGatt, Peripheral, appearance, gatt_server, gatt_service},
};
pub const MAX_TX_PACKET_SIZE: usize = 32;
#[derive(Debug, Clone)]
pub struct Contact {
    pub addr: BdAddr,
    pub data: ContactData,
}
#[derive(Debug, Clone)]
pub struct ContactData {
    pub name: Option<String>,
    pub rssi: i8,
    pub seen_at: Instant,
}

// static SEEN_CHANNEL: Mutex<CriticalSectionRawMutex, Cell<TagStorageMap>> =
//     Mutex::new(Cell::new(FnvIndexMap::new()));

pub static CONTACTS_CHANNEL: Channel<CriticalSectionRawMutex, Contact, 32> = Channel::new();
pub static TX_CHANNEL: Watch<CriticalSectionRawMutex, Vec<u8, MAX_TX_PACKET_SIZE>, 8> =
    Watch::new();

const NAME: &str = "Ariel OS CDC badge";

// GATT Server definition
#[gatt_server]
struct Server {
    write_service: WriteService,
}

#[gatt_service(uuid = "fd544724-0d14-4ecc-b4b5-65f9d0ee1fe3")]
struct WriteService {
    #[characteristic(uuid = "fd544724-0d14-4ecc-b4b5-65f9cafecafe", write_without_response)]
    write_data: [u8; MAX_TX_PACKET_SIZE],
}

pub async fn run() {
    let stack = ariel_os::ble::ble_stack().await;
    let mut host = stack.build();

    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: NAME,
        appearance: &appearance::motorized_vehicle::TROLLEY,
    }))
    .unwrap();
    let printer = DiscorveryHandler {};
    let mut scanner = Scanner::new(host.central);

    let config = ScanConfig::<'_> {
        active: true,
        phys: PhySet::M1,

        // There's an issue with the Duration https://github.com/embassy-rs/bt-hci/pull/74
        // Workaround is to multiply the value by 16.

        // scan for 3 ms every 10 ms
        interval: Duration::from_millis(10 * 16),
        window: Duration::from_millis(3 * 16),
        ..Default::default()
    };

    info!("Starting advertising");
    let _ = join(host.runner.run_with_handler(&printer), async {
        let mut _session = scanner.scan(&config).await.unwrap();
        loop {
            match advertise(NAME, &mut host.peripheral, &server).await {
                Ok(conn) => {
                    // set up tasks when the connection is established to a central, so they don't run when no one is connected.
                    gatt_events_task(&server, &conn).await.unwrap();
                }
                Err(e) => {
                    panic!("[adv] error: {:?}", e);
                }
            }
        }
    })
    .await;
}

/// Stream Events until the connection closes.
///
/// This function will handle the GATT events and process them.
/// This is how we interact with read and write requests.
async fn gatt_events_task(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
) -> Result<(), Error> {
    let write_data = server.write_service.write_data;
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                info!("[gatt] disconnected: {:?}", reason);
                break;
            }
            GattConnectionEvent::Gatt { event } => {
                match &event {
                    GattEvent::Other(event) => {
                        warn!("[gatt] Other event : {:?}", event.payload().handle());
                    }
                    GattEvent::Read(event) => {
                        warn!("[gatt] Read Event to Characteristic: {:?}", event.handle());
                    }
                    GattEvent::Write(event) => {
                        if event.handle() == write_data.handle {
                            let data = event.data();
                            info!("[gatt] Write Event to Characteristic: {:?}", data);
                            let len = data.len().min(MAX_TX_PACKET_SIZE);
                            let mut vec = Vec::<u8, MAX_TX_PACKET_SIZE>::new();

                            let err = vec.extend_from_slice(&data[..len]);
                            if err.is_err() {
                                warn!("[gatt] error extending vec, dropping packet");
                            } else {
                                TX_CHANNEL.sender().send(vec);
                            }
                        }
                    }
                }

                // This step is also performed at drop(), but writing it explicitly is necessary
                // in order to ensure reply is sent.
                match event.accept() {
                    Ok(reply) => {
                        reply.send().await;
                    }
                    Err(e) => warn!("[gatt] error sending response: {:?}", e),
                }
            }
            _ => {}
        }
    }
    info!("[gatt] task finished");
    Ok(())
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
async fn advertise<'a, 'b, C: Controller>(
    name: &'a str,
    peripheral: &mut Peripheral<'a, C, DefaultPacketPool>,
    server: &'b Server<'_>,
) -> Result<GattConnection<'a, 'b, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut advertiser_data = [0; 31];
    AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            // AdStructure::ServiceUuids16(&[[0x0f, 0x18]]),
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut advertiser_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertiser_data[..],
                scan_data: &[],
            },
        )
        .await?;
    info!("[adv] advertising");
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    info!("[adv] connection established");
    Ok(conn)
}

struct DiscorveryHandler {}

impl EventHandler for DiscorveryHandler {
    fn on_adv_reports(&self, mut it: LeAdvReportsIter<'_>) {
        while let Some(Ok(report)) = it.next() {
            let adv_data = AdStructure::decode(report.data);

            let name = {
                let mut decoded = None;
                let mut decoded_short = None;

                for adv in adv_data {
                    match adv {
                        Ok(AdStructure::ShortenedLocalName(data)) => {
                            decoded_short = str::from_utf8(data).ok();
                            if decoded_short.is_none() {
                                warn!("failed to decode name");
                            }
                        }
                        Ok(AdStructure::CompleteLocalName(data)) => {
                            decoded = str::from_utf8(data).ok();
                            if decoded.is_none() {
                                warn!("failed to decode name");
                            }
                            break;
                        }

                        Ok(adv) => {
                            trace!("unknown advertisement {:?}", adv);
                        }
                        Err(e) => {
                            trace!("error decoding advertisement: {:?}", e);
                        }
                    }
                }
                decoded.or(decoded_short)
            };

            let c = Contact {
                addr: report.addr,
                data: ContactData {
                    name: name.map(|s| s.into()),
                    rssi: report.rssi,
                    seen_at: Instant::now(),
                },
            };

            info!("contact: {:?}", Debug2Format(&c));

            if let Err(e) = CONTACTS_CHANNEL.try_send(c) {
                error!("Error sending new contact: {:?}", Debug2Format(&e));
            }
        }
    }
}
