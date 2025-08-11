use crate::AsyncButtonDriver;
use embedded_hal::digital::InputPin;
use embedded_hal_async::digital::Wait;

/// 定义GPIO按钮的有效电平。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveLevel {
    /// 低电平有效（例如，使用上拉电阻，按下时引脚接地）。
    Low,
    /// 高电平有效（例如，使用下拉电阻，按下时引脚接VCC）。
    High,
}

/// 一个直接由GPIO输入引脚驱动的按钮。
///
/// 它是最简单的按钮驱动，封装了一个 `InputPin`，
/// 并实现了 `AsyncButtonDriver` trait。
pub struct GpioButton<P: InputPin> {
    pin: P,
    active_level: ActiveLevel,
}

impl<P: InputPin> GpioButton<P> {
    /// 创建一个新的GPIO按钮。
    ///
    /// # 参数
    /// * `pin`: 一个实现了 `InputPin` 的GPIO引脚。对于 `embassy`，
    ///   这个引脚类型通常也实现了 `wait_for_high/low` 等异步方法。
    /// * `active_level`: 定义了按钮按下时的有效电平 (`ActiveLevel::Low` 或 `ActiveLevel::High`)。
    pub fn new(pin: P, active_level: ActiveLevel) -> Self {
        Self { pin, active_level }
    }
}

impl<P> AsyncButtonDriver for GpioButton<P>
where
    P: InputPin + Wait,
{
    async fn wait_for_press(&mut self) {
        match self.active_level {
            ActiveLevel::Low => self.pin.wait_for_low().await.unwrap_or_default(),
            ActiveLevel::High => self.pin.wait_for_high().await.unwrap_or_default(),
        }
    }

    async fn wait_for_release(&mut self) {
        match self.active_level {
            ActiveLevel::Low => self.pin.wait_for_high().await.unwrap_or_default(),
            ActiveLevel::High => self.pin.wait_for_low().await.unwrap_or_default(),
        }
    }
}
