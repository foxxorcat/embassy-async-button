use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Publisher, Subscriber},
};
use embassy_time::{Duration, Timer};
use embedded_hal::digital::{InputPin, OutputPin};

use crate::AsyncButtonDriver;

/// 表示矩阵键盘上的一个按键事件。
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub row: u8,
    pub col: u8,
    pub pressed: bool,
}

pub type MatrixEventChannel<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> = PubSubChannel<CriticalSectionRawMutex, KeyEvent, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
pub type MatrixEventSubscriber<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> = Subscriber<'a, CriticalSectionRawMutex, KeyEvent, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
type MatrixEventPublisher<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> = Publisher<'a, CriticalSectionRawMutex, KeyEvent, MSG_CAP, SUBS, SUBSCRIBER_CAP>;

#[derive(Clone)]
pub struct MatrixButtonFactory<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    channel: &'a MatrixEventChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize>
    MatrixButtonFactory<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    /// 根据行列号创建一个新的矩阵按键实例。
    pub fn button(&self, row: u8, col: u8) -> MatrixButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP> {
        MatrixButton {
            subscriber: self.channel.subscriber().unwrap(),
            row,
            col,
        }
    }
}

pub struct MatrixDriver<
    'a,
    C: OutputPin,
    R: InputPin,
    const COLS: usize,
    const ROWS: usize,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    cols: [C; COLS],
    rows: [R; ROWS],
    publisher: MatrixEventPublisher<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    last_states: [[bool; ROWS]; COLS],
}

impl<
        'a,
        C: OutputPin,
        R: InputPin,
        const COLS: usize,
        const ROWS: usize,
        const MSG_CAP: usize,
        const SUBS: usize,
        const SUBSCRIBER_CAP: usize,
    > MatrixDriver<'a, C, R, COLS, ROWS, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    /// 创建一个新的矩阵驱动及其关联的按键工厂。
    ///
    /// 这是设置矩阵键盘的唯一入口点。
    ///
    /// # 返回
    /// 一个元组，包含:
    /// - `MatrixDriver`: 需要被 spawn 到后台任务中运行。
    /// - `MatrixButtonFactory`: 用于在程序中创建具体的按键实例。
    pub fn new(
        cols: [C; COLS],
        rows: [R; ROWS],
        channel: &'a MatrixEventChannel<MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    ) -> (Self, MatrixButtonFactory<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>) {
        let driver = Self {
            cols,
            rows,
            publisher: channel.publisher().unwrap(),
            last_states: [[false; ROWS]; COLS],
        };
        let factory = MatrixButtonFactory { channel };
        (driver, factory)
    }

    pub async fn run(mut self) -> ! {
        loop {
            for c in 0..COLS {
                let _ = self.cols[c].set_low();
                Timer::after_micros(50).await;

                for r in 0..ROWS {
                    let is_pressed = self.rows[r].is_low().unwrap_or(false);
                    if is_pressed != self.last_states[c][r] {
                        self.last_states[c][r] = is_pressed;
                        let event = KeyEvent {
                            row: r as u8,
                            col: c as u8,
                            pressed: is_pressed,
                        };
                        self.publisher.publish(event).await;
                    }
                }
                let _ = self.cols[c].set_high();
            }
            Timer::after(Duration::from_millis(5)).await;
        }
    }
}

pub struct MatrixButton<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> {
    subscriber: MatrixEventSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    row: u8,
    col: u8,
}

impl<const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> AsyncButtonDriver
    for MatrixButton<'_, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    async fn wait_for_press(&mut self) {
        loop {
            let event = self.subscriber.next_message_pure().await;
            if event.row == self.row && event.col == self.col && event.pressed {
                return;
            }
        }
    }

    async fn wait_for_release(&mut self) {
        loop {
            let event = self.subscriber.next_message_pure().await;
            if event.row == self.row && event.col == self.col && !event.pressed {
                return;
            }
        }
    }
}
