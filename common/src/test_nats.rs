//! A JetStream server of a test's own, for tests of code that coordinates
//! instances through NATS. Tests using it need `nats-server` on PATH, so
//! they are marked `#[ignore = "needs nats-server on PATH"]` and run with
//! `cargo test -- --include-ignored`.

use async_nats::jetstream;

/// A running JetStream server, gone when dropped.
pub struct Server {
    child: std::process::Child,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Server {
    /// Starts a server on a free local port, storing into a temporary
    /// directory.
    pub async fn start() -> Server {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dir = tempfile::tempdir().unwrap();
        let child = std::process::Command::new("nats-server")
            .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-js", "-sd"])
            .arg(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("nats-server must be on PATH");

        let server = Server {
            child,
            port,
            _dir: dir,
        };

        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                return server;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("nats-server did not start");
    }

    /// A JetStream context over a connection of its own, as a separate
    /// instance of a service would have.
    pub async fn jetstream(&self) -> jetstream::Context {
        let client = async_nats::connect(format!("127.0.0.1:{}", self.port))
            .await
            .unwrap();
        jetstream::new(client)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
