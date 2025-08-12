pub mod adc;
pub mod adc_keypad;
pub mod config;
pub mod gpio;
pub mod matrix;

pub use config::*;

use embassy_futures::select::{select, Either};
use embassy_time::{Instant, Timer};

// 从 config 模块导入 ButtonConfig
use crate::config::ButtonConfig;

/// 一个trait，抽象了所有可以提供异步“按下”和“释放”事件的硬件源。
pub trait AsyncButtonDriver {
    async fn wait_for_press(&mut self);
    async fn wait_for_release(&mut self);
}

/// 按钮可能产生的逻辑事件类型。
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Click,
    DoubleClick,
    MultipleClick { count: u8 },
    LongPressStart,
    LongPressHold,
    LongPressRelease,
}

/// 按钮的内部状态机。
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Debouncing,
    Pressed {
        press_time: Instant,
    },
    WaitingForNextClick {
        count: u8,
        last_release_time: Instant,
    },
    LongPress,
}

pub struct Button<T: AsyncButtonDriver> {
    press_source: T,
    config: ButtonConfig,
    state: State,
}

impl<T: AsyncButtonDriver> Button<T> {
    pub fn new(press_source: T, config: ButtonConfig) -> Self {
        Self {
            press_source,
            config,
            state: State::Idle,
        }
    }

    /// 异步获取下一个按钮事件。
    pub async fn next_event(&mut self) -> ButtonEvent {
        loop {
            match self.state {
                State::Idle => {
                    self.press_source.wait_for_press().await;
                    self.state = State::Debouncing;
                }
                State::Debouncing => {
                    let debounce_timer = Timer::after(self.config.debounce);
                    match select(self.press_source.wait_for_release(), debounce_timer).await {
                        Either::First(_) => {
                            self.state = State::Idle;
                        }
                        Either::Second(_) => {
                            self.state = State::Pressed {
                                press_time: Instant::now(),
                            };
                        }
                    }
                }
                State::Pressed { press_time } => {
                    let long_press_timer = Timer::at(press_time + self.config.long_press_time);
                    match select(self.press_source.wait_for_release(), long_press_timer).await {
                        Either::First(_) => {
                            self.state = State::WaitingForNextClick {
                                count: 1,
                                last_release_time: Instant::now(),
                            };
                        }
                        Either::Second(_) => {
                            self.state = State::LongPress;
                            return ButtonEvent::LongPressStart;
                        }
                    }
                }
                State::WaitingForNextClick {
                    count,
                    last_release_time,
                } => {
                    let multi_click_timer =
                        Timer::at(last_release_time + self.config.multi_click_window);
                    match select(self.press_source.wait_for_press(), multi_click_timer).await {
                        Either::First(_) => {
                            self.press_source.wait_for_release().await;
                            self.state = State::WaitingForNextClick {
                                count: count + 1,
                                last_release_time: Instant::now(),
                            };
                        }
                        Either::Second(_) => {
                            self.state = State::Idle;
                            return match count {
                                1 => ButtonEvent::Click,
                                2 => ButtonEvent::DoubleClick,
                                n => ButtonEvent::MultipleClick { count: n },
                            };
                        }
                    }
                }
                State::LongPress => {
                    let hold_timer = Timer::after(self.config.long_press_hold_interval);
                    match select(self.press_source.wait_for_release(), hold_timer).await {
                        Either::First(_) => {
                            self.state = State::Idle;
                            return ButtonEvent::LongPressRelease;
                        }
                        Either::Second(_) => {
                            return ButtonEvent::LongPressHold;
                        }
                    }
                }
            }
        }
    }

    pub fn set_config(&mut self, new_config: ButtonConfig) {
        self.config = new_config;
    }

    pub fn driver(&self) -> &T {
        &self.press_source
    }

    pub fn driver_mut(&mut self) -> &mut T {
        &mut self.press_source
    }
}
