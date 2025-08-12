use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Publisher, Subscriber},
};

use crate::{
    adc::{AdcFilter, AsyncAdc},
    AsyncButtonDriver,
};

pub trait KeyDecoder {
    fn decode(&self, value: u16) -> u32;
}

pub type KeymaskChannel<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    PubSubChannel<CriticalSectionRawMutex, u32, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
type KeymaskPublisher<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> =
    Publisher<'a, CriticalSectionRawMutex, u32, MSG_CAP, SUBS, SUBSCRIBER_CAP>;
pub type KeymaskSubscriber<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> = Subscriber<'a, CriticalSectionRawMutex, u32, MSG_CAP, SUBS, SUBSCRIBER_CAP>;


#[derive(Clone)]
pub struct KeypadButtonFactory<
    'a,
    const MSG_CAP: usize,
    const SUBS: usize,
    const SUBSCRIBER_CAP: usize,
> {
    mask_channel: &'a KeymaskChannel<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize>
    KeypadButtonFactory<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    /// 在任何时候创建一个新的按键驱动实例。
    pub fn button(&self, key_id: u8) -> KeypadButton<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP> {
        assert!(key_id < 32, "Key ID must be less than 32");
        KeypadButton {
            keymask_sub: self.mask_channel.subscriber().unwrap(),
            key_mask: 1 << key_id,
            last_known_mask: 0,
        }
    }
}

pub struct KeypadDriver<
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
    mask_pub: KeymaskPublisher<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
}

impl<
        'a,
        ADC: AsyncAdc,
        F: AdcFilter,
        D: KeyDecoder,
        const MSG_CAP: usize,
        const SUBS: usize,
        const SUBSCRIBER_CAP: usize,
    > KeypadDriver<'a, ADC, F, D, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    /// 创建一个新的键盘驱动及其关联的按键工厂。
    ///
    /// 这是设置键盘的唯一入口点。
    ///
    /// # 返回
    /// 一个元组，包含:
    /// - `KeypadDriver`: 需要被 spawn 到后台任务中运行。
    /// - `KeypadButtonFactory`: 用于在程序中创建具体的按键实例。
    pub fn new(
        adc: ADC,
        filter: F,
        decoder: D,
        mask_channel: &'a KeymaskChannel<MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    ) -> (Self, KeypadButtonFactory<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>) {
        let driver = Self {
            adc,
            filter,
            decoder,
            mask_pub: mask_channel.publisher().unwrap(),
        };
        let factory = KeypadButtonFactory { mask_channel };
        (driver, factory)
    }

    /// 运行解码循环。这是您需要 spawn 到后台的唯一任务。
    pub async fn run(mut self) -> ! {
        let mut last_mask = u32::MAX;
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
                self.mask_pub.publish(current_mask).await;
                last_mask = current_mask;
            }
        }
    }
}

pub struct KeypadButton<'a, const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> {
    keymask_sub: KeymaskSubscriber<'a, MSG_CAP, SUBS, SUBSCRIBER_CAP>,
    key_mask: u32,
    last_known_mask: u32,
}

impl<const MSG_CAP: usize, const SUBS: usize, const SUBSCRIBER_CAP: usize> AsyncButtonDriver
    for KeypadButton<'_, MSG_CAP, SUBS, SUBSCRIBER_CAP>
{
    async fn wait_for_press(&mut self) {
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
