use actix_web::{web, App, HttpServer};
use std::{
    net::{IpAddr, Ipv4Addr},
    panic, process,
    sync::{mpsc, Arc},
    thread, time,
};
use tokio::signal;

use uaas::{
    config::get_config,
    db, migrate,
    peer_event::{PeerEventMessage, PeerEventType},
    rate_limit::RateLimiter,
    rest_api::{add_monitor, broadcast_tx, delete_monitor, health, version, AppState},
    thread_manager::ThreadManager,
    thread_tracker::ThreadTracker,
    thread_util::catch_unwind_logged,
    uaas::logic::Logic,
};

const USAGE: &str = "\
usage:
  uaas                      run the indexing service
  uaas migrate [URL]        apply the PostgreSQL schema migrations

`migrate` takes a libpq connection URL, or reads UAAS_POSTGRES_URL.
It is safe to run repeatedly: migrations already applied are skipped.";

// Deliberately not `#[actix_web::main]`. That attribute wraps the whole of
// `main` in a tokio runtime, and the synchronous `postgres` client drives its
// own runtime internally — calling it from inside one panics with "Cannot start
// a runtime from within a runtime". So `migrate` has to run before any runtime
// exists, and the service enters one explicitly afterwards. The expansion below
// is what the attribute would have generated.
fn main() {
    // One subcommand, matched by hand. A CLI parser would be a dependency
    // earning its keep only once there is a second flag to parse.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("migrate") => {
            if let Err(err) = run_migrate(args.get(1).map(String::as_str)) {
                // {err:#} so anyhow's context chain is printed, not just the
                // outermost message: "applying V5__utxo: relation already
                // exists" is actionable, "applying V5__utxo" is not.
                eprintln!("migrate failed: {err:#}");
                process::exit(1);
            }
            return;
        }
        Some("--help" | "-h" | "help") => {
            println!("{USAGE}");
            return;
        }
        Some(other) => {
            eprintln!("unknown argument {other:?}\n\n{USAGE}");
            process::exit(2);
        }
    }

    if let Err(err) = start() {
        eprintln!("Fatal startup error: {err}");
        process::exit(1);
    }
}

/// Applies the schema migrations and exits. Does not start the service.
///
/// Separate from `run` on purpose: this is the step that runs once per deploy,
/// before the service starts, and it must be possible to run it without
/// bringing anything else up.
fn run_migrate(url_arg: Option<&str>) -> anyhow::Result<()> {
    let url = match url_arg {
        Some(url) => url.to_string(),
        None => std::env::var("UAAS_POSTGRES_URL").map_err(|_| {
            anyhow::anyhow!("no connection URL: pass one as an argument or set UAAS_POSTGRES_URL")
        })?,
    };

    let mut client = postgres::Client::connect(&url, postgres::NoTls)
        .map_err(|err| anyhow::anyhow!("could not connect to PostgreSQL: {err}"))?;

    let report = migrate::run(&mut client)?;
    if report.applied.is_empty() {
        println!(
            "schema already at version {}, {} migrations previously applied",
            report.version, report.already_applied
        );
    } else {
        for name in &report.applied {
            println!("applied {name}");
        }
        println!("schema now at version {}", report.version);
    }
    Ok(())
}

