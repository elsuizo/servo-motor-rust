// #![deny(warnings)]
// #![deny(unsafe_code)]
#![no_main]
#![no_std]
#[allow(unsafe_code)] // esto es por una funcion de las interrupciones

//-------------------------------------------------------------------------
//                        external modules
//-------------------------------------------------------------------------
mod logger;
//-------------------------------------------------------------------------
//                        crates imports
//-------------------------------------------------------------------------
extern crate panic_semihosting;
// use stm32f1xx_hal::{delay, gpio, i2c, spi, stm32, timer};
// use cortex_m_semihosting::hprintln;
use rtfm::{app, Instant, Duration};
use stm32f1xx_hal::prelude::*;
use stm32f1xx_hal::{gpio};
use stm32f1xx_hal::stm32::Interrupt;
use stm32f1xx_hal::serial::Serial;
use stm32f1xx_hal::qei::Qei;
use stm32f1xx_hal::pac::TIM4;
use heapless::{String, consts::*};
use core::fmt::Write;
//-------------------------------------------------------------------------
//                        types alias
//-------------------------------------------------------------------------
type LED = gpio::gpioc::PC13<gpio::Output<gpio::PushPull>>;
type BUZZER = gpio::gpioc::PC15<gpio::Output<gpio::PushPull>>;
type PINS = (gpio::gpiob::PB6<gpio::Input<gpio::Floating>>, gpio::gpiob::PB7<gpio::Input<gpio::Floating>>);

//-------------------------------------------------------------------------
//                        app
//-------------------------------------------------------------------------

const PERIOD_LOGGER: u32 = 8_000_000;
const PERIOD_MEASURE: u32 = 8_000_000;

