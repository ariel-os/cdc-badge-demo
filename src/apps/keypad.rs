use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{cell::RefCell, cmp, marker::PhantomData, sync::atomic::AtomicBool};

use ariel_os::{
    log::{Debug2Format, error, info, warn},
    time::Timer,
};
use bt_hci::param::BdAddr;
use embassy_futures::select::Either3;
use embassy_sync::{
    blocking_mutex::{
        self,
        raw::{CriticalSectionRawMutex, RawMutex},
    },
    channel::Receiver,
    pubsub::Subscriber,
};
use heapless::index_map::FnvIndexMap;
use ratatui::{
    Terminal,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    prelude::Backend,
    style::{
        Color, Style, Stylize,
        palette::material::{BLACK, WHITE},
    },
    text::{Line, Span},
    widgets::{
        List, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget, Wrap,
    },
};

use crate::{
    ble::{self, Contact, ContactData},
    buttons::{Button, ButtonSatuChange},
};







#[derive(Debug)]
pub enum NextScreen {
    Back,
}

/// Receives the scanned devices from [`crate::ble::SCANNED_DEVICES_CHANNEL`] and displays them on the screen.
/// Pressing "9" toggles the filter that only displays devices with names.
pub struct App<B: Backend> {
    _marker: PhantomData<B>,
    buttons_down: blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<heapless::Vec<Button, { Button::COUNT }>>,
    >,
    list_state: blocking_mutex::Mutex<CriticalSectionRawMutex, RefCell<ListState>>,

    devices: blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<FnvIndexMap<BdAddr, ContactData, 32>>,
    >,

    only_show_devices_with_names: AtomicBool,
}

impl<B: Backend> App<B> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            buttons_down: blocking_mutex::Mutex::new(RefCell::new(heapless::Vec::new())),
            list_state: blocking_mutex::Mutex::new(RefCell::new(
                ListState::default().with_selected(Some(0)),
            )),
            devices: blocking_mutex::Mutex::new(RefCell::new(FnvIndexMap::new())),
            only_show_devices_with_names: AtomicBool::new(true),
        }
    }

    async fn handle_devices_events(
        &self,
        receiver: &Receiver<'_, CriticalSectionRawMutex, Contact, 32>,
    ) {
        loop {
            let contact = receiver.receive().await;
            {
                if self
                    .only_show_devices_with_names
                    .load(core::sync::atomic::Ordering::Acquire)
                    && contact.data.name.is_none()
                {
                    continue;
                }
                self.devices.lock(|devices_ref| {
                    let mut devices = devices_ref.borrow_mut();
                    if let Some(c) = devices.get_mut(&contact.addr) {
                        if contact.data.name.is_some() {
                            c.name = contact.data.name;
                        }
                        c.rssi = contact.data.rssi;
                        c.seen_at = contact.data.seen_at;
                    } else {
                        if devices.is_full() {
                            // remove oldest
                            if let Some(oldest_key) = devices
                                .iter()
                                .min_by_key(|(_, v)| v.seen_at.as_ticks())
                                .map(|a| *a.0)
                            {
                                devices.remove(&oldest_key);
                            }
                        }

                        let _ = devices.insert(contact.addr, contact.data);
                    }
                });
            }
        }
    }

    async fn handle_inputs<
        'b,
        M: RawMutex,
        const CAP: usize,
        const SUBS: usize,
        const PUBS: usize,
    >(
        &self,
        subscriber: &mut Subscriber<'b, M, (Button, ButtonSatuChange), CAP, SUBS, PUBS>,
    ) -> NextScreen {
        loop {
            let event = subscriber.next_message_pure().await;
            self.buttons_down.lock(|v| {
                let mut buttons_down = v.borrow_mut();
                if event.1.was_presed {
                    let index = buttons_down.iter().position(|b| *b == event.0);
                    if let Some(i) = index {
                        buttons_down.remove(i);
                    }
                } else {
                    buttons_down.push(event.0).unwrap();
                }
            });
            // Move on key up
            if event.1.was_presed {
                let next = match event.0 {
                    Button::Btn2 => {
                        self.list_state.lock(|s| s.borrow_mut().select_previous());
                        None
                    }
                    Button::Btn8 => {
                        self.list_state.lock(|s| s.borrow_mut().select_next());
                        None
                    }
                    Button::Btn9 => {
                        self.only_show_devices_with_names.update(
                            core::sync::atomic::Ordering::Release,
                            core::sync::atomic::Ordering::Acquire,
                            |v| !v,
                        );
                        None
                    }
                    Button::BtnYes | Button::Btn5 => self.handle_enter().await,
                    Button::BtnNo => Some(NextScreen::Back),

                    Button::Btn0 => {
                        self.devices.lock(|devices_ref| {
                            let mut devices = devices_ref.borrow_mut();
                            devices.clear();
                        });
                        None
                    }
                    _ => None,
                };
                if let Some(next_screen) = next {
                    return next_screen;
                }
            }
        }
    }

    pub async fn handle_enter(&self) -> Option<NextScreen> {
        match self.list_state.lock(|s| s.borrow().selected()) {
            Some(0) => Some(NextScreen::Back),
            Some(e) => {
                info!("No function for list entry {}", e);
                None
            }
            None => {
                warn!("ListState has None selected");
                None
            }
        }
    }

    pub async fn run<'b, M: RawMutex, const CAP: usize, const SUBS: usize, const PUBS: usize>(
        &mut self,
        terminal: &mut Terminal<B>,
        subscriber: &mut Subscriber<'b, M, (Button, ButtonSatuChange), CAP, SUBS, PUBS>,
    ) where
        B::Error: 'static,
    {
        let ble_scan_receiver = ble::scanned_device_receiver();

        match embassy_futures::select::select3(
            self.handle_devices_events(&ble_scan_receiver),
            self.handle_inputs(subscriber),
            async {
                loop {
                    if let Err(e) = terminal.draw(|frame| frame.render_widget(&*self, frame.area()))
                    {
                        return e;
                    }
                    Timer::after_millis(100).await
                }
            },
        )
        .await
        {
            Either3::First(res) => {
                error!("Terminal draw error :{:?}", Debug2Format(&res));
            }
            Either3::Second(next) => match next {
                NextScreen::Back => { // return to the main app
                }
            },
            Either3::Third(res) => {
                error!("Scanner error :{:?}", Debug2Format(&res));
            }
        }
    }
}

