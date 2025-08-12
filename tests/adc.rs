use core::convert::Infallible;
use embassy_async_button::{
    adc::{filter::RawFilter, AdcDriver, AsyncAdc},
    config::ButtonConfig,
    Button, ButtonEvent,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, pubsub::PubSubChannel};
use embassy_time::{Duration, Timer};
use tokio::sync::watch;

// --- Mock Hardware (模拟硬件) ---

// 1. 模拟一个异步 ADC
struct MockAdc {
    // 使用 watch channel 来让测试动态地改变 ADC "读取" 到的值
    receiver: watch::Receiver<u16>,
}

// 2. 实现我们自定义的 AsyncAdc trait
impl AsyncAdc for MockAdc {
    type Error = Infallible;
    // read 方法会等待 watch channel 中的值发生变化
    async fn read(&mut self) -> Result<u16, Self::Error> {
        self.receiver.changed().await.unwrap();
        Ok(*self.receiver.borrow())
    }
}

// --- Test Harness (测试工具) ---

// 1. 电压模拟器
async fn voltage_simulator(sender: watch::Sender<u16>, press_voltage: u16) {
    // 初始状态：空闲电压 (0)
    sender.send(0).unwrap();
    Timer::after(Duration::from_millis(10)).await;

    // --- 模拟单击 ---
    // 1. 发送一个在阈值内的电压，模拟“按下”
    sender.send(press_voltage).unwrap();
    Timer::after(Duration::from_millis(100)).await;

    // 2. 发送一个空闲电压，模拟“释放”
    sender.send(0).unwrap();
    Timer::after(Duration::from_millis(300)).await;
}

// 2. 主测试函数
#[tokio::test]
async fn test_adc_single_click() {
    const PRESS_VOLTAGE: u16 = 1000;
    const THRESHOLD_LOW: u16 = 900;
    const THRESHOLD_HIGH: u16 = 1100;

    // --- 设置模拟硬件和 async-button ---
    // 创建 watch channel 用于模拟 ADC 电压变化
    let (sender, receiver) = watch::channel(0u16);
    let adc = MockAdc { receiver };

    // PubSubChannel 用于在 ADC 组和按钮驱动之间广播滤波后的 ADC 值
    static CHANNEL: PubSubChannel<CriticalSectionRawMutex, u16, 4, 4, 4> = PubSubChannel::new();
    let config = ButtonConfig::default();

    // 1. 创建 ADC 按钮组，使用最简单的 RawFilter
    let  (runner, factory) = AdcDriver::new(adc, RawFilter::default(), &CHANNEL);

    // 2. 从组中创建一个具体的 ADC 按钮实例
    let adc_driver = factory.button(THRESHOLD_LOW, THRESHOLD_HIGH);

    // 3. 将 ADC 按钮驱动包装在通用的 Button 逻辑中
    let mut button = Button::new(adc_driver, config);

    // --- 运行测试 ---
    // 在后台运行 ADC 读取和滤波循环
    let group_task = tokio::spawn(runner.run());

    // 在后台运行电压模拟器
    let simulator_task = tokio::spawn(voltage_simulator(sender, PRESS_VOLTAGE));

    // 在主任务中验证事件
    let event =
        embassy_time::with_timeout(embassy_time::Duration::from_secs(1), button.next_event())
            .await
            .expect("测试超时，未等到 ADC 按钮事件");

    assert_eq!(event, ButtonEvent::Click);

    // 清理任务
    group_task.abort();
    simulator_task.abort();
}
