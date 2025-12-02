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
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::Controller;
use esp_radio::wifi::event::{EventExt, StaDisconnected};
use esp_radio::wifi::{WifiEvent, WifiStaState};
use esp_radio::{
    // ble::controller::BleConnector,
    wifi::{ClientConfig, ModeConfig, WifiController, WifiDevice},
};
use esp_wmata_pids::wmata::Client;
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
    // unwrap env so we can panic early
    let ssid = SSID.expect("environment variables must be set during compilation: SSID");
    let password =
        PASSWORD.expect("environment variables must be set during compilation: PASSWORD");
    let api_key = API_KEY.expect("environment variables must be set during compilation: API_KEY");

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 98768);
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    info!("Embassy initialized!");

    let esp_radio_ctrl = &*mk_static!(
        Controller<'static>,
        esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller")
    );
    info!("radio intialized");

    // let transport = BleConnector::new(&esp_radio_ctrl, peripherals.BT, Default::default()).unwrap();
    // let ble_controller = ExternalController::<_, 20>::new(transport);
    // let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
    //     HostResources::new();
    // let _ble_stack = trouble_host::new(ble_controller, &mut resources);
    // info!("bluetooth stack intialized");

    let (wifi_controller, interfaces) =
        esp_radio::wifi::new(esp_radio_ctrl, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");
    info!("wifi controller initialized");

    let device = interfaces.sta;
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

    spawner
        .spawn(manage_connection(wifi_controller, ssid, password))
        .unwrap();
    spawner.spawn(net_task(runner)).unwrap();
    init_wifi_handlers();

    while !stack.is_link_up() {
        Timer::after_millis(200).await;
    }

    let mut ip = None;
    while ip.is_none() {
        if let Some(cfg) = stack.config_v4() {
            ip = Some(cfg.address);
            info!("assigned ip: {}", cfg.address);
        }
        Timer::after_millis(200).await;
    }

    let state = mk_static!(TcpClientState<1, 4096, 4096>, TcpClientState::<1, 4096, 4096>::new());
    let tcp = TcpClient::new(stack, state);
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
                info!("\n\nUpdate: ");

                let mut format_str: String<48> = String::new();
                for t in &trains {
                    format_str.clear();
                    t.write_debug_display(&mut format_str)
                        .expect("couldn't write debug display");
                    info!("{}", format_str);
                }
            }
            Err(e) => error!("{:?}", e),
        }

        Timer::after_secs(10).await;
    }
}

#[task]
async fn manage_connection(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    password: &'static str,
) {
    debug!("starting manage_connection task");
    debug!("device capabilities: {:?}", controller.capabilities());

    let client_config = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(ssid.into())
            .with_password(password.into())
            .with_scan_method(esp_radio::wifi::ScanMethod::AllChannels)
            .with_failure_retry_cnt(10)
            .with_channel_none()
            .with_bssid_none(),
    );

    controller
        .set_config(&client_config)
        .expect("couldn't set wifi controller config");

    // for the loop below, we sleep a short time each iteration.
    // each iteration where we don't connect succesfully, increase the delay a little bit.
    // num_iters without connection
    let mut num_failures = 0u32;

    // loop forever, keeping the controller started and the connection up
    loop {
        match esp_radio::wifi::sta_state() {
            WifiStaState::Started => match controller.connect_async().await {
                Ok(_) => info!("wifi connected"),
                Err(e) => {
                    error!("Failed to connect to wifi: {:?}", e);
                    num_failures = num_failures.saturating_add(1);
                }
            },
            WifiStaState::Connected => {
                num_failures = 0;
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
            }
            WifiStaState::Disconnected => {
                // the amount of trouble in getting a disconnected device to reconnect is not worth it
                // im still convinced theres a bug in the esp-radio library, where once the controller
                // fails to connect by reason `Err(Disconnected)` then it is permanently broken until
                // the controller is restarted. i give up. just restart the whole controller every time
                warn!("disconnected. restarting the wifi controller");
                controller.disconnect_async().await.unwrap();
                controller.stop_async().await.unwrap();
                controller
                    .set_config(&client_config)
                    .expect("couldn't set wifi controller config");
            }
            WifiStaState::Stopped | WifiStaState::Invalid => match controller.start_async().await {
                Ok(_) => info!("wifi controller started"),
                Err(_) => error!("failed to start the wifi controller"),
            },
            _ => {
                error!("Unknown wifi state");
            }
        }
        let delay_ms = reconnect_backoff_ms(num_failures);
        if delay_ms > 0 {
            warn!("too many connection attempts. sleeping for {}ms", delay_ms);
            Timer::after_millis(reconnect_backoff_ms(num_failures)).await;
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

fn reconnect_backoff_ms(failures: u32) -> u64 {
    const DELAY_MAX_MS: u64 = 10_000;

    if failures < 3 {
        return 0;
    }

    let step = 1u64 << (failures - 3); // 1, 2, 4, 8, ...
    (500 * step).min(DELAY_MAX_MS)
}

fn init_wifi_handlers() {
    StaDisconnected::update_handler(|event| debug!("EVENT: StaDisconnected - {}", event.reason()));
}
