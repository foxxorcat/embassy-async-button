use core::convert::Infallible;
use embassy_async_button::{
    config::ButtonConfig,
    matrix::{KeyEvent, MatrixDriver},
    Button, ButtonEvent,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, pubsub::PubSubChannel};
use embassy_time::{Duration, Timer};
use std::sync::{Arc, Mutex}; // 【新】引入 Arc 和 std::sync::Mutex
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

// --- Mock Hardware (模拟硬件) ---

// 用于模拟输出引脚（列）
struct MockOutputPin {
    col_index: u8,
    sender: UnboundedSender<u8>,
}

impl embedded_hal::digital::ErrorType for MockOutputPin {
    type Error = Infallible;
}

impl embedded_hal::digital::OutputPin for MockOutputPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        let _ = self.sender.send(self.col_index);
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Clone)]
struct MockInputPin {
    state: Arc<Mutex<bool>>,
}

impl embedded_hal::digital::ErrorType for MockInputPin {
    type Error = Infallible;
}

impl embedded_hal::digital::InputPin for MockInputPin {
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        // 锁定互斥锁并读取共享状态
        let is_pressed = *self.state.lock().unwrap();
        Ok(is_pressed)
    }

    fn is_high(&mut self) -> Result<bool, Self::Error> {
        let is_pressed = *self.state.lock().unwrap();
        Ok(!is_pressed)
    }
}

// --- Test Harness (测试工具) ---

// 1. 事件生成器/模拟器
async fn matrix_simulator(
    mut col_receiver: UnboundedReceiver<u8>,
    rows_state: [Arc<Mutex<bool>>; 2],
    key_to_press: (u8, u8),
) {
    // 初始状态：所有按键都未按下
    for pin_state in rows_state.iter() {
        *pin_state.lock().unwrap() = false;
    }

    // 等待扫描循环激活我们想要按下的那一列
    while let Some(active_col) = col_receiver.recv().await {
        if active_col == key_to_press.1 {
            // 就是这一列！现在模拟“按下”状态
            *rows_state[key_to_press.0 as usize].lock().unwrap() = true;
            break;
        }
    }
    // 保持按下状态一小段时间
    Timer::after(Duration::from_millis(50)).await;

    // 模拟“释放”
    *rows_state[key_to_press.0 as usize].lock().unwrap() = false;
}

// 2. 主测试函数（同时也是验证器）
#[tokio::test]
async fn test_matrix_single_click() {
    const ROWS: usize = 2;
    const COLS: usize = 2;
    const KEY_TO_TEST: (u8, u8) = (1, 1);

    let (col_sender, col_receiver) = mpsc::unbounded_channel();

    // 1. 创建共享状态的数组
    let rows_state: [Arc<Mutex<bool>>; ROWS] = std::array::from_fn(|_| Arc::new(Mutex::new(false)));

    // 2. 创建 Mock 引脚，它们都引用共享状态
    let cols: [MockOutputPin; COLS] = [
        MockOutputPin {
            col_index: 0,
            sender: col_sender.clone(),
        },
        MockOutputPin {
            col_index: 1,
            sender: col_sender.clone(),
        },
    ];
    let rows: [MockInputPin; ROWS] = [
        MockInputPin {
            state: rows_state[0].clone(),
        },
        MockInputPin {
            state: rows_state[1].clone(),
        },
    ];

    // --- 设置 async-button ---
    static CHANNEL: PubSubChannel<CriticalSectionRawMutex, KeyEvent, 4, 4, 4> =
        PubSubChannel::new();
    let config = ButtonConfig::default();

    // 1. 创建矩阵键盘组
    let (runner, factory) = MatrixDriver::new(cols, rows, &CHANNEL);

    // 2. 从组中创建一个具体的按钮实例
    let matrix_driver = factory.button(KEY_TO_TEST.0, KEY_TO_TEST.1);

    // 3. 将矩阵按钮驱动包装在通用的 Button 逻辑中
    let mut button = Button::new(matrix_driver, config);

    // --- 运行测试 ---
    let group_task = tokio::spawn(runner.run());
    let simulator_task = tokio::spawn(matrix_simulator(col_receiver, rows_state, KEY_TO_TEST));

    // 在主任务中验证事件
    let event =
        embassy_time::with_timeout(embassy_time::Duration::from_secs(1), button.next_event())
            .await
            .expect("测试超时，未等到矩阵按钮事件");

    assert_eq!(event, ButtonEvent::Click);

    // 清理任务
    group_task.abort();
    simulator_task.abort();
}
