use core::{cell::RefCell, marker::PhantomData};

use ariel_os::{
    debug::log::{debug, info, warn},
    gpio::Output,
    time::Timer,
};
use embassy_sync::{
    blocking_mutex::{
        self,
        raw::{CriticalSectionRawMutex, RawMutex},
    },
    pubsub::Subscriber,
};
use embedded_hal::digital::StatefulOutputPin;
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
    widgets::{List, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};

use crate::buttons::{Button, ButtonSatuChange};

pub struct App<B: Backend> {
    _marker: PhantomData<B>,
    buttons_down: blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<heapless::Vec<Button, { Button::COUNT }>>,
    >,
    list_state: blocking_mutex::Mutex<CriticalSectionRawMutex, RefCell<ListState>>,
    backlight: RefCell<Output>,
}

impl<B: Backend> App<B> {
    pub fn new(backlight: Output) -> Self {
        Self {
            _marker: PhantomData,
            buttons_down: blocking_mutex::Mutex::new(RefCell::new(heapless::Vec::new())),
            list_state: blocking_mutex::Mutex::new(RefCell::new(
                ListState::default().with_selected(Some(0)),
            )),
            backlight: RefCell::new(backlight),
        }
    }

    async fn handle_inputs<
        'a,
        M: RawMutex,
        const CAP: usize,
        const SUBS: usize,
        const PUBS: usize,
    >(
        &self,
        mut subscriber: Subscriber<'a, M, (Button, ButtonSatuChange), CAP, SUBS, PUBS>,
    ) {
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
                match event.0 {
                    Button::Btn2 => self.list_state.lock(|s| s.borrow_mut().select_previous()),
                    Button::Btn8 => self.list_state.lock(|s| s.borrow_mut().select_next()),
                    Button::BtnYes | Button::Btn5 => self.handle_enter().await,
                    _ => {}
                }
            }
        }
    }

    pub async fn handle_enter(&self) {
        match self.list_state.lock(|s| s.borrow().selected()) {
            Some(1) => {
                info!("Toggling backlight");

                self.backlight.borrow_mut().toggle();
            }
            Some(e) => {
                info!("No function for list entry {}", e);
            }
            None => {
                warn!("ListState has None selected");
            }
        }
    }

    pub async fn run<'a, M: RawMutex, const CAP: usize, const SUBS: usize, const PUBS: usize>(
        &mut self,
        terminal: &mut Terminal<B>,
        subscriber: Subscriber<'a, M, (Button, ButtonSatuChange), CAP, SUBS, PUBS>,
    ) where
        B::Error: 'static,
    {
        embassy_futures::join::join(self.handle_inputs(subscriber), async {
            debug!("Running the Tabs app");
            loop {
                terminal
                    .draw(|frame| frame.render_widget(&*self, frame.area()))
                    .unwrap();
                Timer::after_millis(100).await
            }
        })
        .await;
    }
}

impl<B: Backend> Widget for &App<B> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Length(1), Min(1), Length(1)]);
        let [header_area, inner_area, _footer_area] = vertical.areas(area);

        let led_status = if self.backlight.borrow_mut().is_set_high().unwrap_or(false) {
            "ON"
        } else {
            "OFF"
        };

        let items: [Line; 4] = [
            "Item 1".into(),
            Line::from_iter([Span::from("Backlight: "), Span::from(led_status)]),
            "Item 3".into(),
            "Item 4".into(),
        ];
        let list = List::new(items)
            .style(Color::White)
            .highlight_style(Style::new().bg(WHITE).fg(BLACK))
            .highlight_symbol("> ");
        self.list_state.lock(|s| {
            StatefulWidget::render(list, inner_area, buf, &mut s.borrow_mut());
        });

        Paragraph::new("Menu demo ".bg(BLACK).fg(WHITE))
            .wrap(Wrap { trim: true })
            .centered()
            .render(header_area, buf);
    }
}
