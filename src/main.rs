use actix_web::{get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use chrono::prelude::*;
use clap::Parser;
use env_logger;
use log;
use oj;
use oj::{check_job, Config, PostJob};
use std::borrow::BorrowMut;
use std::fs;
use std::fs::create_dir;
use std::process::{Command, Stdio};

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
async fn post_jobs(body: web::Json<PostJob>, config: web::Data<Config>) -> impl Responder {
    // if !check_job(&body.0, config.get_ref()) {
    //     return HttpResponse::BadRequest().json(
    //         "{reason=ERR_NOT_FOUND, code=3, HTTP 404 Not Found}"
    //     );
    // }
    let temp_dir=format!("./problem{}",body.problem_id);
    let temp_file=format!("{}/user_{}.rs",temp_dir,body.user_id);
    let temp_bin=format!("{}/user_{}",temp_dir,body.user_id);
    create_dir(temp_dir).unwrap();
    fs::File::create(&temp_file).unwrap();
    fs::write(&temp_file,&body.source_code).unwrap();
    let build_job=std::process::Command::new("rustc")
        .arg(&temp_file)
        .arg("-o")
        .arg(&temp_bin)
        .status();
    let run_job=std::process::Command::new(&temp_bin)
        .stdout(Stdio::piped())
        .output().unwrap();
    println!("{}",String::from_utf8(run_job.stdout).unwrap());
    HttpResponse::Ok().json("Ok")
    // format!("{}", body.language)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let path = oj::Args::parse().config;
    let config: Config = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(config.clone()))
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(greet)
            .service(post_jobs)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
    .bind(("127.0.0.1", 12345))?
    .run()
    .await
}