fn format_bd_addr(addr: &BdAddr) -> String {
    let raw: Vec<String> = addr.raw().iter().map(|u| format!("{:02X}", u)).collect();

    raw.join(":")
}

impl<B: Backend> Widget for &App<B> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Length(1), Min(1), Length(1)]);
        let [header_area, center_area, footer_area] = vertical.areas(area);
        let horizontal = Layout::horizontal([Min(1), Length(1)]);
        let [inner_area, scroll_area] = horizontal.areas(center_area);

        let mut items: Vec<Line> = Vec::new();

        items.push(Line::from_iter([Span::from("Back")]));

        let mut devices: Vec<(BdAddr, ContactData)> = self
            .devices
            .lock(|c| c.borrow().iter().map(|(a, d)| (*a, d.clone())).collect());

        devices.sort_by_key(|(_, d)| cmp::Reverse(d.seen_at));

        let mut devices = devices
            .iter()
            .map(|(addr, data)| {
                Line::from_iter([
                    Span::from(format_bd_addr(addr)),
                    Span::from(" "),
                    Span::from(format!("{} db", data.rssi)),
                    Span::from(" "),
                    Span::from(data.name.clone().unwrap_or("".to_string())),
                ])
            })
            .collect();

        items.append(&mut devices);
        let items_count = items.len();

        let list = List::new(items)
            .style(Color::White)
            .highlight_style(Style::new().bg(WHITE).fg(BLACK))
            .highlight_symbol("> ");
        let scroll = self.list_state.lock(|s| {
            StatefulWidget::render(list, inner_area, buf, &mut s.borrow_mut());
            s.borrow().selected().unwrap_or(0)
        });

        if items_count > scroll_area.height as usize {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);

            // We're recreating the state for every render, there might be a better way (one challenge is that the list length may change with every render).
            let mut scrollbar_state = ScrollbarState::new(items_count);

            // FIXME: shows as fully scrolled even though there is one item remaining
            scrollbar_state = scrollbar_state.position(scroll);
            StatefulWidget::render(scrollbar, scroll_area, buf, &mut scrollbar_state);
        }

        Paragraph::new("BLE Devices".bg(BLACK).fg(WHITE))
            .wrap(Wrap { trim: true })
            .centered()
            .render(header_area, buf);

        let mut footer_items: Vec<String> = Vec::new();

        if self
            .only_show_devices_with_names
            .load(core::sync::atomic::Ordering::Acquire)
        {
            footer_items.push("Only showing devices with names".to_string());
        }

        let footer: Line = Line::from_iter(footer_items.iter());

        Paragraph::new(footer)
            .wrap(Wrap { trim: true })
            .render(footer_area, buf);
    }
}
