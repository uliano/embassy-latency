#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

// 6.43 us latency to get exti response 
// when pulse is generated on an another board
// pulse detector runs on high interrupt executor
// mainloop rest of the stuff

// the presence of a second interrupt executor doesn't affect significantly the response time
// ~ 3 clocks worser


use defmt::*;
use core::mem;
use cortex_m_rt::entry;
use cortex_m::peripheral::NVIC;
use cortex_m::asm::nop as nop;
use embassy_stm32::executor::{Executor, InterruptExecutor};
use embassy_stm32::{interrupt, Config};
use embassy_stm32::pac::Interrupt;
use embassy_stm32::time::mhz;
use embassy_stm32::peripherals::{PA0, PB0, PB1, PB2, PC13};
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Input, Pull, Speed};
use embassy_time::{Duration, Instant, Timer};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_MED: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();

#[interrupt]
unsafe fn USART6() {
    EXECUTOR_MED.on_interrupt()
}

#[interrupt]
unsafe fn USART1() {
    EXECUTOR_HIGH.on_interrupt()
}

#[embassy_executor::task]
async fn button_monitor(mut button: ExtiInput<'static,PA0>) {
    loop {
        button.wait_for_falling_edge().await;
        info!("Button pressed!");
        button.wait_for_rising_edge().await;
        info!("Button released!");
    }
}

#[embassy_executor::task]
async fn pulse_on_exti(mut input: ExtiInput<'static,PB1>,mut output: Output<'static,PB2>) {
    loop {
        input.wait_for_rising_edge().await;
        output.set_high();
        //Timer::after(Duration::from_micros(1)).await;
        nop();
        output.set_low();
    }
}

#[embassy_executor::task]
async fn trigger(mut output: Output<'static,PB0>) {
    loop {
    output.set_high();
    output.set_low();
    Timer::after(Duration::from_millis(10)).await;
    }
}

#[embassy_executor::task]
async fn blinker(mut led: Output<'static,PC13>) {
    loop {
        info!("Blink");
        led.toggle();
        Timer::after(Duration::from_millis(500)).await;
    }
}

#[embassy_executor::task]
async fn heavy() {
    loop {
        let start = Instant::now();
        info!("[heavy] Starting long computation");
        // Spin-wait to simulate a long CPU computation
        cortex_m::asm::delay(300_000_000); 
        let end = Instant::now();
        let ms = end.duration_since(start).as_millis();
        info!("[heavy] done in {} ms", ms);
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[entry]
fn main() -> ! {
    // clocks
    let mut config = Config::default();
    config.rcc.pll48 = true;
    config.rcc.hse = Some(mhz(25));
    config.rcc.hclk = Some(mhz(96));
    config.rcc.sys_ck = Some(mhz(96));
    let p = embassy_stm32::init(config);

    // pins
    let led = Output::new(p.PC13, Level::High, Speed::Low);
    let input = Input::new(p.PB1, Pull::Down);
    let exti_input = ExtiInput::new(input, p.EXTI1);
    let button = Input::new(p.PA0, Pull::Up);
    let exti_button = ExtiInput::new(button, p.EXTI0);
    //let trigger_pin = Output::new(p.PB0, Level::Low, Speed::VeryHigh);
    let exti_output = Output::new(p.PB2, Level::Low, Speed::High);

    let mut nvic: NVIC = unsafe { mem::transmute(()) };
    unsafe { nvic.set_priority(Interrupt::USART1, 6 << 4) };
    let spawner = EXECUTOR_HIGH.start(Interrupt::USART1);
    unwrap!(spawner.spawn(pulse_on_exti(exti_input, exti_output)));

    // Medium-priority executor: USART6, priority level 7
    unsafe { nvic.set_priority(Interrupt::USART6, 7 << 4) };
    //let spawner = EXECUTOR_MED.start(Interrupt::USART6);
    // unwrap!(spawner.spawn(trigger(trigger_pin)));

    // Low priority executor: runs in thread mode, using WFE/SEV
    let executor = EXECUTOR_LOW.init(Executor::new());
    executor.run(|spawner| {
        unwrap!(spawner.spawn(blinker(led)));
        unwrap!(spawner.spawn(button_monitor(exti_button)));
        unwrap!(spawner.spawn(heavy()));
    });
}
