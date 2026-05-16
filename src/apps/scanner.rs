use core::{cell::RefCell, cmp, marker::PhantomData};

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use ariel_os::{
    log::{Debug2Format, debug, error, info, warn},
    time::{Instant, Timer},
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
    Frame, Terminal,
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
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
enum NextScreen {
    Back,
}

pub struct App<B: Backend> {
    _marker: PhantomData<B>,
    buttons_down: blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<heapless::Vec<Button, { Button::COUNT }>>,
    >,
    list_state: blocking_mutex::Mutex<CriticalSectionRawMutex, RefCell<ListState>>,

    // scroll_state: blocking_mutex::Mutex<CriticalSectionRawMutex, RefCell<ScrollbarState>>,
    contacts: blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<FnvIndexMap<BdAddr, ContactData, 32>>,
    >,
}

impl<B: Backend> App<B> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            buttons_down: blocking_mutex::Mutex::new(RefCell::new(heapless::Vec::new())),
            list_state: blocking_mutex::Mutex::new(RefCell::new(
                ListState::default().with_selected(Some(0)),
            )),
            // scroll_state: blocking_mutex::sMutex::new(RefCell::new(ScrollbarState::default())),
            contacts: blocking_mutex::Mutex::new(RefCell::new(FnvIndexMap::new())),
        }
    }

    async fn handle_contacts(
        &self,
        // subscriber: &mut Subscriber<>,
        receiver: &Receiver<'_, CriticalSectionRawMutex, Contact, 32>,
    ) {
        loop {
            let contact = receiver.receive().await;
            {
                self.contacts.lock(|contacts_ref| {
                    let mut contacts = contacts_ref.borrow_mut();
                    if let Some(c) = contacts.get_mut(&contact.addr) {
                        if contact.data.name.is_some() {
                            c.name = contact.data.name;
                        }
                        c.rssi = contact.data.rssi;
                        c.seen_at = contact.data.seen_at;
                    } else {
                        if contacts.is_full() {
                            // remove oldest
                            if let Some(oldest_key) = contacts
                                .iter()
                                .min_by_key(|(_, v)| v.seen_at.as_ticks())
                                .map(|a| a.0.clone())
                            {
                                contacts.remove(&oldest_key);
                            }
                        }

                        contacts.insert(contact.addr, contact.data);
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
                        // self.scroll_state.lock(|s| s.borrow_mut().prev());

                        None
                    }
                    Button::Btn8 => {
                        self.list_state.lock(|s| s.borrow_mut().select_next());
                        // self.scroll_state.lock(|s| s.borrow_mut().next());

                        None
                    }
                    Button::BtnYes | Button::Btn5 => self.handle_enter().await,
                    Button::BtnNo => Some(NextScreen::Back),

                    Button::Btn0 => {
                        self.contacts.lock(|contacts_ref| {
                            let mut contacts = contacts_ref.borrow_mut();
                            contacts.clear();
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
        let ble_scan_receiver = ble::CONTACTS_CHANNEL.receiver();

        match embassy_futures::select::select3(
            self.handle_contacts(&ble_scan_receiver),
            self.handle_inputs(subscriber),
            async {
                debug!("Running the Tabs app");
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
        let [header_area, center_area, _footer_area] = vertical.areas(area);
        let horizontal = Layout::horizontal([Min(1), Length(1)]);
        let [inner_area, scroll_area] = horizontal.areas(center_area);

        let mut items: Vec<Line> = Vec::new();

        items.push(Line::from_iter([Span::from("Back")]));

        let mut contacts: Vec<(BdAddr, ContactData)> = self.contacts.lock(|c| {
            c.borrow()
                .iter()
                .map(|(a, d)| (a.clone(), d.clone()))
                .collect()
        });

        contacts.sort_by_key(|(_, d)| cmp::Reverse(d.seen_at));

        let mut contacts = contacts
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

        items.append(&mut contacts);
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
            let mut scrollbar_state = ScrollbarState::new(items_count);

            // FIXME: shows as fully scrolled even though there is one item remaining
            scrollbar_state = scrollbar_state.position(scroll);
            StatefulWidget::render(scrollbar, scroll_area, buf, &mut scrollbar_state);
        }

        Paragraph::new("BLE Devices".bg(BLACK).fg(WHITE))
            .wrap(Wrap { trim: true })
            .centered()
            .render(header_area, buf);
    }
}
