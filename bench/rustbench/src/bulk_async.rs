//! Bulk scenarios for tokio-postgres and sqlx (S4 scan, S5 batch insert, S6 COPY).
//! tokio-postgres: streamed rows (`query_raw`), concurrent prepared inserts on one connection
//! (they pipeline), `copy_in`. sqlx: streamed rows, multi-row `QueryBuilder` inserts (sqlx does not
//! pipeline), `copy_in_raw`.

use crate::bulk::*;
use crate::{die, Cfg, Scenario};
use bytes::Bytes;
use futures_util::{pin_mut, SinkExt, StreamExt, TryStreamExt};
use tokio_postgres::types::ToSql;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

fn copy_chunks(rows: i64) -> impl Iterator<Item = Bytes> {
    let mut next = 1i64;
    std::iter::from_fn(move || {
        if next > rows {
            return None;
        }
        let mut s = String::new();
        while next <= rows && s.len() < 64 * 1024 {
            gen_text(next, &mut s);
            next += 1;
        }
        Some(Bytes::from(s))
    })
}

pub fn tokio_pg(cfg: &Cfg) -> (String, Vec<Pass>) {
    let url = crate::clients::tp_url_pub(&cfg.url);
    let rt = rt();
    let passes = rt.block_on(async {
        let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap_or_else(|e| die(&format!("connect: {e}")));
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let scan_st = client.prepare(S4_SQL).await.unwrap();
        let ins_st = client.prepare(INS_SQL).await.unwrap();
        let mut passes = Vec::new();
        for pass in 0..=cfg.repeats {
            let rows = if pass == 0 { 1000 } else { cfg.bulk_rows };
            let c0 = cpu_now();
            let t = std::time::Instant::now();
            let (n, sum): (u64, u64) = match cfg.scenario {
                Scenario::S4 => {
                    let stream = client
                        .query_raw(&scan_st, std::iter::empty::<&(dyn ToSql + Sync)>())
                        .await
                        .unwrap();
                    pin_mut!(stream);
                    let mut acc = Acc::default();
                    while let Some(r) = stream.next().await {
                        let r = r.unwrap();
                        acc.add(
                            r.get(0),
                            r.get(1),
                            r.get(2),
                            r.get(3),
                            r.get(4),
                            r.get(5),
                            r.get::<_, &str>(6).len(),
                            r.get::<_, &str>(7).len(),
                            r.get(8),
                            r.get(9),
                        );
                    }
                    (acc.rows, acc.sum)
                }
                Scenario::S5 => {
                    client.batch_execute("TRUNCATE bench_ins; BEGIN").await.unwrap();
                    let mut i = 1;
                    while i <= rows {
                        let end = (i + 999).min(rows);
                        let gens: Vec<GenRow> = (i..=end).map(gen_row).collect();
                        let futs = gens.iter().map(|r| {
                            let client = &client;
                            let ins_st = &ins_st;
                            async move {
                                client
                                    .execute(
                                        ins_st,
                                        &[&r.id, &r.a, &r.b, &r.c, &r.d, &r.e, &r.s1, &r.s2, &r.flag, &r.f],
                                    )
                                    .await
                            }
                        });
                        futures_util::future::try_join_all(futs).await.unwrap();
                        i = end + 1;
                    }
                    client.batch_execute("COMMIT").await.unwrap();
                    let n: i64 = client.query_one("SELECT count(*) FROM bench_ins", &[]).await.unwrap().get(0);
                    (n as u64, 0)
                }
                _ => {
                    client.batch_execute("TRUNCATE bench_ins").await.unwrap();
                    let sink = client.copy_in::<_, Bytes>(COPY_SQL).await.unwrap();
                    pin_mut!(sink);
                    for chunk in copy_chunks(rows) {
                        sink.send(chunk).await.unwrap();
                    }
                    (sink.finish().await.unwrap(), 0)
                }
            };
            if pass > 0 {
                passes.push(Pass { secs: t.elapsed().as_secs_f64(), cpu: cpu_now() - c0, rows: n, sum });
            }
        }
        passes
    });
    ("tokio-postgres 0.7".into(), passes)
}

pub fn sqlx_(cfg: &Cfg) -> (String, Vec<Pass>) {
    use sqlx::{Executor, Row};
    let rt = rt();
    let passes = rt.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .test_before_acquire(cfg.client != "sqlx-tuned")
            .connect(&cfg.url)
            .await
            .unwrap_or_else(|e| die(&format!("connect: {e}")));
        let mut passes = Vec::new();
        for pass in 0..=cfg.repeats {
            let rows = if pass == 0 { 1000 } else { cfg.bulk_rows };
            let c0 = cpu_now();
            let t = std::time::Instant::now();
            let (n, sum): (u64, u64) = match cfg.scenario {
                Scenario::S4 => {
                    let mut s = sqlx::query(S4_SQL).fetch(&pool);
                    let mut acc = Acc::default();
                    while let Some(r) = s.try_next().await.unwrap() {
                        acc.add(
                            r.get(0),
                            r.get(1),
                            r.get(2),
                            r.get(3),
                            r.get(4),
                            r.get(5),
                            r.get::<&str, _>(6).len(),
                            r.get::<&str, _>(7).len(),
                            r.get(8),
                            r.get(9),
                        );
                    }
                    (acc.rows, acc.sum)
                }
                Scenario::S5 => {
                    let mut tx = pool.begin().await.unwrap();
                    tx.execute("TRUNCATE bench_ins").await.unwrap();
                    let mut i = 1;
                    while i <= rows {
                        let end = (i + 999).min(rows);
                        let gens: Vec<GenRow> = (i..=end).map(gen_row).collect();
                        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                            "INSERT INTO bench_ins (id, a, b, c, d, e, s1, s2, flag, f) ",
                        );
                        qb.push_values(gens.iter(), |mut b, r| {
                            b.push_bind(r.id)
                                .push_bind(r.a)
                                .push_bind(r.b)
                                .push_bind(r.c)
                                .push_bind(r.d)
                                .push_bind(r.e)
                                .push_bind(&r.s1)
                                .push_bind(&r.s2)
                                .push_bind(r.flag)
                                .push_bind(r.f);
                        });
                        qb.build().execute(&mut *tx).await.unwrap();
                        i = end + 1;
                    }
                    tx.commit().await.unwrap();
                    let n: i64 = sqlx::query("SELECT count(*) FROM bench_ins").fetch_one(&pool).await.unwrap().get(0);
                    (n as u64, 0)
                }
                _ => {
                    pool.execute("TRUNCATE bench_ins").await.unwrap();
                    let mut c = pool.acquire().await.unwrap();
                    let mut copy = c.copy_in_raw(COPY_SQL).await.unwrap();
                    for chunk in copy_chunks(rows) {
                        copy.send(chunk.as_ref()).await.unwrap();
                    }
                    (copy.finish().await.unwrap(), 0)
                }
            };
            if pass > 0 {
                passes.push(Pass { secs: t.elapsed().as_secs_f64(), cpu: cpu_now() - c0, rows: n, sum });
            }
        }
        passes
    });
    ("sqlx 0.8".into(), passes)
}