/// Brings the service up. Deliberately synchronous.
///
/// Every database access at startup has to happen before a runtime exists.
/// r2d2 validates a connection as it hands it out — `PostgresConnectionManager`
/// implements `is_valid` with a query — so `Pool::get` performs synchronous I/O
/// on the *calling* thread, and the synchronous postgres client drives a tokio
/// runtime of its own to do it. A `get` on a thread already inside a runtime
/// therefore panics with "Cannot start a runtime from within a runtime", and
/// the panic recurs in the client's destructor during cleanup, which turns it
/// into an abort.
///
/// That rules out the whole startup sequence running inside `block_on`: the
/// schema check and `Logic::new` both take connections. So the runtime is
/// entered at the very end, for the web server alone — and the one place the
/// web layer touches the pool, `rest_api::check_database`, already moves onto a
/// plain thread to do it.
fn start() -> Result<(), String> {
    // Log panics without terminating unrelated threads (for example the web server).
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        log::error!("Thread panic: {panic_info}");
        orig_hook(panic_info);
    }));

    let config = get_config("UAASR_CONFIG", "../data/uaasr.toml")?;

    simple_logger::init_with_level(config.get_log_level())
        .map_err(|err| format!("failed to initialize logger: {err}"))?;

    config.validate_startup()?;

    let server_address = config.service.rust_address.clone();
    let (tx_rest, rx_rest) = mpsc::channel();

    let rate_limiter = Arc::new(RateLimiter::new(config.web_interface.rate_limit_per_minute));
    let max_broadcast_tx_bytes = config.web_interface.max_broadcast_tx_bytes;
    let payload_limit = max_broadcast_tx_bytes.saturating_mul(2).max(1024);

    // Resolved before the pool is built so a placeholder is refused with a
    // message about what to set, rather than surfacing as a URL parse failure.
    let postgres_url = config.get_postgres_url()?;
    let db_pool = db::build_pool(&postgres_url).map_err(|err| {
        log::error!("Problem connecting to database: {err:#}");
        format!(
            "Problem connecting to database. Check the database is running and \
             that UAAS_POSTGRES_URL or database.postgres_url is correct: {err:#}"
        )
    })?;

    // Keeps the pool alive past the runtime. Closing a postgres client blocks
    // on its internal runtime exactly as `get` does, so the last `Pool` clone
    // must not be dropped inside `block_on`. Holding one here means the web
    // layer's clone drops to a live refcount, and the real teardown happens on
    // this thread once the runtime is gone.
    let _pool_guard = db_pool.clone();

    // Refuse to run against a schema this build does not understand, before any
    // query is issued. A mismatch here is a deployment mistake, and it should
    // say so rather than surface later as a column that has changed meaning.
    {
        let mut conn = db_pool
            .get()
            .map_err(|err| format!("could not take a connection from the pool: {err}"))?;
        migrate::assert_expected_version(&mut conn)
            .map_err(|err| format!("schema check failed: {err:#}"))?;
    }

    let app_state = AppState {
        msg_from_rest_api: tx_rest,
        api_key: config.web_interface.api_key.clone(),
        rate_limiter,
        max_broadcast_tx_bytes,
        db_pool: db_pool.clone(),
    };

    let mut logic = Logic::new(&config, db_pool)?;
    logic.setup();

    let mut children = ThreadTracker::new();
    let mut manager = ThreadManager::new(rx_rest);
    let tx = manager.get_tx();

    let ips = config.get_ips()?;

    // Start the peer threads. A plain thread, never inside a runtime, which is
    // what lets the indexing side use the pool freely.
    let handle = thread::spawn(move || {
        catch_unwind_logged("peer manager", || {
            for ip in ips.into_iter().cycle() {
                manager.create_thread(ip, &mut children, &config);
                if manager.process_messages(&mut children, &mut logic) {
                    break;
                }
            }
        });
    });

    actix_web::rt::System::new().block_on(serve(app_state, server_address, payload_limit, tx))?;

    // Wait for peer threads
    if handle.join().is_err() {
        log::error!("Peer manager thread panicked during shutdown");
    }

    Ok(())
}

/// Runs the web server until shutdown. The only part of the service inside a
/// tokio runtime.
async fn serve(
    app_state: AppState,
    server_address: String,
    payload_limit: usize,
    tx_stop: mpsc::Sender<PeerEventMessage>,
) -> Result<(), String> {
    let web_state = web::Data::new(app_state);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::PayloadConfig::default().limit(payload_limit))
            .app_data(web_state.clone())
            .service(health)
            .service(broadcast_tx)
            .service(version)
            .service(add_monitor)
            .service(delete_monitor)
    })
    .workers(1)
    .bind(&server_address)
    .map_err(|err| format!("failed to bind web server to {server_address}: {err}"))?
    .run();

    let server_handle = server.handle();

    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        log::info!("Shutdown requested, stopping peer threads and web server...");
        let stop_msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            event: PeerEventType::Stop,
        };
        if tx_stop.send(stop_msg).is_err() {
            log::warn!("Failed to send stop message to peer manager");
        }
        server_handle.stop(true).await;
    });

    server
        .await
        .map_err(|err| format!("web server error: {err}"))
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            log::error!("failed to install Ctrl+C handler: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                log::error!("failed to install SIGTERM handler: {err}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
