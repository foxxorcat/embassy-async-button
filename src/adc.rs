use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Subscriber},
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

/// ADC按钮组，其职责是纯粹地运行ADC采样和滤波任务。
pub struct AdcButtonGroup<
    'a,
    ADC: AsyncAdc,
    F: AdcFilter,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    adc: ADC,
    filter: F,
    channel: &'a AdcChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<
        'a,
        ADC: AsyncAdc,
        F: AdcFilter,
        const MSG_CAP: usize,
        const SUBS: usize,
        const SUBSCRIBER_CAP: usize,
    > AdcButtonGroup<'a, ADC, F, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    pub fn new(
        adc: ADC,
        filter: F,
        channel: &'a AdcChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    ) -> Self {
        Self {
            adc,
            filter,
            channel,
        }
    }

    /// 工厂方法：创建一个与此组关联的SimpleAdcButton实例。
    pub fn simple_button(
        &self,
        threshold_low: u16,
        threshold_high: u16,
    ) -> SimpleAdcButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP> {
        SimpleAdcButton::new(
            self.channel.subscriber().unwrap(),
            threshold_low,
            threshold_high,
        )
    }

    /// 运行ADC读取和滤波循环。
    pub async fn run(mut self) -> ! {
        let publisher = self.channel.publisher().unwrap();
        loop {
            if let Ok(raw_value) = self.adc.read().await {
                if let Some(filtered_value) = self.filter.process(raw_value) {
                    // 只有当滤波器产出有效值时，才发布
                    publisher.publish(filtered_value).await;
                }
            }
            // 采样周期的延迟由滤波器内部的 inter_sample_delay 控制
            self.filter.inter_sample_delay().await;
        }
    }
}

/// 一个简单的、基于电压范围的ADC按钮，消费已被上游完全处理好的数据。
pub struct SimpleAdcButton<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize>
{
    subscriber: AdcSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    threshold_low: u16,
    threshold_high: u16,
}

impl<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize>
    SimpleAdcButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    pub fn new(
        subscriber: AdcSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
        threshold_low: u16,
        threshold_high: u16,
    ) -> Self {
        Self {
            subscriber,
            threshold_low,
            threshold_high,
        }
    }
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