#[app(device = stm32f1xx_hal::stm32)]
const APP: () = {
    // recursos que vamos a utilizar
    // static mut BUZZER: BUZZER = ();
    static mut LED: LED = ();
    static mut POSITION: u16 = ();
    static mut FREQ: u32 = ();
    static mut EXTI: stm32f1xx_hal::device::EXTI = ();
    static mut LOGGER: logger::Logger = ();
    static mut ENCODER: stm32f1xx_hal::qei::Qei<TIM4, PINS> = ();
    static mut CALCULATED: bool = ();
    static mut TIME_BEFORE: Instant = ();
    static mut TIME_NOW: Instant = ();
    // static mut SLEEP: u32        = ();

    #[init(schedule = [periodic_logger])]
    fn init() -> init::LateResources {
        //-------------------------------------------------------------------------
        //                   interrupt initialization
        //-------------------------------------------------------------------------
        // NOTE(elsuizo:2019-05-09): este device no hace falta ya que inicializa en app
        let device: stm32f1xx_hal::stm32::Peripherals = device;
        // let mut _flash = device.FLASH.constrain();
        // // Enable the alternate function I/O clock (for external interrupts)
        device.RCC.apb2enr.write(|w| w.afioen().enabled());
        let exti = device.EXTI;
        // Set PB11 to input with pull up resistor
        let mut rcc = device.RCC.constrain();
        let mut afio = device.AFIO.constrain(&mut rcc.apb2);
        let mut gpiob = device.GPIOB.split(&mut rcc.apb2);
        // configure PB0 with the index pulse interrupt
        gpiob.pb0.into_pull_up_input(&mut gpiob.crl);

        // configure interrupts, PB0 to EXTI0
        // enable interrupt mask for line 0
        exti.imr.write(|w| {
            w.mr0().set_bit()
        });

        // set to falling edge triggering
        exti.ftsr.write(|w| {
            w.tr0().set_bit()
        });

        // set exti0 and 1 to gpio bank B
        // TODO: submit patch to stm32f1 crate to make this call safe
        afio.exticr1.exticr1().write(|w| unsafe {
            w.exti0().bits(1)
        });

        let mut gpioc = device.GPIOC.split(&mut rcc.apb2);
        let mut gpioa = device.GPIOA.split(&mut rcc.apb2);

        let mut led = gpioc.pc13.into_push_pull_output(&mut gpioc.crh);
        // let mut buzzer_output = gpioc.pc15.into_push_pull_output(&mut gpioc.crh);
        led.set_low();
        // buzzer_output.set_low();
        let position: u16 = 0;
        let c1 = gpiob.pb6;
        let c2 = gpiob.pb7;
        // Quadrature Encoder Interface
        let qei = Qei::tim4(device.TIM4, (c1, c2), &mut afio.mapr, &mut rcc.apb1);
        // NOTE(elsuizo:2019-05-09): con este timer vamos a capturar el tiempo entre dos
        // interrupciones para poder calcular las rpm
        // let tim3 = Timer::tim3(device.TIM3, 1.hz(), clocks, &mut rcc.apb1);

        // USART1
        let mut flash = device.FLASH.constrain();
        let clocks = rcc.cfgr.freeze(&mut flash.acr);
        let tx = gpioa.pa9.into_alternate_push_pull(&mut gpioa.crh);
        let rx = gpioa.pa10;
        let serial = Serial::usart1(
            device.USART1,
            (tx, rx),
            &mut afio.mapr,
            9_600.bps(),
            clocks,
            &mut rcc.apb2,
        );
        let tx = serial.split().0;

        // NOTE(elsuizo:2019-03-14): necesitamos pasarle tx a write_message()
        // let tx = serial.split().0;
        let logger = logger::Logger::new(tx);
        let calculated: bool = false;
        let time_before: Instant = Instant::artificial(0);
        let time_now: Instant = Instant::artificial(0);
        let freq: u32 = 0;
        //-------------------------------------------------------------------------
        //                        resources
        //-------------------------------------------------------------------------
        schedule.periodic_logger(Instant::now() + PERIOD_LOGGER.cycles()).unwrap();
        init::LateResources {
            LED: led,
            // BUZZER: buzzer_output,
            POSITION: position,
            FREQ: freq,
            EXTI: exti,
            LOGGER: logger,
            ENCODER: qei,
            CALCULATED: calculated,
            TIME_NOW: time_now,
            TIME_BEFORE: time_before,
        }

    }

    #[task(schedule = [periodic_logger], resources = [LOGGER, ENCODER, POSITION, FREQ])]
    fn periodic_logger() {

        // toggle for debug
        // resources.LED.toggle();
        let rpm = calculate_rpm(*resources.FREQ);
        let mut out: String<U256> = String::new();

        write!(&mut out, "rpm: {:}", rpm).unwrap();
        resources.LOGGER.log(out.as_str()).unwrap();

        schedule.periodic_logger(scheduled + PERIOD_LOGGER.cycles()).unwrap();
    }

    // #[task(spawn=[periodic_logger], schedule = [tachometer], resources = [FLAG, CYCLES])]
    // fn tachometer() {
    //     // *resources.CYCLES = 0;
    //     if *resources.FLAG {
    //         *resources.CYCLES += 1;
    //     }
    //     schedule.tachometer(scheduled + PERIOD_MEASURE.cycles()).unwrap();
    // }

    // #[idle(resources = [SLEEP])]
    // fn idle() -> ! {
    //     loop {
    //         // record when this loop starts
    //         let before = Instant::now();
    //         // wait for an interrupt (sleep)
    //         wfi();
    //         // after interrupt is fired add sleep time to the sleep tracker
    //         resources
    //             .SLEEP
    //             .lock(|sleep| *sleep += before.elapsed().as_cycles());
    //     }
    // }

    #[interrupt(resources = [EXTI, FREQ, LED, CALCULATED, TIME_BEFORE, TIME_NOW])]
    fn EXTI0() {
        // the index pulse has trigger set the position to zero
        if *resources.CALCULATED == false {
            *resources.TIME_BEFORE = Instant::now();
            *resources.CALCULATED = true;
        } else {
            *resources.TIME_NOW = Instant::now();
            *resources.FREQ = (*resources.TIME_NOW - *resources.TIME_BEFORE).as_cycles();
            *resources.TIME_BEFORE = *resources.TIME_NOW;
        }
        resources.LED.toggle();
        // Set the pending register for EXTI11
        // resources.EXTI.pr.modify(|_, w| w.pr0().set_bit());
        resources.EXTI.pr.write(|w| w.pr0().set_bit());
    }

    // NOTE(elsuizo:2019-04-24): necesita que le asignemos una interrupcion el sistema operativo
    extern "C" {
        fn EXTI15_10();
    }
};

// TODO(elsuizo:2019-04-25): lo que pude ver es que se pueden tener funciones separadas de lo que
// es el OS
fn calculate_rpm(cycles: u32) -> f32 {
    60.0 / (cycles as f32 / 8_000_000 as f32)
}
