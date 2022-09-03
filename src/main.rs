use std::fs;
use actix_web::{get, middleware::Logger, post, web, App, HttpServer, Responder};
use env_logger;
use log;
use oj;
use clap::Parser;
use oj::{Config, PostJob};

#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    log::info!(target: "greet_handler", "Greeting {}", name);
    format!("Hello {name}!")
}

// DO NOT REMOVE: used in automatic testing
#[post("/internal/exit")]
#[allow(unreachable_code)]
async fn exit() -> impl Responder {
    log::info!("Shutdown as requested");
    std::process::exit(0);
    format!("Exited")
}

#[post("/jobs")]
async fn post_jobs(body:web::Json<PostJob>)->impl Responder{
    let job:PostJob=serde::Deserializer(body);

}
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let path=oj::Args::parse().config;
    let config:Config=serde_json::from_str(& fs::read_to_string(path).unwrap()).unwrap();
    println!("{}",serde_json::to_string_pretty(&config).unwrap());
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(config.clone()))
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(greet)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
    .bind(("127.0.0.1", 12345))?
    .run()
    .await
}
