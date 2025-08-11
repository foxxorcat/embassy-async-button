use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Subscriber},
};

use crate::{
    adc::{AdcFilter, AsyncAdc},
    AsyncButtonDriver,
};

pub trait KeyDecoder {
    fn decode(&self, value: u16) -> u32;
}

// 为位掩码通道创建类型别名
pub type KeymaskChannel<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    PubSubChannel<CriticalSectionRawMutex, u32, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
pub type KeymaskSubscriber<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> = Subscriber<'a, CriticalSectionRawMutex, u32, MSG_CAP, SUBS, SUBSCRIBER_CAP>;

pub struct KeypadDriverGroup<
    'a,
    ADC: AsyncAdc,
    F: AdcFilter,
    D: KeyDecoder,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    adc: ADC,
    filter: F,
    decoder: D,
    mask_channel: &'a KeymaskChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<
        'a,
        ADC: AsyncAdc,
        F: AdcFilter,
        D: KeyDecoder,
        const MSG_CAP: usize,
        const SUBS: usize,
        const SUBSCRIBER_CAP: usize,
    > KeypadDriverGroup<'a, ADC, F, D, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    pub fn new(
        adc: ADC,
        filter: F,
        decoder: D,
        mask_channel: &'a KeymaskChannel<MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    ) -> Self {
        Self {
            adc,
            filter,
            decoder,
            mask_channel,
        }
    }

    pub fn button(&self, key_id: u8) -> KeypadButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP> {
        assert!(key_id < 32, "Key ID must be less than 32");
        KeypadButton {
            keymask_sub: self.mask_channel.subscriber().unwrap(),
            key_mask: 1 << key_id,
            last_known_mask: 0,
        }
    }

    /// 运行解码循环。
    pub async fn run(mut self) -> ! {
        let mut last_mask = u32::MAX;
        let publisher = self.mask_channel.publisher().unwrap();
        loop {
            let value = loop {
                if let Ok(raw_value) = self.adc.read().await {
                    if let Some(filtered_value) = self.filter.process(raw_value) {
                        break filtered_value;
                    }
                }
                self.filter.inter_sample_delay().await;
            };

            let current_mask = self.decoder.decode(value);
            if current_mask != last_mask {
                publisher.publish(current_mask).await;
                last_mask = current_mask;
            }
        }
    }
}

pub struct KeypadButton<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> {
    keymask_sub: KeymaskSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    key_mask: u32,        // 例如: 1 << 5, 代表我们关心第5个按键
    last_known_mask: u32, // 用于处理驱动启动时按键就已按下的情况
}

impl<const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> AsyncButtonDriver
    for KeypadButton<'_, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    async fn wait_for_press(&mut self) {
        // 如果我们已知的最新状态就是“按下”，则立即返回
        if (self.last_known_mask & self.key_mask) != 0 {
            return;
        }

        loop {
            let new_mask = self.keymask_sub.next_message_pure().await;
            self.last_known_mask = new_mask;
            if (new_mask & self.key_mask) != 0 {
                return;
            }
        }
    }

    async fn wait_for_release(&mut self) {
        // 如果我们已知的最新状态就是“释放”，则立即返回
        if (self.last_known_mask & self.key_mask) == 0 {
            return;
        }

        loop {
            let new_mask = self.keymask_sub.next_message_pure().await;
            self.last_known_mask = new_mask;
            if (new_mask & self.key_mask) == 0 {
                return;
            }
        }
    }
}
