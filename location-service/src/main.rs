pub mod health;
pub mod ip;

use async_nats::service::ServiceExt;
use common::{handler, shared::nats};
use futures::StreamExt;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    common::shared::init("ip2location").await;

    let nats = nats();

    let service = nats
        .service_builder()
        .start("user", env!("CARGO_PKG_VERSION"))
        .await
        .expect("Unable to start user service");

    let session = service.group("location");

    let mut toplevel = JoinSet::new();

    toplevel.spawn(handler!(session, "ip", ip::ip));
    toplevel.spawn(handler!(session, "ipv4", ip::ipv4));
    toplevel.spawn(handler!(session, "ipv6", ip::ipv6));

    let health = service.group("location.health");
    toplevel.spawn(handler!(health, "check", health::health_check));

    // Set up graceful shutdown
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");

    tokio::select! {
        _ = toplevel.join_all() => {
            println!("All service handlers completed");
        }
        _ = sigint.recv() => {
            println!("Received SIGINT, shutting down location service gracefully...");
        }
        _ = sigterm.recv() => {
            println!("Received SIGTERM, shutting down location service gracefully...");
        }
    }

    println!("Location service shutdown complete");
}
