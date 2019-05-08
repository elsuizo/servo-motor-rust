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
use rtfm::{app, Instant};
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

type PIN = gpio::gpiob::PB0<gpio::Input<gpio::PullUp>>;
//-------------------------------------------------------------------------
//                        app
//-------------------------------------------------------------------------

const PERIOD_LOGGER: u32 = 8_000_000;
const PERIOD_MEASURE: u32 = 800;

#[app(device = stm32f1xx_hal::stm32)]
const APP: () = {
    // recursos que vamos a utilizar
    // static mut BUZZER: BUZZER = ();
    static mut LED: LED = ();
    static mut PIN: PIN = ();
    static mut COUNTER: u32 = ();
    static mut POSITION: u16 = ();
    static mut FLAG: bool = ();
    static mut EXTI: stm32f1xx_hal::device::EXTI = ();
    static mut LOGGER: logger::Logger = ();
    static mut ENCODER: stm32f1xx_hal::qei::Qei<TIM4, PINS> = ();

    #[init(schedule = [periodic_logger])]
    fn init() -> init::LateResources {

        //-------------------------------------------------------------------------
        //                   interrupt initialization
        //-------------------------------------------------------------------------
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
        let mut pin = gpiob.pb0.into_pull_up_input(&mut gpiob.crl);

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
        let counter: u32 = 0;
        let position: u16 = 0;
        let flag: bool = false;
        let c1 = gpiob.pb6;
        let c2 = gpiob.pb7;
        // Quadrature Encoder Interface
        let qei = Qei::tim4(device.TIM4, (c1, c2), &mut afio.mapr, &mut rcc.apb1);

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
        //-------------------------------------------------------------------------
        //                        resources
        //-------------------------------------------------------------------------
        schedule.periodic_logger(Instant::now() + PERIOD_LOGGER.cycles()).unwrap();

        init::LateResources {
            LED: led,
            // BUZZER: buzzer_output,
            COUNTER: counter,
            POSITION: position,
            FLAG: flag,
            EXTI: exti,
            LOGGER: logger,
            ENCODER: qei,
            PIN: pin,
        }

    }

    #[task(schedule = [periodic_logger], resources = [LOGGER, ENCODER, POSITION, FLAG, LED, PIN])]
    fn periodic_logger() {

        *resources.POSITION = resources.ENCODER.count();
        if *resources.FLAG {
            *resources.POSITION = 0;
        }
        if resources.PIN.is_high() {
            resources.LED.toggle();
        }
        // toggle for debug
        // resources.LED.toggle();
        // let f = calculate_rpm(*resources.COUNTER);
        let mut out: String<U256> = String::new();
        write!(&mut out, "Position: {}", *resources.POSITION).unwrap();
        resources.LOGGER.log(out.as_str()).unwrap();
        // *resources.COUNTER = 0;

        schedule.periodic_logger(scheduled + PERIOD_LOGGER.cycles()).unwrap();
    }


    // #[idle]
    // fn idle() -> ! {
    //     // NOTE(elsuizo:2019-04-23): este pend es importante porque sino se queda
    //     // la interrupcion colgada
    //     rtfm::pend(Interrupt::EXTI0);
    //     // NOTE(elsuizo:2019-04-23): debemos poner el loop porque estamos diciendo que no devuelve
    //     // nada
    //     loop {
    //
    //     }
    // }

    #[interrupt(resources = [COUNTER, EXTI, POSITION, FLAG, LED])]
    fn EXTI0() {
        *resources.COUNTER += 1;
        // the index pulse has trigger set the position to zero
        // resources.LED.toggle();
        if *resources.FLAG {
            *resources.FLAG = false;
        } else {
            *resources.FLAG = true;
        }
        // Set the pending register for EXTI11
        resources.EXTI.pr.modify(|_, w| w.pr11().set_bit());
    }

    // NOTE(elsuizo:2019-04-24): necesita que le asignemos una interrupcion el sistema operativo
    extern "C" {
        fn EXTI15_10();
    }
};

// TODO(elsuizo:2019-04-25): lo que pude ver es que se pueden tener funciones separadas de lo que
// es el OS
fn calculate_rpm(frequency: u32) -> f32 {
    frequency as f32 * 60.0
}
