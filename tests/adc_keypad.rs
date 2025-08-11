use embassy_async_button::{
    adc::{filter::RawFilter, AsyncAdc},
    adc_keypad::{KeyDecoder, KeymaskChannel, KeypadDriverGroup},
    config::ButtonConfig,
    Button, ButtonEvent,
};
use core::convert::Infallible;
use tokio::sync::watch;

// --- 模拟硬件与解码器 ---

/// 模拟ADC，使用 `watch` channel 来模拟电压值的变化。
struct MockAdc {
    receiver: watch::Receiver<u16>,
}

impl AsyncAdc for MockAdc {
    type Error = Infallible;
    async fn read(&mut self) -> Result<u16, Self::Error> {
        // 等待 `watch` channel 中的值发生变化，以此模拟一个阻塞式的ADC读取操作。
        // 如果发送方被丢弃（测试结束时），`changed()` 会返回一个错误。
        if self.receiver.changed().await.is_err() {
            // 在发送端关闭后，返回一个不可能的电压值，以确保测试的健壮性。
            return Ok(u16::MAX);
        }
        // 返回当前最新的电压值。
        Ok(*self.receiver.borrow())
    }
}

/// 为我们的特定测试电路实现的解码器。
struct MyTestKeypadDecoder;

impl KeyDecoder for MyTestKeypadDecoder {
    fn decode(&self, value: u16) -> u32 {
        match value {
            // 900-1100 的电压范围对应于按键0 (bit 0)。
            900..=1100 => 1 << 0,
            // 1900-2100 的电压范围对应于按键1 (bit 1)。
            1900..=2100 => 1 << 1,
            // 其他所有电压值都代表没有按键按下。
            _ => 0,
        }
    }
}

#[tokio::test]
async fn test_keypad_button_integration() {
    // --- 准备阶段 ---

    // 1. 创建 mock ADC，使用 watch channel 来发送和接收模拟的电压值。
    let (voltage_sender, adc_receiver) = watch::channel(0u16);
    let adc = MockAdc {
        receiver: adc_receiver,
    };

    // 2. 创建一个专用的 PubSub 通道，用于发布解码后的按键位掩码。
    static KEYMASK_CHANNEL: KeymaskChannel<4, 4, 4> = KeymaskChannel::new();

    // 3. 实例化第一层：`KeypadDriverGroup`，负责从ADC读取并发布位掩码。
    let keypad_group = KeypadDriverGroup::new(
        adc,
        RawFilter::default(), // 使用最简单的原始值滤波器
        MyTestKeypadDecoder,
        &KEYMASK_CHANNEL,
    );

    // 4. 从 group 中为 `key ID 1` 创建一个第二层实例：`KeypadButton`。
    let keypad_button_driver = keypad_group.button(1);

    // 5. 将第二层驱动包装进最终的 `Button` 逻辑中，以获得高级事件处理能力。
    let mut button = Button::new(keypad_button_driver, ButtonConfig::default());

    // --- 执行阶段 ---

    // 在后台tokio任务中运行 `KeypadDriverGroup` 的主循环。
    let group_task = tokio::spawn(keypad_group.run());

    // 在另一个后台任务中模拟物理电压的变化，模拟对“按键1”的一次完整点击。
    let simulator_task = tokio::spawn(async move {
        // 初始状态：空闲电压
        voltage_sender.send(0).unwrap();
        embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;

        // "按下"按键：发送一个在按键1范围内的电压值。
        voltage_sender.send(2000).unwrap();
        embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;

        // "释放"按键：恢复到空闲电压。
        voltage_sender.send(0).unwrap();
    });

    // --- 验证阶段 ---

    // 等待 `Button` 逻辑处理完“按下”和“释放”的完整序列。
    // 我们期望最终能从 `next_event()` 得到一个 `Click` 事件。
    let event =
        embassy_time::with_timeout(embassy_time::Duration::from_secs(1), button.next_event())
            .await
            .expect("测试超时，未等到 Click 事件");

    // 断言收到的高级事件确实是 `Click`。
    assert_eq!(event, ButtonEvent::Click);

    // --- 清理阶段 ---
    // 终止后台任务，保持测试环境的干净。
    group_task.abort();
    simulator_task.await.unwrap();
}
