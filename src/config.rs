use embassy_time::Duration;

/// 定义按钮事件逻辑的通用配置参数。
/// 
/// 这个结构体允许用户精细调整各种时间相关的行为，
/// 例如消抖、双击间隔和长按检测时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ButtonConfig {
    /// 消抖持续时间。
    /// 
    /// 在检测到第一次电平变化后，此时间段内的任何后续变化都将被忽略，
    /// 以防止物理按键的机械抖动产生误报。
    pub debounce: Duration,

    /// 双击和多次点击的时间窗口。
    /// 
    /// 一次点击事件发生后，库会在此时间窗口内等待下一次点击。
    /// 如果在窗口内发生，则会被计为双击或多次点击。
    pub multi_click_window: Duration,

    /// 触发长按事件所需的持续时间。
    /// 
    /// 按键需要持续按下超过这个时长，才会触发 `LongPressStart` 事件。
    pub long_press_time: Duration,
    
    /// 在长按状态下，重复触发 `LongPressHold` 事件的周期。
    pub long_press_hold_interval: Duration,
}

impl Default for ButtonConfig {
    /// 提供一套合理的默认配置。
    /// 
    /// - 消抖: 20ms
    /// - 多击窗口: 250ms
    /// - 长按阈值: 500ms
    /// - 长按连发周期: 100ms
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(20),
            multi_click_window: Duration::from_millis(250),
            long_press_time: Duration::from_millis(500),
            long_press_hold_interval: Duration::from_millis(100),
        }
    }
}