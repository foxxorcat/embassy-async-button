#![no_std]
#![allow(async_fn_in_trait)]

pub mod adc;
pub mod adc_keypad;
pub mod config;
pub mod gpio;
pub mod matrix;

pub use config::*;
use embassy_futures::select::{select, Either};
use embassy_time::{Instant, Timer};

use crate::config::ButtonConfig;

/// 一个trait，抽象了所有可以提供异步“按下”和“释放”事件的硬件源。
pub trait AsyncButtonDriver {
    async fn wait_for_press(&mut self);
    async fn wait_for_release(&mut self);
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonState {
    Idle,
    PressDebouncing {
        count: u8,
        start_time: Instant,
    },
    Pressed {
        start_time: Instant,
        count: u8,
    },
    ReleaseDebouncing {
        count: u8,
        press_start: Instant,
        release_start: Instant,
    },
    WaitingForMultiClick {
        count: u8,
        last_release: Instant,
    },
    LongPress {
        start_time: Instant,
    },
}

pub struct Button<T: AsyncButtonDriver> {
    driver: T,
    config: ButtonConfig,
    state: ButtonState,
}

impl<T: AsyncButtonDriver> Button<T> {
    pub fn new(driver: T, config: ButtonConfig) -> Self {
        Self {
            driver,
            config,
            state: ButtonState::Idle,
        }
    }

    pub async fn next_event(&mut self) -> ButtonEvent {
        loop {
            match self.state {
                ButtonState::Idle => {
                    self.driver.wait_for_press().await;
                    self.state = ButtonState::PressDebouncing {
                        count: 0,
                        start_time: Instant::now(),
                    };
                }

                ButtonState::PressDebouncing { count, start_time } => {
                    let debounce_timer = Timer::at(start_time + self.config.debounce);
                    match select(self.driver.wait_for_release(), debounce_timer).await {
                        Either::First(_) => {
                            self.state = ButtonState::Idle;
                        }
                        Either::Second(_) => {
                            self.state = ButtonState::Pressed {
                                start_time: Instant::now(),
                                count: count + 1,
                            };
                        }
                    }
                }

                ButtonState::Pressed { start_time, count } => {
                    let long_press_timer = Timer::at(start_time + self.config.long_press_time);
                    match select(self.driver.wait_for_release(), long_press_timer).await {
                        Either::First(_) => {
                            self.state = ButtonState::ReleaseDebouncing {
                                count,
                                press_start: start_time,
                                release_start: Instant::now(),
                            };
                        }
                        Either::Second(_) => {
                            self.state = ButtonState::LongPress {
                                start_time,
                            };
                            return ButtonEvent::LongPressStart;
                        }
                    }
                }

                ButtonState::ReleaseDebouncing { count, press_start, release_start } => {
                    let debounce_timer = Timer::at(release_start + self.config.debounce);
                    match select(self.driver.wait_for_press(), debounce_timer).await {
                        Either::First(_) => {
                            self.state = ButtonState::Pressed {
                                start_time: press_start,
                                count,
                            };
                        }
                        Either::Second(_) => {
                            self.state = ButtonState::WaitingForMultiClick {
                                count,
                                last_release: release_start,
                            };
                        }
                    }
                }

                ButtonState::WaitingForMultiClick {
                    count,
                    last_release,
                } => {
                    let multi_click_timer = Timer::at(last_release + self.config.multi_click_window);
                    match select(self.driver.wait_for_press(), multi_click_timer).await {
                        Either::First(_) => {
                            self.state = ButtonState::PressDebouncing {
                                count,
                                start_time: Instant::now(),
                            };
                        }
                        Either::Second(_) => {
                            self.state = ButtonState::Idle;
                           return match count {
                                0 => {
                                    // 如果 count 为 0 (来自长按释放)，则不产生事件，直接继续循环
                                    continue;
                                }
                                1 => ButtonEvent::Click,
                                2 => ButtonEvent::DoubleClick,
                                n => ButtonEvent::MultipleClick { count: n },
                            };
                        }
                    }
                }

                ButtonState::LongPress { start_time } => {
                    // 计算下一次保持事件的时间点
                    let next_hold_time = start_time + self.config.long_press_hold_interval;
                    let hold_timer = Timer::at(next_hold_time);
                    
                    match select(self.driver.wait_for_release(), hold_timer).await {
                        Either::First(_) => {
                            self.state = ButtonState::Idle;
                            return ButtonEvent::LongPressRelease;
                        }
                        Either::Second(_) => {
                            // 更新开始时间为下一次保持事件的时间点
                            self.state = ButtonState::LongPress {
                                start_time: next_hold_time,
                            };
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
    
    /// 获取底层驱动的不可变引用
    pub fn driver(&self) -> &T {
        &self.driver
    }

    /// 获取底层驱动的可变引用
    pub fn driver_mut(&mut self) -> &mut T {
        &mut self.driver
    }
    
    /// 重置按钮状态到初始空闲状态
    pub fn reset(&mut self) {
        self.state = ButtonState::Idle;
    }
}