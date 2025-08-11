use embassy_async_button::{
    config::ButtonConfig,
    gpio::{ActiveLevel, GpioButton},
    Button, ButtonEvent,
};
use core::convert::Infallible;
use embassy_time::{Duration, Timer};
use tokio::sync::watch;

struct MockPin {
    rx: watch::Receiver<bool>,
}
struct MockPinController {
    tx: watch::Sender<bool>,
}
impl MockPin {
    fn split() -> (MockPinController, Self) {
        let (tx, rx) = watch::channel(true);
        (MockPinController { tx }, Self { rx })
    }
}
impl embedded_hal::digital::ErrorType for MockPin {
    type Error = Infallible;
}
impl embedded_hal::digital::InputPin for MockPin {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(*self.rx.borrow())
    }
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!*self.rx.borrow())
    }
}
impl embedded_hal_async::digital::Wait for MockPin {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        self.rx.wait_for(|state| *state).await.unwrap();
        Ok(())
    }
    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        self.rx.wait_for(|state| !*state).await.unwrap();
        Ok(())
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        self.wait_for_low().await?;
        self.wait_for_high().await
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        self.wait_for_high().await?;
        self.wait_for_low().await
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        self.rx.wait_for(|_| true).await.unwrap();
        Ok(())
    }
}

// 1. 事件生成器
async fn event_generator(controller: MockPinController) {
    let config = ButtonConfig::default();

    controller.tx.send(true).unwrap(); // 初始状态
    Timer::after(Duration::from_millis(100)).await;

    // 单击
    controller.tx.send(false).unwrap();
    Timer::after(Duration::from_millis(50)).await;
    controller.tx.send(true).unwrap();
    Timer::after(config.multi_click_window + Duration::from_millis(10)).await;

    // 双击
    controller.tx.send(false).unwrap();
    Timer::after(Duration::from_millis(50)).await;
    controller.tx.send(true).unwrap();
    Timer::after(Duration::from_millis(100)).await;
    controller.tx.send(false).unwrap();
    Timer::after(Duration::from_millis(50)).await;
    controller.tx.send(true).unwrap();
    Timer::after(config.multi_click_window + Duration::from_millis(10)).await;

    // 长按
    controller.tx.send(false).unwrap();
    Timer::after(config.long_press_time + Duration::from_millis(10)).await;
    Timer::after(config.long_press_hold_interval + Duration::from_millis(20)).await;
    controller.tx.send(true).unwrap();
}

// 2. 事件验证器
async fn event_validator(mut button: Button<GpioButton<MockPin>>) {
    let expected_events = [
        ButtonEvent::Click,
        ButtonEvent::DoubleClick,
        ButtonEvent::LongPressStart,
        ButtonEvent::LongPressHold,
        ButtonEvent::LongPressRelease,
    ];

    for expected in expected_events {
        let event =
            embassy_time::with_timeout(embassy_time::Duration::from_secs(1), button.next_event())
                .await
                .expect("Test timed out waiting for an event");
        assert_eq!(event, expected);
    }
}

// --- 主测试函数 ---

#[tokio::test]
async fn test_gpio_events_with_tokio_and_embassy_time() {
    let (controller, driver_pin) = MockPin::split();
    let config = ButtonConfig::default();

    // 我们的 Button 库继续使用它所期望的 embassy-time
    let gpio_driver = GpioButton::new(driver_pin, ActiveLevel::Low);
    let button = Button::new(gpio_driver, config);

    // 并发运行生成器和验证器
    tokio::join!(event_generator(controller), event_validator(button));
}


// 1. 三次点击事件生成器
async fn triple_click_event_generator(controller: MockPinController) {
    let config = ButtonConfig::default();

    // 初始状态为高电平 (未按下)
    controller.tx.send(true).unwrap();
    Timer::after(Duration::from_millis(100)).await;

    // 模拟三次连续点击
    for _ in 0..3 {
        controller.tx.send(false).unwrap(); // 按下
        Timer::after(Duration::from_millis(50)).await;
        controller.tx.send(true).unwrap();  // 释放
        Timer::after(Duration::from_millis(100)).await; // 点击间隔
    }

    // 等待超过多击窗口，以让库最终确定点击次数
    Timer::after(config.multi_click_window + Duration::from_millis(10)).await;
}

// 2. 三次点击事件验证器
async fn triple_click_event_validator(mut button: Button<GpioButton<MockPin>>) {
    let expected_event = ButtonEvent::MultipleClick { count: 3 };

    // 使用 with_timeout 确保测试不会无限等待
    let event =
        embassy_time::with_timeout(embassy_time::Duration::from_secs(2), button.next_event())
            .await
            .expect("Test timed out waiting for a triple-click event");

    assert_eq!(event, expected_event);
}

// 3. 主测试函数
#[tokio::test]
async fn test_gpio_triple_click_event() {
    let (controller, driver_pin) = MockPin::split();
    let config = ButtonConfig::default();

    let gpio_driver = GpioButton::new(driver_pin, ActiveLevel::Low);
    let button = Button::new(gpio_driver, config);

    // 并发运行三次点击的生成器和验证器
    tokio::join!(
        triple_click_event_generator(controller),
        triple_click_event_validator(button)
    );
}