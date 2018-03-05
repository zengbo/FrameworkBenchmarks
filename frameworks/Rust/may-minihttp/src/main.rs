#[macro_use]
extern crate may;
extern crate may_minihttp;
extern crate num_cpus;
extern crate postgres;
extern crate rand;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;

use std::io;
use std::ops::Deref;

use rand::Rng;
use postgres::{Connection, TlsMode};
use may::sync::mpmc::{self, Receiver, Sender};
use may_minihttp::{HttpServer, HttpService, Request, Response};

// thread_local!(static RNG: ThreadRng = rand::thread_rng());

#[derive(Serialize)]
struct WorldRow {
    id: i32,
    randomnumber: i32,
}

struct Techempower {
    db_rx: Receiver<Connection>,
    db_tx: Sender<Connection>,
}

struct PgConnection {
    conn: Option<Connection>,
    tx: Sender<Connection>,
}

impl Deref for PgConnection {
    type Target = Connection;

    #[inline]
    fn deref(&self) -> &Connection {
        self.conn.as_ref().unwrap()
    }
}

impl Drop for PgConnection {
    fn drop(&mut self) {
        let conn = self.conn.take().unwrap();
        self.tx.send(conn).unwrap();
    }
}

impl Techempower {
    fn get_conn(&self) -> PgConnection {
        PgConnection {
            conn: Some(self.db_rx.recv().unwrap()),
            tx: self.db_tx.clone(),
        }
    }

    fn random_world_row(&self) -> io::Result<WorldRow> {
        let conn = self.get_conn();
        let stmt = conn.prepare_cached(
            "SELECT id,randomNumber \
             FROM World WHERE id = $1",
        )?;

        let random_id = rand::thread_rng().gen_range(1, 10_000);
        let rows = &stmt.query(&[&random_id])?;
        let row = rows.get(0);

        Ok(WorldRow {
            id: row.get(0),
            randomnumber: row.get(1),
        })
    }
}

impl HttpService for Techempower {
    fn call(&self, req: Request) -> io::Result<Response> {
        let mut resp = Response::new();

        // Bare-bones router
        match req.path() {
            "/json" => {
                resp.header("Content-Type", "application/json");
                *resp.body_mut() =
                    serde_json::to_vec(&json!({"message": "Hello, World!"})).unwrap();
            }
            "/plaintext" => {
                resp.header("Content-Type", "text/plain")
                    .body("Hello, World!");
            }
            "/db" => {
                let msg = self.random_world_row().expect("failed to get random world");
                resp.header("Content-Type", "application/json");
                *resp.body_mut() = serde_json::to_vec(&msg).unwrap();
            }
            _ => {
                resp.status_code(404, "Not Found");
            }
        }

        Ok(resp)
    }
}

fn main() {
    // may::config().set_io_workers(num_cpus::get());
    may::config()
        .set_io_workers(num_cpus::get())
        .set_workers(num_cpus::get());

    let (db_tx, db_rx) = mpmc::channel();

    let dbhost = match option_env!("DBHOST") {
        Some(it) => it,
        _ => "localhost",
    };
    let db_url = format!(
        "postgres://benchmarkdbuser:benchmarkdbpass@{}/hello_world",
        dbhost
    );
    join!(for _ in 0..(num_cpus::get() * 4) {
        let conn = Connection::connect(db_url.as_str(), TlsMode::None).unwrap();
        db_tx.send(conn).unwrap();
    });

    let server = HttpServer(Techempower { db_rx, db_tx })
        .start("0.0.0.0:8080")
        .unwrap();
    server.join().unwrap();
}
