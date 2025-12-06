#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use defmt::*;
use embassy_executor::{Spawner, task};
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::{Runner, StackResources};
use embassy_time::Timer;
use esp_hal::clock::CpuClock;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;

use esp_radio::wifi::event::{EventExt, StationDisconnected};
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{ModeConfig, WifiController, WifiDevice};
use esp_radio::wifi::{ScanConfig, WifiEvent, WifiStationState};
use esp_storage::FlashStorage;
use esp_wmata_pids::wmata::Client;
use esp_wmata_pids::wmata::Config;
use heapless::String;
use reqwless::client::HttpClient;
use {esp_backtrace as _, esp_println as _};

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

const SSID: Option<&str> = option_env!("SSID");
const PASSWORD: Option<&str> = option_env!("PASSWORD");
const API_KEY: Option<&str> = option_env!("API_KEY");

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 98768);
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    info!("Embassy initialized!");

    let (wifi_controller, interfaces) = unwrap!(
        esp_radio::wifi::new(peripherals.WIFI, Default::default()),
        "Failed to initialize Wi-Fi controller"
    );
    info!("wifi controller initialized");

    let device = interfaces.station;
    let config = embassy_net::Config::dhcpv4(Default::default());

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        device,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    init_wifi_handlers();

    let mut flash = FlashStorage::new(peripherals.FLASH);

    let ssid = mk_static!(String<32>, String::<32>::new());
    let pass = mk_static!(String<64>, String::<64>::new());
    let api_key = mk_static!(String<32>, String::<32>::new());

    let wmata_cfg = Config::load(&mut flash);

    if let Ok(cfg) = wmata_cfg {
        info!("found a config:\n{:?}\n", cfg);
        ssid.clear();
        ssid.push_str(cfg.ssid()).unwrap();

        pass.clear();
        pass.push_str(cfg.pass()).unwrap();

        api_key.clear();
        api_key.push_str(cfg.api_key()).unwrap();
    } else {
        info!("no valid config. loading environment variables");
        ssid.clear();
        unwrap!(
            ssid.push_str(unwrap!(SSID, "SSID not set")),
            "SSID too long"
        );

        pass.clear();
        unwrap!(
            pass.push_str(unwrap!(PASSWORD, "PASSWORD not set")),
            "PASSWORD too long"
        );

        api_key.clear();
        unwrap!(
            api_key.push_str(unwrap!(API_KEY, "API_KEY not set")),
            "API_KEY too long"
        );

        let cfg = Config::new(ssid.as_str(), pass.as_str(), api_key.as_str()).unwrap();
        if let Err(e) = cfg.save(&mut flash) {
            error!("flash error: {}", e);
        } else {
            info!("saved config:\n{:?}\n", cfg);
        }
    }

    unwrap!(
        spawner.spawn(manage_station(
            wifi_controller,
            ssid.as_str(),
            pass.as_str(),
        )),
        "failed to spawn task"
    );
    unwrap!(spawner.spawn(net_task(runner)), "failed to spawn task");

    while !stack.is_link_up() {
        Timer::after_millis(200).await;
    }

    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            break;
        }
        Timer::after_millis(500).await;
    }

    let state = mk_static!(TcpClientState<1, 4096, 4096>, TcpClientState::<1, 4096, 4096>::new());
    let mut tcp = TcpClient::new(stack, state);
    tcp.set_timeout(Some(embassy_time::Duration::from_secs(5)));
    let dns = DnsSocket::new(stack);

    let reqwless = HttpClient::new(&tcp, &dns);
    let rx_buf = mk_static!([u8; 4096], [0u8; 4096]);
    let mut client = Client::new(reqwless, rx_buf, api_key);

    loop {
        // stack may go down but it should come back up eventually
        while !stack.is_link_up() {
            Timer::after_millis(1000).await;
        }

        let trains = client
            .next_trains(esp_wmata_pids::wmata::types::Station::K04)
            .await;

        match trains {
            Ok(trains) => {
                println!("\nUpdate: ");

                let mut format_str: String<48> = String::new();
                for t in &trains {
                    format_str.clear();
                    t.write_debug_display(&mut format_str)
                        .expect("couldn't write debug display");
                    println!("{}", format_str);
                }
            }
            Err(e) => error!("{:?}", e),
        }
        Timer::after_secs(10).await;
    }
}

#[task]
async fn manage_station(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    password: &'static str,
) {
    debug!("starting manage_connection task");
    debug!("device capabilities: {:?}", controller.capabilities());

    // loop forever, keeping the controller started and the connection up
    loop {
        if esp_radio::wifi::station_state() == WifiStationState::Connected {
            // wait until we're no longer connected
            controller
                .wait_for_event(WifiEvent::StationDisconnected)
                .await;
            Timer::after_millis(5000).await;
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let station_config = ModeConfig::Station(
                StationConfig::default()
                    .with_ssid(ssid.into())
                    .with_password(password.into()),
            );
            controller.set_config(&station_config).unwrap();
            println!("Starting wifi");
            controller.start_async().await.unwrap();
            println!("Wifi started!");

            println!("Scan");
            let scan_config = ScanConfig::default().with_max(10);
            let result = controller
                .scan_with_config_async(scan_config)
                .await
                .unwrap();
            for ap in result {
                println!("{:?}", ap);
            }
        }
        println!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {:?}", e);
                Timer::after_millis(5000).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

fn init_wifi_handlers() {
    StationDisconnected::update_handler(|event| {
        debug!("EVENT: StationDisconnected - {}", event.reason());
    });
}
