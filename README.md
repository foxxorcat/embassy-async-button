# async-button: 异步按钮处理库

一个为 `no_std` 嵌入式环境设计的、现代化的异步按钮处理库。它基于 `embedded-hal 1.0` 的异步 trait 和 `embassy-time` 构建。

本库旨在以最小的资源开销，提供强大而灵活的按钮事件处理能力。

---

## 主要特性

- ✅ **完全异步**: 基于 `async/await`，绝不阻塞其他任务的执行。
- ✅ **多种驱动支持**:
  - **GPIO**: 支持单个由 GPIO 驱动的简单按键。
  - **矩阵键盘**: 内置行、列扫描逻辑，高效处理矩阵键盘。
  - **ADC 键盘**: 支持由单个ADC引脚驱动的、通过不同电阻分压的模拟键盘，并能精确解码**组合按键**。
- ✅ **高级事件检测**:
  - 内置的消抖处理。
  - 可靠地检测单击、双击和多次连击。
  - 可靠地检测长按、长按保持和长按释放。
- ✅ **高度可配置**: 可通过 `ButtonConfig` 精确调整消抖时间、多击间隔、长按阈值等参数。
- ✅ **可组合架构**: 驱动层与逻辑层分离，您可以轻松地将任何实现了 `AsyncButtonDriver` trait 的硬件适配器包装进 `Button` 中，以复用所有高级事件检测逻辑。

---

## 使用示例

### 示例 1: 简单的 GPIO 按键

这是最基础的用法，用于处理一个连接到 GPIO 引脚的独立按键。

```rust,ignore
use async_button::{gpio::{GpioButton, ActiveLevel}, Button, ButtonConfig, ButtonEvent};
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Pull};

#[embassy_executor::task]
async fn button_task(pin: Input<'static, GpioPin>) {
    // 1. 创建一个 GPIO 驱动实例
    let driver = GpioButton::new(pin, ActiveLevel::Low); // 低电平有效（内部上拉）

    // 2. 将驱动包装进 Button 逻辑中
    let mut button = Button::new(driver, ButtonConfig::default());

    // 3. 在循环中等待并处理事件
    loop {
        match button.next_event().await {
            ButtonEvent::Click => println!("按钮被单击!"),
            ButtonEvent::DoubleClick => println!("按钮被双击!"),
            ButtonEvent::LongPressStart => println!("长按开始..."),
            _ => {}
        }
    }
}
```

### 示例 2: 高级 ADC 组合按键

这个例子展示了本库最强大的功能：处理由单个ADC引脚驱动的、具有多个物理按键的模拟键盘。

```rust,ignore
use async_button::{
    adc::{
        filter::RawFilter,
        adc_keypad::{KeyDecoder, KeypadDriverGroup, KeymaskChannel},
    },
    config::ButtonConfig,
    Button, ButtonEvent,
};
use embassy_adc::Adc;

// 1. 根据您的硬件电路，实现 KeyDecoder trait
//    它负责将一个ADC电压值，解码成一个代表当前所有被按下按键的位掩码。
struct MyKeypadDecoder;
impl KeyDecoder for MyKeypadDecoder {
    fn decode(&self, value: u16) -> u32 {
        match value {
            // 电压范围 -> 按键位掩码
            900..=1100 => 1 << 0,  // 按键 0
            1900..=2100 => 1 << 1, // 按键 1
            2900..=3100 => (1 << 0) | (1 << 1), // 按键 0 和 1 组合
            _ => 0, // 无按键
        }
    }
}

#[embassy_executor::task]
async fn keypad_task(adc: Adc<'static, embassy_rp::peripherals::ADC>) {
    // 2. 创建一个专用的通道，用于发布解码后的位掩码
    static KEYMASK_CHANNEL: KeymaskChannel<4, 4, 4> = KeymaskChannel::new();

    // 3. 创建并运行第一层：KeypadDriverGroup
    //    它在后台持续运行，将ADC值解码后发布出去。
    let keypad_group = KeypadDriverGroup::new(
        adc,
        RawFilter::default(),
        MyKeypadDecoder,
        &KEYMASK_CHANNEL,
    );
    spawner.spawn(keypad_group_task(keypad_group)).unwrap();

    // 4. 为您关心的每一个按键创建独立的 Button 实例
    let mut button_0 = Button::new(keypad_group.button(0), ButtonConfig::default());
    let mut button_1 = Button::new(keypad_group.button(1), ButtonConfig::default());

    // 5. 在主逻辑中分别处理每个按键的高级事件
    loop {
        // ... 使用 select! 同时等待 button_0 和 button_1 的事件 ...
    }
}

#[embassy_executor::task]
async fn keypad_group_task(group: KeypadDriverGroup<'static, ...>) {
    group.run().await;
}
```

---

## Cargo Features

- `defmt`: 为所有公共类型派生 `defmt::Format`，便于在嵌入式系统上进行日志记录。

---

## 许可证

本项目采用双重许可，您可以任选其一：

-   Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <http://www.apache.org/licenses/LICENSE-2.0>)
-   MIT license ([LICENSE-MIT](LICENSE-MIT) 或 <http://opensource.org/licenses/MIT>)
