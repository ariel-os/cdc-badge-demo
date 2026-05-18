use core::{cell::RefCell, marker::PhantomData};

use alloc::vec::Vec;
use ariel_os::{
    log::{Debug2Format, debug, error},
    time::Timer,
};

use embassy_futures::select::Either3;
use embassy_sync::{
    blocking_mutex::{
        self,
        raw::{CriticalSectionRawMutex, RawMutex},
    },
    channel::Receiver,
    pubsub::Subscriber,
};
use ratatui::{
    Terminal,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    prelude::Backend,
    style::{
        Color, Stylize,
        palette::material::{BLACK, WHITE},
    },
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use crate::{
    ble,
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
    messages: blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<heapless::HistoryBuf<heapless::String<{ crate::ble::MAX_TX_PACKET_SIZE }>, 10>>,
    >,
}

impl<B: Backend> App<B> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            buttons_down: blocking_mutex::Mutex::new(RefCell::new(heapless::Vec::new())),

            messages: blocking_mutex::Mutex::new(RefCell::new(heapless::HistoryBuf::new())),
        }
    }

    async fn handle_messages(
        &self,
        // subscriber: &mut Subscriber<>,
        receiver: &Receiver<
            '_,
            CriticalSectionRawMutex,
            heapless::String<{ crate::ble::MAX_TX_PACKET_SIZE }>,
            10,
        >,
    ) {
        loop {
            let message = receiver.receive().await;
            {
                self.messages.lock(|messages_ref| {
                    let mut messages = messages_ref.borrow_mut();
                    messages.write(message);
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
                    Button::BtnNo => Some(NextScreen::Back),
                    Button::Btn0 => {
                        self.messages.lock(|messages_ref| {
                            let mut messages = messages_ref.borrow_mut();
                            messages.clear();
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

    pub async fn run<'b, M: RawMutex, const CAP: usize, const SUBS: usize, const PUBS: usize>(
        &mut self,
        terminal: &mut Terminal<B>,
        subscriber: &mut Subscriber<'b, M, (Button, ButtonSatuChange), CAP, SUBS, PUBS>,
    ) where
        B::Error: 'static,
    {
        let ble_gatt_receiver = ble::TX_CHANNEL.receiver();

        match embassy_futures::select::select3(
            self.handle_messages(&ble_gatt_receiver),
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

impl<B: Backend> Widget for &App<B> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Length(1), Min(1), Length(1)]);
        let [header_area, inner_area, _footer_area] = vertical.areas(area);

        let messages = self.messages.lock(|messages_ref| {
            messages_ref
                .borrow()
                .oldest_ordered()
                .rev() // Puts the newest message on top
                .cloned()
                .collect::<Vec<_>>()
        });

        let items: Vec<Line> = messages
            .iter()
            .map(|m| Line::from_iter([Span::from("* "), Span::from(m.as_str())]))
            .collect();

        Paragraph::new(items)
            .style(Color::White)
            .scroll((0, 0))
            .wrap(Wrap { trim: false })
            .render(inner_area, buf);

        Paragraph::new("GATT Messaging".bg(BLACK).fg(WHITE))
            .wrap(Wrap { trim: true })
            .centered()
            .render(header_area, buf);
    }
}
