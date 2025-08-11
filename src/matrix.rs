use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Subscriber},
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

// 通道和订阅者的类型别名
type MatrixChannel<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    PubSubChannel<CriticalSectionRawMutex, KeyEvent, MSG_CAP, SUBS, SUBSCRIBER_CAP>;

type MatrixSubscriber<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    Subscriber<'a, CriticalSectionRawMutex, KeyEvent, MSG_CAP, SUBS, SUBSCRIBER_CAP>;

/// 矩阵键盘组，负责拥有IO引脚、运行扫描循环并广播按键事件。
pub struct MatrixKeyboardGroup<
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
    channel: &'a MatrixChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
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
    > MatrixKeyboardGroup<'a, C, R, COLS, ROWS, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    pub fn new(
        cols: [C; COLS],
        rows: [R; ROWS],
        channel: &'a MatrixChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    ) -> Self {
        Self {
            cols,
            rows,
            channel,
            last_states: [[false; ROWS]; COLS],
        }
    }

    /// 工厂方法：直接创建一个与此组关联的 MatrixButton 实例。
    pub fn button(&self, row: u8, col: u8) -> MatrixButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP> {
        assert!(
            (row as usize) < ROWS && (col as usize) < COLS,
            "Button position out of bounds"
        );
        MatrixButton {
            subscriber: self.channel.subscriber().unwrap(),
            row,
            col,
        }
    }

    /// 运行矩阵扫描循环。
    pub async fn run(mut self) -> ! {
        let publisher = self.channel.publisher().unwrap();
        loop {
            for c in 0..COLS {
                // 1. 激活当前列 (设置为低电平)
                let _ = self.cols[c].set_low();

                // 2. 短暂延时以稳定电平
                Timer::after_micros(50).await;

                // 3. 读取该列所有行的状态
                for r in 0..ROWS {
                    let is_pressed = self.rows[r].is_low().unwrap_or(false);
                    let was_pressed = self.last_states[c][r];

                    // 4. 如果状态发生变化，则发布事件
                    if is_pressed != was_pressed {
                        self.last_states[c][r] = is_pressed;
                        let event = KeyEvent {
                            row: r as u8,
                            col: c as u8,
                            pressed: is_pressed,
                        };
                        publisher.publish(event).await;
                    }
                }

                // 5. 取消激活当前列
                let _ = self.cols[c].set_high();
            }
            // 控制整体扫描频率
            Timer::after(Duration::from_millis(5)).await;
        }
    }
}

/// 代表矩阵键盘中的一个具体按键。
pub struct MatrixButton<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> {
    subscriber: MatrixSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
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
