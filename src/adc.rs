use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Publisher, Subscriber},
};

use crate::AsyncButtonDriver;

/// 本地定义的异步ADC读取trait。
pub trait AsyncAdc {
    type Error;
    async fn read(&mut self) -> Result<u16, Self::Error>;
}

/// ADC采样滤波器的trait。
/// 现在它在内部管理样本，并控制采样间的延迟。
pub trait AdcFilter {
    /// 处理一个新的采样值。
    /// 如果滤波器已准备好输出一个有效值，则返回 `Some(value)`。
    /// 否则返回 `None`，表示需要更多样本。
    fn process(&mut self, new_sample: u16) -> Option<u16>;

    /// 定义两次连续采样之间所需的异步延迟。
    /// 这允许滤波器根据其算法需求（例如等待ADC稳定）来控制采样时序。
    async fn inter_sample_delay(&self);
}

pub mod filter {
    use super::*;
    use embassy_time::{Duration, Timer};

    /// 原始值滤波器，立即返回结果，无额外延迟。
    #[derive(Default)]
    pub struct RawFilter;

    impl AdcFilter for RawFilter {
        fn process(&mut self, new_sample: u16) -> Option<u16> {
            Some(new_sample)
        }

        async fn inter_sample_delay(&self) {
            // RawFilter不需要额外延迟
        }
    }

    /// 中位值滤波器，在采集足够样本后输出中间值。
    pub struct MedianFilter<const N: usize> {
        samples: [u16; N],
        index: usize,
    }

    impl<const N: usize> MedianFilter<N> {
        pub fn new() -> Self {
            assert!(N > 0, "MedianFilter requires at least 1 sample");
            Self {
                samples: [0; N],
                index: 0,
            }
        }
    }

    impl<const N: usize> AdcFilter for MedianFilter<N> {
        fn process(&mut self, new_sample: u16) -> Option<u16> {
            self.samples[self.index] = new_sample;
            self.index += 1;

            if self.index < N {
                // 样本还未采满
                return None;
            }

            // 样本已满，计算中位值并重置索引
            self.index = 0;
            self.samples.sort_unstable();
            Some(self.samples[N / 2])
        }

        async fn inter_sample_delay(&self) {
            // 假设两次采样间需要一个短暂的稳定延迟
            Timer::after(Duration::from_micros(100)).await;
        }
    }
}

pub type AdcChannel<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    PubSubChannel<CriticalSectionRawMutex, u16, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
pub type AdcSubscriber<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    Subscriber<'a, CriticalSectionRawMutex, u16, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
pub type AdcPublisher<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    Publisher<'a, CriticalSectionRawMutex, u16, MSG_CAP, SUBS, SUBSCRIBER_CAP>;

/// 【按键工厂】用于创建简单的ADC按钮，可被克隆并在程序各处使用。
#[derive(Clone)]
pub struct SimpleAdcButtonFactory<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    channel: &'a AdcChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize>
    SimpleAdcButtonFactory<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    /// 创建一个基于阈值的简单ADC按钮。
    pub fn button(
        &self,
        threshold_low: u16,
        threshold_high: u16,
    ) -> SimpleAdcButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP> {
        SimpleAdcButton {
            subscriber: self.channel.subscriber().unwrap(),
            threshold_low,
            threshold_high,
        }
    }
}

/// 【后台驱动器】拥有硬件资源，并提供 run 方法以在后台任务中运行。
pub struct AdcDriver<
    'a,
    ADC: AsyncAdc,
    F: AdcFilter,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    adc: ADC,
    filter: F,
    publisher: AdcPublisher<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<
        'a,
        ADC: AsyncAdc,
        F: AdcFilter,
        const MSG_CAP: usize,
        const SUBS: usize,
        const SUBSCRIBER_CAP: usize,
    > AdcDriver<'a, ADC, F, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    /// 创建一个新的ADC驱动及其关联的按键工厂。
    ///
    /// 这是设置简单ADC按钮的唯一入口点。
    ///
    /// # 返回
    /// 一个元组，包含:
    /// - `AdcDriver`: 需要被 spawn 到后台任务中运行。
    /// - `SimpleAdcButtonFactory`: 用于在程序中创建具体的按键实例。
    pub fn new(
        adc: ADC,
        filter: F,
        channel: &'a AdcChannel<MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    ) -> (Self, SimpleAdcButtonFactory<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>) {
        let driver = Self {
            adc,
            filter,
            publisher: channel.publisher().unwrap(),
        };
        let factory = SimpleAdcButtonFactory { channel };
        (driver, factory)
    }

    pub async fn run(mut self) -> ! {
        loop {
            if let Ok(raw_value) = self.adc.read().await {
                if let Some(filtered_value) = self.filter.process(raw_value) {
                    self.publisher.publish(filtered_value).await;
                }
            }
            self.filter.inter_sample_delay().await;
        }
    }
}

pub struct SimpleAdcButton<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize>
{
    subscriber: AdcSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    threshold_low: u16,
    threshold_high: u16,
}

impl<const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> AsyncButtonDriver
    for SimpleAdcButton<'_, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    async fn wait_for_press(&mut self) {
        loop {
            let value = self.subscriber.next_message_pure().await;
            if value >= self.threshold_low && value <= self.threshold_high {
                return;
            }
        }
    }
    async fn wait_for_release(&mut self) {
        loop {
            let value = self.subscriber.next_message_pure().await;
            if value < self.threshold_low || value > self.threshold_high {
                return;
            }
        }
    }
}